use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::effect::AgentEffect;
use crate::error::Result;
use crate::memory_store::MemoryStore;
use crate::message::AgentStatus;

/// Lightweight tool info for intent detection (passed to all agents)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters_hint: String,
}

/// Simplified agent input — only what agents actually need.
#[derive(Debug, Clone)]
pub struct AgentInput {
    /// Pre-built system prompt (assembled from context providers)
    pub system_prompt: String,
    /// User content / query
    pub content: String,
    /// Recent conversation history (only populated for agents that need it)
    pub recent_history: Vec<serde_json::Value>,
    /// Effects from prior agents in this turn (only populated for agents that need it)
    pub prior_effects: std::sync::Arc<Vec<AgentEffect>>,
    /// Session ID for memory scoping
    pub session_id: Option<String>,
    /// User ID for memory scoping
    pub user_id: Option<String>,
    /// Available tools registry info (tool names + descriptions) for intent detection
    pub available_tools: Vec<ToolInfo>,
    /// Full agent context — injected from the coordinator pipeline.
    /// Carries working memory, system instructions, domain state, conversation history,
    /// and cache data. Tools can access this directly without separate cache lookups.
    pub agent_context: Option<Arc<crate::context::AgentContext>>,
}

/// Simplified agent output — no metadata/plan_ref noise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    pub content: String,
    pub thinking: Option<String>,
    pub effects: Vec<AgentEffect>,
    pub quality: f32,
    pub status: AgentStatus,
    /// Structured metadata for cross-agent communication.
    /// SubAgents can attach key findings, tool results, confidence breakdowns, etc.
    /// MainAgent synthesis uses this for richer context without redundant LLM calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Annotations from the LLM response (e.g., web search citations).
    /// Propagated to the frontend for displaying source links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<serde_json::Value>,
}

impl Default for AgentOutput {
    fn default() -> Self {
        Self {
            content: String::new(),
            thinking: None,
            effects: Vec::new(),
            quality: 1.0,
            status: AgentStatus::Success,
            metadata: None,
            annotations: Vec::new(),
        }
    }
}

impl AgentOutput {
    pub fn error(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            content: msg.clone(),
            quality: 0.0,
            status: AgentStatus::Error(msg),
            ..Default::default()
        }
    }

    /// Attach structured metadata to this output
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Agent capability declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub message_types: Vec<String>,
    pub requires_llm: bool,
    pub supports_streaming: bool,
    pub priority: i32,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 0,
        }
    }
}

impl AgentCapabilities {
    pub fn matches_message_type(&self, msg_type: &str) -> bool {
        self.message_types.iter().any(|t| t == "*" || t == msg_type)
    }
}

/// Black-box agent trait — simplified interface.
/// Agents receive only what they need and produce only what matters.
#[async_trait]
pub trait BoxedAgent: Send + Sync {
    /// Unique agent identifier
    fn id(&self) -> &str;

    /// What this agent can handle
    fn capabilities(&self) -> AgentCapabilities;

    /// Process a task and produce an output
    async fn run(&self, input: AgentInput) -> AgentOutput;

    /// Check if the agent is healthy
    async fn health_check(&self) -> bool {
        true
    }

    /// Handle a bus envelope (for inter-agent communication)
    async fn handle_envelope(
        &self,
        _input: AgentInput,
        _envelope_type: &str,
    ) -> Option<AgentOutput> {
        None
    }

    /// Support downcasting to concrete types (for MemoryAwareAgent detection)
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get reference as MemoryAwareAgent if this agent implements it.
    /// Returns None by default; override in agents that implement MemoryAwareAgent.
    fn as_memory_aware(&self) -> Option<&dyn MemoryAwareAgent> {
        None
    }
}

/// Memory-aware Agent extension trait
/// Agents implementing this trait can bidirectionally sync with the memory system
/// via their own local AgentMemoryCache
#[async_trait]
pub trait MemoryAwareAgent: BoxedAgent {
    /// Get a reference to the agent's local memory cache (always available)
    fn memory_cache(&self) -> &crate::agent_memory_cache::AgentMemoryCache;

    /// Sync agent output to memory system after execution
    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> Result<()>;

    /// Load context from memory system and inject into AgentInput
    async fn load_memory_context(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        query: &str,
    ) -> Result<String> {
        let _ = (store, session_id, query);
        Ok(String::new())
    }

    /// Flush all dirty entries from local cache to global store
    async fn flush_cache(&self) -> Result<usize> {
        self.memory_cache().flush_all().await
    }
}
