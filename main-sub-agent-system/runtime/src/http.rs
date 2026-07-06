use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use agent_core::context::AgentContext;
use agent_core::error::{AgentTeamsError, ApiResponse};
use agent_core::message::AgentMessage;
use agent_core::registry::AgentRegistry;
use secrecy::{ExposeSecret, SecretString};

use agent_teams_coordinator::MainAgentCoordinator;

use crate::events::{AgentProgressSseEvent, DeltaEvent, DoneEvent, ErrorEvent, SubAgentResultsEvent, ToolStatusSseEvent};
use crate::validation::ValidatedChatRequest;
use utoipa::OpenApi;

// ─── Error handling ──────────────────────────────────────────────

pub struct AppError(pub AgentTeamsError);

impl From<AgentTeamsError> for AppError {
    fn from(err: AgentTeamsError) -> Self {
        AppError(err)
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self.0 {
            AgentTeamsError::ProviderAuth(_) => (axum::http::StatusCode::UNAUTHORIZED, 40101),
            AgentTeamsError::ProviderRateLimit { .. } => {
                (axum::http::StatusCode::TOO_MANY_REQUESTS, 42901)
            }
            AgentTeamsError::Provider(_) => (axum::http::StatusCode::BAD_GATEWAY, 50201),
            AgentTeamsError::AgentTimeout { .. } => {
                (axum::http::StatusCode::GATEWAY_TIMEOUT, 50401)
            }
            AgentTeamsError::ConfigError(_) | AgentTeamsError::ConfigValidation(_) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, 50001)
            }
            AgentTeamsError::ToolNotFound(_) => (axum::http::StatusCode::NOT_FOUND, 40401),
            AgentTeamsError::HookHalt { .. } => (axum::http::StatusCode::FORBIDDEN, 40301),
            _ => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, 50000),
        };
        let body = Json(ApiResponse::<()>::error(code, self.0.to_string()));
        (status, body).into_response()
    }
}

// ─── Metrics (internal) ──────────────────────────────────────────
// Note: agent_execution_{durations,counts} DashMaps are bounded by the number of
// registered agents (configured at startup). They do not grow unboundedly.

#[derive(Debug, Default)]
pub struct Metrics {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub cache_hits: AtomicU64,
    pub total_duration_ms: AtomicU64,
    pub thinking_used_count: AtomicU64,
    pub plan_generation_duration_ms: AtomicU64,
    pub plan_generation_count: AtomicU64,
    pub llm_input_tokens: AtomicU64,
    pub llm_output_tokens: AtomicU64,
    pub llm_cached_tokens: AtomicU64,
    pub pipeline_stage_success: AtomicU64,
    pub pipeline_stage_failure: AtomicU64,
    pub agent_execution_durations: dashmap::DashMap<String, AtomicU64>,
    pub agent_execution_counts: dashmap::DashMap<String, AtomicU64>,
    pub forced_sub_agent_calls: AtomicU64,
    pub sub_agent_skip_prevented: AtomicU64,
    pub memory_sync_latency_us: AtomicU64,
    pub memory_sync_count: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_agent_duration(&self, agent_id: &str, duration_ms: u64) {
        self.agent_execution_durations
            .entry(agent_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.agent_execution_counts
            .entry(agent_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_token_usage(&self, input_tokens: u32, output_tokens: u32, cached_tokens: u32) {
        self.llm_input_tokens
            .fetch_add(input_tokens as u64, Ordering::Relaxed);
        self.llm_output_tokens
            .fetch_add(output_tokens as u64, Ordering::Relaxed);
        self.llm_cached_tokens
            .fetch_add(cached_tokens as u64, Ordering::Relaxed);
    }

    pub fn record_plan_generation(&self, duration_ms: u64) {
        self.plan_generation_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.plan_generation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_pipeline_stage(&self, success: bool) {
        if success {
            self.pipeline_stage_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.pipeline_stage_failure.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_forced_sub_agent_call(&self) {
        self.forced_sub_agent_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_skip_prevented(&self) {
        self.sub_agent_skip_prevented
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_memory_sync(&self, latency_us: u64) {
        self.memory_sync_latency_us
            .fetch_add(latency_us, Ordering::Relaxed);
        self.memory_sync_count.fetch_add(1, Ordering::Relaxed);
    }
}

// ─── Application state ───────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub coordinator: Arc<MainAgentCoordinator>,
    pub registry: Arc<AgentRegistry>,
    pub metrics: Arc<Metrics>,
    pub cache_metrics: Arc<agent_teams_coordinator::cache_metrics::CacheMetrics>,
    pub tool_registry: Arc<agent_core::tool::UnifiedToolRegistry>,
    pub tool_engine: Arc<agent_teams_agents::tool_engine::ToolExecutionEngine>,
    pub provider: Arc<dyn agent_core::provider::LlmProvider>,
    pub api_keys: Arc<Vec<SecretString>>,
    pub default_model: String,
    pub presets: Arc<Vec<PresetDef>>,
    pub pipeline_timeout_secs: u64,
    pub rate_limiter: Option<crate::rate_limit::RateLimiter>,
}

// ─── Auth middleware ──────────────────────────────────────────────

async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    if state.api_keys.is_empty() {
        return next.run(req).await;
    }

    let provided_key = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            req.headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
        });

    match provided_key {
        Some(key) if state.api_keys.iter().any(|k| constant_time_eq(k.expose_secret().as_bytes(), key.as_bytes())) => {
            next.run(req).await
        }
        _ => {
            let body = Json(ApiResponse::<()>::error(40101, "Unauthorized: invalid or missing API key".to_string()));
            (axum::http::StatusCode::UNAUTHORIZED, body).into_response()
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─── Types ───────────────────────────────────────────────────────

/// Built-in preset persona definition
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PresetDef {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub system_instructions: Vec<String>,
}

/// Request body for POST /chat
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ChatRequest {
    pub message: String,
    pub session_id: Option<String>,
    pub recent_history: Option<Vec<serde_json::Value>>,
    /// System instructions (role/persona). Standard chat API field.
    pub system_instructions: Option<Vec<String>>,
    /// Stream mode: "simple" (text only) or "full" (all events). Default: "full".
    /// Simple mode is for external consumers — only emits delta and done events.
    pub stream_mode: Option<String>,
    /// Force use of a specific tool (internal, ignored in simple mode)
    #[serde(default)]
    pub force_tool: Option<String>,
    /// Enable companion mode (emotional state tracking and injection)
    #[serde(default)]
    pub companion_mode: Option<bool>,
}

/// Build AgentContext from a ChatRequest
fn build_context(req: ChatRequest, session_id: &str) -> (AgentContext, AgentMessage) {
    let ctx = AgentContext {
        session_id: session_id.to_string(),
        recent_history: Arc::new(
            req.recent_history
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| {
                    entry
                        .get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
                })
                .collect(),
        ),
        system_instructions: Arc::new(req.system_instructions.unwrap_or_default()),
        ..Default::default()
    };
    let mut msg = AgentMessage::new(req.message).with_session(session_id);
    if let Some(tool) = req.force_tool {
        msg = msg.with_force_tool(tool);
    }
    (ctx, msg)
}

// ─── Session instruction store (internal) ────────────────────────

use dashmap::DashMap;
use std::sync::LazyLock;
use std::time::Instant;

struct SessionEntry {
    instructions: Vec<String>,
    last_accessed: Instant,
}

static SESSION_INSTRUCTIONS: LazyLock<DashMap<String, SessionEntry>> = LazyLock::new(DashMap::new);

/// Companion state with last-access tracking for TTL eviction
struct CompanionEntry {
    state: agent_core::companion::CompanionState,
    last_accessed: Instant,
}

/// Per-session companion states (bounded by periodic eviction)
static COMPANION_STATES: LazyLock<DashMap<String, CompanionEntry>> = LazyLock::new(DashMap::new);

/// Evict sessions not accessed in the last hour.
/// Runs unconditionally — called by periodic background task.
fn evict_stale_sessions() {
    let now = Instant::now();
    let duration = std::time::Duration::from_secs(3600);
    let cutoff = now.checked_sub(duration).unwrap_or(now);
    SESSION_INSTRUCTIONS.retain(|_, entry| entry.last_accessed > cutoff);
    COMPANION_STATES.retain(|_, entry| entry.last_accessed > cutoff);
}

/// Extract companion delta from sentiment sub-agent results and apply to session state.
/// Returns the updated companion state if a delta was found and applied.
fn apply_companion_delta(
    sub_results: &[agent_core::provider::SubAgentResultSummary],
    session_id: &str,
) -> Option<agent_core::companion::CompanionState> {
    let sentiment = sub_results.iter().find(|r| r.agent_id == "sentiment")?;
    let json: serde_json::Value = serde_json::from_str(&sentiment.content_summary).ok()?;
    let delta_json = json.get("companion_delta")?;

    let delta = agent_core::companion::CompanionDelta {
        mood: delta_json.get("mood").and_then(|v| v.as_str()).map(|s| s.to_string()),
        mood_intensity: delta_json.get("mood_intensity").and_then(|v| v.as_f64()).map(|v| v as f32),
        affinity_delta: delta_json.get("affinity_delta").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        energy_delta: delta_json.get("energy_delta").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        patience_delta: delta_json.get("patience_delta").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        trust_delta: delta_json.get("trust_delta").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        reason: delta_json.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        sticker: json.get("sticker").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    };

    let mut entry = COMPANION_STATES.entry(session_id.to_string())
        .or_insert_with(|| CompanionEntry {
            state: agent_core::companion::CompanionState::default(),
            last_accessed: Instant::now(),
        });
    entry.state.apply(&delta);
    entry.last_accessed = Instant::now();
    Some(entry.state.clone())
}

/// Start a periodic background cleanup task for the session store.
/// Call once at server startup.
pub fn start_session_cleanup_task() {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await; // every 5 min
            evict_stale_sessions();
        }
    });
}

pub fn get_session_instructions(session_id: &str) -> Option<Vec<String>> {
    // Single get_mut call — atomic read + last_accessed update
    SESSION_INSTRUCTIONS
        .get_mut(session_id)
        .map(|mut entry| {
            entry.last_accessed = Instant::now();
            entry.instructions.clone()
        })
}

pub fn insert_session(session_id: String, instructions: Vec<String>) {
    // Background cleanup task handles eviction — avoid synchronous retain()
    // on the hot path which would block all DashMap operations.
    SESSION_INSTRUCTIONS.insert(
        session_id,
        SessionEntry {
            instructions,
            last_accessed: Instant::now(),
        },
    );
}

pub fn remove_session(session_id: &str) {
    SESSION_INSTRUCTIONS.remove(session_id);
}

// ─── Handlers ────────────────────────────────────────────────────

/// POST /chat — streaming chat (primary entry point)
///
/// Returns an SSE stream. Use `stream_mode: "simple"` for text-only output
/// suitable for external consumers.
#[utoipa::path(
    post,
    path = "/chat",
    request_body = ChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat responses", content_type = "text/event-stream"),
        (status = 400, description = "Validation error"),
        (status = 401, description = "Unauthorized — invalid or missing API key"),
    ),
    tag = "chat"
)]
async fn chat_handler(
    State(state): State<AppState>,
    ValidatedChatRequest(mut req): ValidatedChatRequest,
) -> Sse<std::pin::Pin<Box<dyn Stream<Item = Result<Event, std::convert::Infallible>> + Send>>> {
    state.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
    let metrics = state.metrics.clone();

    let session_id = req
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Guard against session exhaustion attacks.
    // Background cleanup task handles eviction — avoid synchronous retain() here.
    if SESSION_INSTRUCTIONS.len() > 50_000 {
        let err = ErrorEvent::new("Too many active sessions. Please try again later.");
        let stream = async_stream::stream! {
            yield Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()));
        };
        return Sse::new(Box::pin(stream));
    }

    // Load system instructions from session store if not provided in request
    if req.system_instructions.is_none() {
        if let Some(instructions) = get_session_instructions(&session_id) {
            req.system_instructions = Some(instructions);
        }
    }

    // Persist system instructions for this session
    if let Some(ref instructions) = req.system_instructions {
        if !instructions.is_empty() {
            SESSION_INSTRUCTIONS.insert(
                session_id.clone(),
                SessionEntry {
                    instructions: instructions.clone(),
                    last_accessed: Instant::now(),
                },
            );
        }
    }

    // Determine stream mode: "simple" = text-only, "full" = all events
    let simple_mode = req.stream_mode.as_deref() == Some("simple");
    let companion_mode = req.companion_mode.unwrap_or(false);

    // In simple mode, ignore force_tool (external consumers shouldn't know about tools)
    if simple_mode {
        req.force_tool = None;
    }

    // Companion mode: inject current emotional state into system instructions
    if companion_mode {
        let companion_state = COMPANION_STATES
            .entry(session_id.clone())
            .or_insert_with(|| CompanionEntry {
                state: agent_core::companion::CompanionState::default(),
                last_accessed: Instant::now(),
            })
            .state
            .clone();
        let companion_desc = companion_state.to_prompt_description();
        let mut instructions = req.system_instructions.take().unwrap_or_default();
        instructions.push(companion_desc);
        req.system_instructions = Some(instructions);
    }

    let (ctx, msg) = build_context(req, &session_id);

    let stream_session_id = session_id.clone();

    let stream = async_stream::stream! {
        let stream_start = std::time::Instant::now();
        let pipeline_timeout = std::time::Duration::from_secs(state.pipeline_timeout_secs);

        let chunk_stream = match tokio::time::timeout(
            pipeline_timeout,
            state.coordinator.handle_request_stream(&ctx, &msg),
        ).await {
            Ok(stream) => stream,
            Err(_) => {
                metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
                let err = ErrorEvent::new("Request timed out");
                yield Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()));
                return;
            }
        };
        let mut chunk_stream = chunk_stream;

        while let Some(chunk_result) = futures::StreamExt::next(&mut chunk_stream).await {
            match chunk_result {
                Ok(chunk) => {
                    if chunk.done {
                        metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
                        metrics.total_duration_ms.fetch_add(
                            stream_start.elapsed().as_millis() as u64, Ordering::Relaxed
                        );
                    }

                    if simple_mode {
                        // Simple mode: only emit text delta and done (no internal details)
                        if chunk.delta.is_empty() && !chunk.done {
                            continue;
                        }
                        if chunk.done {
                            let ev = DoneEvent::new(&chunk.delta, None);
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        } else {
                            let ev = DeltaEvent::new(&chunk.delta, None, None);
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        }
                    } else {
                        // Full mode: emit all events (for internal frontend)
                        if let Some(ref sub_agent_results) = chunk.sub_agent_results {
                            // Companion mode: extract and apply companion delta
                            if companion_mode {
                                if let Some(updated) = apply_companion_delta(sub_agent_results, &stream_session_id) {
                                    let comp_event = serde_json::json!({
                                        "type": "companion_state",
                                        "companion_state": updated,
                                    });
                                    yield Ok(Event::default().data(serde_json::to_string(&comp_event).unwrap_or_default()));
                                }
                            }

                            let ev = SubAgentResultsEvent::new(sub_agent_results.clone());
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        }

                        if let Some(ref annotations) = chunk.annotations {
                            // Emit annotations (e.g., web search citations) as SSE event
                            let ev = serde_json::json!({
                                "type": "annotations",
                                "annotations": annotations,
                                "done": false,
                            });
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        } else if let Some(ref tool_status) = chunk.tool_status {
                            let ev = ToolStatusSseEvent::new(tool_status.clone());
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        } else if let Some(ref agent_progress) = chunk.agent_progress {
                            let ev = AgentProgressSseEvent::new(agent_progress.clone());
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        } else if chunk.done {
                            let ev = DoneEvent::new(&chunk.delta, chunk.usage.clone());
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        } else {
                            let ev = DeltaEvent::new(&chunk.delta, chunk.thinking_delta.clone(), chunk.usage.clone());
                            yield Ok(Event::default().data(serde_json::to_string(&ev).unwrap_or_default()));
                        }
                    }
                }
                Err(e) => {
                    metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
                    let err = ErrorEvent::new(e.to_string());
                    yield Ok(Event::default().data(serde_json::to_string(&err).unwrap_or_default()));
                    break;
                }
            }
        }
    };

    Sse::new(Box::pin(stream))
}

/// GET /health — system status
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "System health status"),
    ),
    tag = "system"
)]
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": "1.4.0",
        "provider": state.provider.id(),
        "model": state.default_model,
    }))
}

/// GET /tools — list available tools (informational)
#[utoipa::path(
    get,
    path = "/tools",
    responses(
        (status = 200, description = "List of available tools"),
    ),
    tag = "system"
)]
async fn tools_handler(State(state): State<AppState>) -> impl IntoResponse {
    let tools = state.tool_registry.list_tools();
    let items: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": {
                    "schema": t.parameters.schema,
                    "required": t.parameters.required,
                },
            })
        })
        .collect();
    Json(serde_json::json!({ "tools": items }))
}

/// GET /presets — list built-in preset personas
#[utoipa::path(
    get,
    path = "/presets",
    responses(
        (status = 200, description = "List of built-in preset personas"),
    ),
    tag = "system"
)]
async fn presets_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "presets": &*state.presets }))
}

// ─── Static file serving ────────────────────────────────────────

use include_dir::{include_dir, Dir};

static FRONTEND_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../frontend/dist");

async fn serve_static(uri: axum::http::Uri) -> axum::response::Response {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file
    if let Some(file) = FRONTEND_DIR.get_file(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return build_static_response(axum::http::StatusCode::OK, mime.as_ref(), file.contents());
    }

    // Fallback to index.html for SPA routing
    if let Some(index) = FRONTEND_DIR.get_file("index.html") {
        return build_static_response(
            axum::http::StatusCode::OK,
            "text/html; charset=utf-8",
            index.contents(),
        );
    }

    build_static_response(
        axum::http::StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        b"Frontend not built",
    )
}

/// Build a static file response with a content-type header.
/// Falls back to a headerless response if the content-type is invalid (should never happen
/// with mime_guess output, but avoids panicking on edge cases).
fn build_static_response(
    status: axum::http::StatusCode,
    content_type: &str,
    body: &[u8],
) -> axum::response::Response {
    match axum::response::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body.to_vec()))
    {
        Ok(resp) => resp,
        Err(_) => axum::response::Response::builder()
            .status(status)
            .body(axum::body::Body::from(body.to_vec()))
            .unwrap_or_else(|_| {
                axum::response::Response::new(axum::body::Body::from(body.to_vec()))
            }),
    }
}

// ─── OpenAPI ────────────────────────────────────────────────────

#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "Agent Teams API",
        version = "1.4.0",
        description = "Multi-agent orchestration system with streaming chat, tool discovery, and preset personas.\n\n\
            ## Streaming\n\
            `POST /chat` returns an SSE stream. Use `stream_mode: \"simple\"` for text-only output (DeltaEvent + DoneEvent). \
            Full mode emits all event types for the internal frontend.\n\n\
            ## Authentication\n\
            When `api_keys` is configured, all requests require `Authorization: Bearer <key>` or `x-api-key: <key>` header.\n\n\
            ## Rate Limiting\n\
            Per-IP rate limiting is applied. Exceeding the limit returns HTTP 429."
    ),
    paths(
        chat_handler, health_handler, tools_handler, presets_handler,
        crate::sessions::get_session, crate::sessions::set_session, crate::sessions::delete_session,
    ),
    components(schemas(
        ChatRequest, PresetDef,
        DeltaEvent, DoneEvent, ErrorEvent,
        SubAgentResultsEvent, ToolStatusSseEvent, AgentProgressSseEvent,
        agent_core::provider::TokenUsage,
        agent_core::provider::SubAgentResultSummary,
        agent_core::provider::AgentProgress,
        agent_core::tool::ToolStatusEvent,
        crate::sessions::SessionInfo, crate::sessions::SetInstructionsRequest,
    )),
    tags(
        (name = "chat", description = "Streaming chat endpoints"),
        (name = "sessions", description = "Session instruction management"),
        (name = "system", description = "System status and configuration"),
    )
)]
pub struct ApiDoc;

// ─── Router ──────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    use tower_http::limit::RequestBodyLimitLayer;
    use tower_http::compression::CompressionLayer;

    let needs_auth = !state.api_keys.is_empty();

    // Versioned API routes (/v1/*) — the canonical interface for external consumers.
    // Legacy unversioned routes are kept for backward compatibility.
    let api_routes = Router::new()
        .route("/chat", post(chat_handler))
        .route("/health", get(health_handler))
        .route("/tools", get(tools_handler))
        .route("/presets", get(presets_handler))
        .route("/sessions/{session_id}", get(crate::sessions::get_session))
        .route("/sessions/{session_id}", axum::routing::put(crate::sessions::set_session))
        .route("/sessions/{session_id}", axum::routing::delete(crate::sessions::delete_session));

    let swagger = utoipa_swagger_ui::SwaggerUi::new("/v1/swagger-ui")
        .url("/v1/openapi.json", ApiDoc::openapi());

    let router = Router::new()
        .merge(api_routes.clone())
        .nest("/v1", api_routes)
        .merge(swagger)
        .fallback(serve_static)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(1024 * 1024));

    let router = if needs_auth {
        router
            .route_layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware))
            .with_state(state.clone())
    } else {
        router.with_state(state.clone())
    };

    // Apply per-IP rate limiting if configured
    if let Some(ref limiter) = state.rate_limiter {
        let limiter = limiter.clone();
        limiter.start_cleanup_task();
        router.layer(axum::middleware::from_fn(move |req, next| {
            let limiter = limiter.clone();
            async move { crate::rate_limit::rate_limit_middleware(limiter, req, next).await }
        }))
    } else {
        router
    }
}
