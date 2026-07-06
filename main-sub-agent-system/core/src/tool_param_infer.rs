//! Parameter inference trait for tool call enrichment
//!
//! Defines the `ParamInferrer` trait that allows pluggable parameter inference
//! for tool calls. Implementations can use LLM-based inference, regex extraction,
//! rule-based defaults, or any combination.

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ChatMessage;
use crate::tool::{Tool, ToolCall, ToolResult};

/// Parameter inference context extracted from conversation history
#[derive(Debug, Clone, Default)]
pub struct ConversationContext {
    /// Extracted entities from conversation (e.g., city names, file paths, URLs)
    pub entities: std::collections::HashMap<String, Vec<String>>,
    /// Recent tool call results for reference
    pub recent_results: Vec<(String, Value)>,
    /// User preferences mentioned in conversation
    pub preferences: std::collections::HashMap<String, String>,
    /// Current topic/task context
    pub topic: Option<String>,
    /// Conversation history for pattern matching
    pub conversation_history: Vec<String>,
}

/// Trait for pluggable parameter inference.
///
/// Implementations enrich incomplete tool call arguments by extracting
/// context from conversation history, applying rules, or using LLM inference.
#[async_trait]
pub trait ParamInferrer: Send + Sync {
    /// Extract conversation context from message history
    fn extract_context(&self, messages: &[ChatMessage]) -> ConversationContext;

    /// Infer missing parameters for a tool call using conversation context and tool history.
    /// Returns enriched arguments with missing fields filled in.
    async fn infer_parameters_with_history(
        &self,
        tool: &Tool,
        partial_args: &Value,
        context: &ConversationContext,
        messages: &[ChatMessage],
        tool_history: &[(ToolCall, ToolResult)],
    ) -> Value;
}
