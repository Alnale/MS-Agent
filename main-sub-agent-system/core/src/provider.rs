use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool::{Tool, ToolCall, ToolCallDelta};

// Provider errors are handled via ProviderError, not AgentTeamsError

/// LLM provider trait — unified interface for all LLM backends
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn models(&self) -> Vec<String>;

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, ProviderError>;

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<
        Box<
            dyn futures::Stream<Item = std::result::Result<CompletionChunk, ProviderError>>
                + Unpin
                + Send,
        >,
        ProviderError,
    >;

    fn supports_structured(&self) -> bool {
        false
    }

    async fn health_check(&self) -> std::result::Result<(), ProviderError> {
        Ok(())
    }
}

/// Tool choice mode for LLM requests
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required { name: String },
}

/// Response format for structured output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    /// Format type: "text", "json_object", "json_schema"
    pub format_type: String,
    /// Name for json_schema format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// JSON schema for json_schema format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub system: Option<String>,
    pub stream: bool,
    /// Plain string input for Responses API (alternative to messages array).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// Structured tool definitions (replaces raw Value)
    pub tools: Option<Vec<Tool>>,
    /// Force specific tool usage (auto / none / named)
    pub tool_choice: Option<ToolChoice>,
    pub metadata: Option<Value>,
    pub thinking: Option<ThinkingConfig>,
    /// Response format for structured output (e.g., json_object).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: u32,
    #[serde(default)]
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub cache_control: Option<Value>,
    /// Image URLs for multimodal input (user messages with images).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    /// Tool call ID for tool result messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool calls made by the assistant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    /// Create a simple message without tool fields
    pub fn simple(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            cache_control: None,
            images: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub thinking: Option<String>,
    pub model: String,
    pub usage: TokenUsage,
    pub stop_reason: Option<String>,
    /// Tool calls requested by the LLM
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// Annotations from the LLM response (e.g., web search citations)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cached_tokens: u32,
    /// Tokens consumed by reasoning/thinking process.
    #[serde(default)]
    pub reasoning_tokens: u32,
    /// Total tokens (input + output).
    #[serde(default)]
    pub total_tokens: u32,
}

/// Summary of a SubAgent's result for frontend display
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SubAgentResultSummary {
    pub agent_id: String,
    pub content_summary: String,
    pub thinking: Option<String>,
    pub quality: f32,
    /// Optional sticker filename recommended by sentiment agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticker: Option<String>,
}

/// Real-time pipeline progress event for frontend display
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum AgentProgress {
    /// Pipeline stage started
    StageStarted { stage_name: String, detail: String },
    /// A SubAgent started executing
    AgentStarted { agent_id: String, agent_type: String },
    /// A SubAgent completed
    AgentCompleted { agent_id: String, success: bool, duration_ms: u64 },
    /// Synthesis phase started
    SynthesisStarted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletionChunk {
    pub delta: String,
    pub thinking_delta: Option<String>,
    pub done: bool,
    pub usage: Option<TokenUsage>,
    /// Incremental tool call (streaming mode may send in chunks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_delta: Option<ToolCallDelta>,
    /// Tool execution status event (for real-time tool progress feedback)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_status: Option<crate::tool::ToolStatusEvent>,
    /// SubAgent result summaries (emitted once before synthesis)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_agent_results: Option<Vec<SubAgentResultSummary>>,
    /// Real-time pipeline progress (agent execution status)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_progress: Option<AgentProgress>,
    /// Companion emotional state (emitted when companion mode is active)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub companion_state: Option<crate::companion::CompanionState>,
    /// Annotations from the LLM response (e.g., web search citations)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredResponse {
    pub data: Value,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("Rate limited, retry_after: {retry_after:?}")]
    RateLimited { retry_after: Option<u64> },
    #[error("Request too large")]
    TooLarge,
    #[error("Provider unavailable: {0}")]
    Unavailable(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("{0}")]
    Other(String),
}
