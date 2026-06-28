use std::sync::Arc;

use agent_teams_core::config::AppConfig;

use agent_teams_provider::anthropic::AnthropicProvider;
use agent_teams_provider::circuit_breaker::CircuitBreakerProvider;
use agent_teams_provider::retry::RetryProvider;

use agent_teams_runtime::http::{build_router, start_session_cleanup_task, AppState, Metrics, PresetDef};
use agent_teams_runtime::telemetry;
use agent_teams_runtime::RuntimeBuilder;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        tracing::error!("Server fatal: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file if present
    dotenvy::dotenv_override().ok();

    // Initialize tracing with OpenTelemetry
    if let Err(e) = telemetry::init_telemetry("agent-teams-server") {
        eprintln!("Failed to initialize telemetry: {}", e);
        // Fallback to basic tracing
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    tracing::info!("Starting Agent Teams server...");

    // Load config once, reuse for both typed and raw access
    let config_str = tokio::fs::read_to_string("config.json").await.unwrap_or_else(|_| {
        tracing::warn!("config.json not found, using defaults");
        "{}".to_string()
    });

    // Resolve environment variable placeholders like ${VAR_NAME}
    let config_str = agent_teams_runtime::resolve_env_vars(&config_str);

    let config: AppConfig = serde_json::from_str(&config_str).map_err(|e| {
        format!("Failed to parse config.json: {}. Check the file for syntax errors.", e)
    })?;

    let port = config.runtime.port;
    let host = config.runtime.host.clone();
    let cors_enabled = config.runtime.cors_enabled.unwrap_or(false);
    let cors_origins = config.runtime.cors_allowed_origins.clone();
    let max_concurrent = config.runtime.max_concurrent_requests.unwrap_or(100);

    // Build runtime
    let mut runtime_builder = RuntimeBuilder::new(config.clone());

    // Register providers using strong typed config
    let providers = &config.providers;

    // Anthropic
    let mut app_provider: Option<Arc<dyn agent_teams_core::provider::LlmProvider>> = None;
    if let Some(anthropic) = &providers.anthropic {
        if let Some(api_key) = &anthropic.api_key {
            if !api_key.is_empty() {
                let base: Box<dyn agent_teams_core::provider::LlmProvider> = Box::new(
                    AnthropicProvider::new(&anthropic.base_url, api_key, &anthropic.default_model),
                );
                let with_retry: Box<dyn agent_teams_core::provider::LlmProvider> = Box::new(
                    RetryProvider::new(base, anthropic.max_retries, anthropic.retry_base_delay_ms),
                );
                let provider: Arc<dyn agent_teams_core::provider::LlmProvider> = Arc::new(CircuitBreakerProvider::new(
                    with_retry,
                    anthropic.circuit_breaker_threshold,
                    anthropic.circuit_breaker_open_duration_secs,
                ));
                app_provider = Some(provider.clone());
                runtime_builder = runtime_builder.with_provider(provider).await;
                tracing::info!(
                    "Registered Anthropic provider (retries={}, cb_threshold={})",
                    anthropic.max_retries,
                    anthropic.circuit_breaker_threshold
                );
            }
        }
    }

    // Register context providers
    runtime_builder = runtime_builder
        .with_context_provider(Arc::new(
            agent_teams_core::context::MultiTurnContextProvider,
        ))
        .with_context_provider(Arc::new(
            agent_teams_core::context::DomainStateContextProvider,
        ))
        .with_context_provider(Arc::new(agent_teams_core::context::EntityContextProvider))
        .with_context_provider(Arc::new(
            agent_teams_core::context::SystemInstructionContextProvider,
        ));

    let (coordinator, registry, tool_registry) = runtime_builder.build().await
        .map_err(|e| format!("Failed to build runtime: {}", e))?;

    let tool_engine = Arc::new(
        agent_teams_agents::tool_engine::ToolExecutionEngine::new(tool_registry.clone()),
    );

    let provider = app_provider.ok_or("No LLM provider configured. Check config.json: ensure at least one provider has a valid api_key.")?;

    // Extract API keys from security config
    let api_keys: Vec<String> = config
        .security
        .as_ref()
        .map(|s| s.api_keys.clone())
        .unwrap_or_default();

    let default_model = config.providers.default_model();

    // Load built-in presets from raw config JSON
    let presets: Vec<PresetDef> = serde_json::from_str::<serde_json::Value>(&config_str)
        .ok()
        .and_then(|v| v.get("presets")?.get("builtin")?.clone().into())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| {
            tracing::warn!("No presets found in config.json");
            Vec::new()
        });
    tracing::info!("Loaded {} built-in presets", presets.len());

    let pipeline_timeout_secs = config
        .timeouts
        .as_ref()
        .map(|t| t.pipeline_timeout_ms / 1000)
        .unwrap_or(300);

    // Start periodic session instruction cleanup
    start_session_cleanup_task();

    // Initialize per-IP rate limiter if configured
    let rate_limiter = match config.runtime.rate_limit_max_requests {
        Some(max) if max > 0 => {
            let window = config.runtime.rate_limit_window_secs.unwrap_or(60);
            let limiter = agent_teams_runtime::rate_limit::RateLimiter::new(max, window);
            tracing::info!("Rate limiting: {} requests per {}s per IP", max, window);
            Some(limiter)
        }
        _ => None,
    };

    let state = AppState {
        coordinator: Arc::new(coordinator),
        registry,
        metrics: Arc::new(Metrics::new()),
        cache_metrics: Arc::new(agent_teams_coordinator::cache_metrics::CacheMetrics::new()),
        tool_registry,
        tool_engine,
        provider,
        api_keys: Arc::new(api_keys),
        default_model,
        presets: Arc::new(presets),
        pipeline_timeout_secs,
        rate_limiter,
        companion_states: Arc::new(dashmap::DashMap::new()),
    };

    let mut app = build_router(state);

    // Add CORS middleware if enabled
    if cors_enabled {
        use tower_http::cors::{AllowHeaders, AllowMethods, CorsLayer};
        let mut cors = CorsLayer::new();

        if let Some(ref origins) = cors_origins {
            if origins.is_empty() {
                tracing::warn!("CORS enabled but cors_allowed_origins is empty; denying all cross-origin requests");
            } else {
                let allowed: Vec<axum::http::HeaderValue> =
                    origins.iter().filter_map(|o| o.parse().ok()).collect();
                cors = cors.allow_origin(allowed);
                tracing::info!("CORS enabled with {} allowed origins", origins.len());
            }
        } else {
            // No origins configured — default to deny all
            tracing::warn!(
                "CORS enabled but cors_allowed_origins not set; denying all cross-origin requests"
            );
        }

        cors = cors.allow_methods(AllowMethods::list([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ]));
        cors = cors.allow_headers(AllowHeaders::list([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]));
        app = app.layer(cors);
    }

    // Add concurrency limit
    use tower::limit::ConcurrencyLimitLayer;
    app = app.layer(ConcurrencyLimitLayer::new(max_concurrent));
    tracing::info!("Concurrency limit: {}", max_concurrent);


    let addr = format!("{}:{}", host, port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    // Graceful shutdown
    let server = axum::serve(listener, app);

    // Handle shutdown signal
    let shutdown_signal = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for Ctrl+C signal: {}", e);
            return;
        }
        tracing::info!("Shutdown signal received");
    };

    // Run server with graceful shutdown
    if let Err(e) = server.with_graceful_shutdown(shutdown_signal).await {
        tracing::error!("Server error: {}", e);
    }

    // Shutdown telemetry
    telemetry::shutdown_telemetry();
    tracing::info!("Server shutdown complete");
    Ok(())
}
