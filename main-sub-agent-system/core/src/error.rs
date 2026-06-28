use thiserror::Error;

/// Unified API response wrapper
#[derive(Debug, serde::Serialize)]
pub struct ApiResponse<T: serde::Serialize> {
    pub code: u32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: serde::Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "ok".to_string(),
            data: Some(data),
        }
    }

    pub fn error(code: u32, message: impl Into<String>) -> ApiResponse<()> {
        ApiResponse {
            code,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Error, Debug)]
pub enum AgentTeamsError {
    // Provider
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Provider auth error: {0}")]
    ProviderAuth(String),
    #[error("Provider rate limited, retry_after: {retry_after:?}")]
    ProviderRateLimit { retry_after: Option<u64> },

    // Agent
    #[error("Agent {agent_id} error: {message}")]
    Agent { agent_id: String, message: String },
    #[error("Agent {agent_id} timed out after {timeout_ms}ms")]
    AgentTimeout { agent_id: String, timeout_ms: u64 },
    #[error("Agent {agent_id} panicked")]
    AgentPanic { agent_id: String },

    // Bus
    #[error("Bus timeout after {timeout_ms}ms")]
    BusTimeout { timeout_ms: u64 },
    #[error("No subscriber for: {0}")]
    BusNoSubscriber(String),
    #[error("Bus channel closed")]
    BusChannelClosed,

    // Plan
    #[error("Plan generation failed: {0}")]
    PlanGenerationFailed(String),
    #[error("Plan validation failed: {0}")]
    PlanValidationFailed(String),
    #[error("Partial execution failure: succeeded={succeeded:?}, failed={failed:?}")]
    PlanExecutionPartial {
        succeeded: Vec<String>,
        failed: Vec<(String, String)>,
    },

    // Tool
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Tool {tool_name} timed out after {timeout_ms}ms")]
    ToolTimeout { tool_name: String, timeout_ms: u64 },
    #[error("Tool {tool_name} execution failed: {message}")]
    ToolExecutionFailed { tool_name: String, message: String },

    // Hook
    #[error("Hook {hook_name} halted: {reason}")]
    HookHalt { hook_name: String, reason: String },

    // State
    #[error("State store error: {0}")]
    StateStoreError(String),
    #[error("State version conflict: expected {expected}, got {actual}")]
    StateVersionConflict { expected: u64, actual: u64 },

    // Config
    #[error("Config error: {0}")]
    ConfigError(String),
    #[error("Config validation error: {0}")]
    ConfigValidation(String),

    // Generic
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, AgentTeamsError>;

impl From<crate::provider::ProviderError> for AgentTeamsError {
    fn from(e: crate::provider::ProviderError) -> Self {
        match e {
            crate::provider::ProviderError::Auth(msg) => AgentTeamsError::ProviderAuth(msg),
            crate::provider::ProviderError::RateLimited { retry_after } => {
                AgentTeamsError::ProviderRateLimit { retry_after }
            }
            crate::provider::ProviderError::Unavailable(msg) => {
                AgentTeamsError::Provider(format!("Unavailable: {}", msg))
            }
            crate::provider::ProviderError::InvalidResponse(msg) => {
                AgentTeamsError::Provider(format!("InvalidResponse: {}", msg))
            }
            crate::provider::ProviderError::TooLarge => {
                AgentTeamsError::Provider("Request too large".to_string())
            }
            crate::provider::ProviderError::Other(msg) => {
                AgentTeamsError::Provider(format!("Other: {}", msg))
            }
        }
    }
}
