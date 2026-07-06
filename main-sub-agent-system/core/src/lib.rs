//! Agent Core — foundational types and traits for LLM-powered agent systems
//!
//! This crate provides the building blocks for agent frameworks:
//! - **Tool system**: `Tool`, `ToolExecutor`, `UnifiedToolRegistry`, `ToolExecutionEngine`
//! - **LLM abstraction**: `LlmProvider`, `CompletionRequest/Response`, `ChatMessage`
//! - **Agent abstractions**: `AgentOutput`, `AgentInput`, `BoxedAgent`
//! - **Orchestration**: `ExecutionPlan`, `AgentRegistry`, `RoutingTable`
//!
//! For a batteries-included experience, use the `agent-toolkit` crate which
//! re-exports the most common types and adds the `AgentToolLoop`.

// ─── SDK-critical modules (public API for external consumers) ─────────
pub mod boxed_agent;
pub mod context;
pub mod context_provider;
pub mod effect;
pub mod error;
pub mod event;
pub mod hook;
pub mod message;
pub mod pipeline;
pub mod plan;
pub mod processor;
pub mod provider;
pub mod registry;
pub mod routing;
pub mod state;
pub mod sub_agent;
pub mod tool;
pub mod tool_cache;
pub mod tool_engine;
pub mod tool_param_infer;
pub mod config;

// ─── Application-specific modules (used internally by agents/coordinator) ─
// These are public for internal use but not part of the external SDK surface.
// External consumers should use agent-toolkit instead.
pub mod agent_memory_cache;
pub mod bus;
pub mod companion;
pub mod dedup_engine;
pub mod domain;
pub mod hash;
pub mod memory;
pub mod memory_event_bus;
pub mod memory_intent;
pub mod memory_lifecycle;
pub mod memory_reranker;
pub mod memory_store;
pub mod tag_extractor;
pub mod unified_cache_manager;
pub mod unified_memory_bus;

// ─── SDK re-exports (the types external consumers actually need) ─────

// Agent abstractions
pub use boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent, ToolInfo};
pub use context::AgentContext;
pub use context::{
    DomainStateContextProvider, EntityContextProvider, MemoryContextProvider,
    MultiTurnContextProvider, SystemInstructionContextProvider, estimate_tokens,
};
pub use context_provider::{ContextProvider, PromptFragment, PromptPriority};
pub use effect::AgentEffect;
pub use error::{AgentTeamsError, ApiResponse, Result};
pub use event::{EventBus, SystemEvent};
pub use hook::{Hook, HookContext, HookData, HookPoint, HookPriority, HookRegistry, HookResult};
pub use message::{AgentMessage, AgentStatus, StreamChunk, TaggedStreamChunk, TaskType};
pub use pipeline::{PipelineDef, PipelineStage, StageMode};
pub use plan::{ArgumentSource, ExecutionPlan, PlanExecutionState, PlanNode, PlanStage};
pub use processor::{
    detect_injection, extract_tool_calls, sanitize_user_input, strip_system_tags, strip_tool_tags,
    ParsedToolCall,
};
pub use provider::ThinkingConfig;
pub use provider::{
    ChatMessage, CompletionChunk, CompletionRequest, CompletionResponse, LlmProvider, TokenUsage,
};
pub use registry::{AgentRegistry, SharedAgent};
pub use routing::{RouteCondition, RouteTarget, RoutingRule, RoutingTable};
pub use state::{ApplyResult, StateStore};
pub use sub_agent::{SubAgentDescriptor, SubAgentRunner};
pub use tool::{
    ExternalToolSource, McpTransport, OpenApiAuth, PermissionDecision, ResourcePool, RetryPolicy,
    Tool, ToolBuilder, ToolCall, ToolCallDelta, ToolExecutionContext, ToolExecutor, ToolParameters,
    ToolPolicyEngine, ToolResult, ToolStatusEvent, UnifiedToolRegistry, tool_error, tool_success,
};
pub use tool_cache::{ToolCacheConfig, ToolResultCache, ToolCacheStats};
pub use tool_engine::{ToolExecutionEngine, ToolMetrics, PerToolMetrics, ToolHealthStatus, HealthStatus};
pub use tool_param_infer::{ConversationContext, ParamInferrer};
pub use config::{AppConfig, CacheModeConfig, MainAgentConfig, RuntimeConfig, UnifiedCacheConfig};

// ─── Application-specific re-exports (used by agents/coordinator internally) ─
pub use agent_memory_cache::{AgentMemoryCache, CacheMode, CacheStats, ExecutionPolicy};
pub use companion::{CompanionDelta, CompanionState};
pub use bus::{AgentBus, AgentEnvelope, AgentTarget, BusPayload};
pub use domain::DomainModule;
pub use hash::cosine_similarity;
pub use memory::{
    CompressionStrategy, MemoryConfig, MemoryEntry, MemoryEvent, MemoryEventHandler, MemoryKind,
    MemoryQuery, MemoryRelation, MemoryRelationType, MemoryRetrievalResult,
};
pub use memory_event_bus::{MemoryChangeEvent, MemoryEventBus};
pub use memory_store::{EmbeddingError, EmbeddingProvider, MemoryStore};
pub use unified_cache_manager::{CacheManagerConfig, CacheError, UnifiedCacheManager};
pub use unified_memory_bus::{CacheMetrics, SharedMemoryCache, UnifiedMemoryBus};
