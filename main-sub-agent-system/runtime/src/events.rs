use serde::Serialize;

use agent_teams_core::provider::{AgentProgress, SubAgentResultSummary, TokenUsage};

// ─── SSE event types (typed, documented) ────────────────────────
//
// These structs define the wire format for the `/chat` SSE stream.
// External consumers should rely on `stream_mode: "simple"` which
// only emits `Delta` and `Done` events.

/// Text chunk delivered during streaming.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct DeltaEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub delta: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

/// Stream completion event.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct DoneEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub delta: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

/// Error event — terminates the stream.
/// Note: `delta` mirrors `error` for frontend compatibility (StreamChunk reads `chunk.delta`).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ErrorEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub error: String,
    /// Same as `error` — kept for frontend StreamChunk compatibility.
    pub delta: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
}

/// Sub-agent execution results (full mode only).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct SubAgentResultsEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub sub_agent_results: Vec<SubAgentResultSummary>,
    pub done: bool,
}

/// Tool execution status (full mode only).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ToolStatusSseEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub tool_status: agent_teams_core::tool::ToolStatusEvent,
    pub done: bool,
}

/// Agent progress update (full mode only).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AgentProgressSseEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub agent_progress: AgentProgress,
    pub done: bool,
}

// ─── Constructors ───────────────────────────────────────────────

impl DeltaEvent {
    pub fn new(
        delta: impl Into<String>,
        thinking_delta: Option<String>,
        usage: Option<TokenUsage>,
    ) -> Self {
        Self {
            event_type: "delta",
            delta: delta.into(),
            done: false,
            thinking_delta,
            usage,
        }
    }
}

impl DoneEvent {
    pub fn new(delta: impl Into<String>, usage: Option<TokenUsage>) -> Self {
        Self {
            event_type: "done",
            delta: delta.into(),
            done: true,
            usage,
        }
    }
}

impl ErrorEvent {
    pub fn new(error: impl Into<String>) -> Self {
        let error = error.into();
        Self {
            event_type: "error",
            delta: error.clone(),
            error,
            done: true,
            code: None,
        }
    }

    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }
}

impl SubAgentResultsEvent {
    pub fn new(results: Vec<SubAgentResultSummary>) -> Self {
        Self {
            event_type: "sub_agent_results",
            sub_agent_results: results,
            done: false,
        }
    }
}

impl ToolStatusSseEvent {
    pub fn new(status: agent_teams_core::tool::ToolStatusEvent) -> Self {
        Self {
            event_type: "tool_status",
            tool_status: status,
            done: false,
        }
    }
}

impl AgentProgressSseEvent {
    pub fn new(progress: AgentProgress) -> Self {
        Self {
            event_type: "agent_progress",
            agent_progress: progress,
            done: false,
        }
    }
}
