use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::effect::AgentEffect;

/// Message sent to an Agent for processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub content: String,
    pub message_type: String,
    pub session_id: Option<String>,
    pub entity: Option<Value>,
    pub domain_state: Option<Value>,
    pub memory: Option<Value>,
    pub recent_history: Vec<Value>,
    pub custom: Option<Value>,
    /// Hint from frontend: force a specific tool to be used (e.g. from ToolSelector)
    pub force_tool: Option<String>,
}

impl AgentMessage {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content: content.into(),
            message_type: "user_input".to_string(),
            session_id: None,
            entity: None,
            domain_state: None,
            memory: None,
            recent_history: Vec::new(),
            custom: None,
            force_tool: None,
        }
    }

    pub fn with_type(mut self, msg_type: impl Into<String>) -> Self {
        self.message_type = msg_type.into();
        self
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_force_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.force_tool = Some(tool_name.into());
        self
    }
}

/// Agent execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Success,
    Error(String),
    Timeout,
    Skipped,
}

/// Chunk for streaming responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
    pub session_id: Option<String>,
    pub effects: Vec<AgentEffect>,
}

impl StreamChunk {
    pub fn delta(delta: impl Into<String>) -> Self {
        Self {
            delta: delta.into(),
            done: false,
            session_id: None,
            effects: Vec::new(),
        }
    }

    pub fn done() -> Self {
        Self {
            delta: String::new(),
            done: true,
            session_id: None,
            effects: Vec::new(),
        }
    }
}

/// Tagged chunk for multi-agent streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggedStreamChunk {
    pub agent_id: String,
    pub chunk: StreamChunk,
}

/// Task type for routing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TaskType {
    KnowledgeQuery,
    SentimentAnalysis,
    ToolRequest,
    EscalationCheck,
    ConversationSummary,
    General,
    Custom(String),
}

impl TaskType {
    pub fn from_message_type(msg_type: &str) -> Self {
        match msg_type {
            "knowledge_query" => Self::KnowledgeQuery,
            "sentiment_analysis" => Self::SentimentAnalysis,
            "tool_request" => Self::ToolRequest,
            "escalation_check" => Self::EscalationCheck,
            "conversation_summary" => Self::ConversationSummary,
            "user_input" | "general" => Self::General,
            other => Self::Custom(other.to_string()),
        }
    }
}
