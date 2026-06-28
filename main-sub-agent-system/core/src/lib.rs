pub mod agent_memory_cache;
pub mod boxed_agent;
pub mod companion;
pub mod bus;
pub mod config;
pub mod context;
pub mod context_provider;
pub mod dedup_engine;
pub mod domain;
pub mod effect;
pub mod error;
pub mod event;
pub mod hash;
pub mod hook;
pub mod memory;
pub mod memory_event_bus;
pub mod memory_intent;
pub mod memory_lifecycle;
pub mod memory_reranker;
pub mod memory_store;
pub mod message;
pub mod pipeline;
pub mod plan;
pub mod processor;
pub mod provider;
pub mod registry;
pub mod routing;
pub mod state;
pub mod sub_agent;
pub mod tag_extractor;
pub mod tool;
pub mod tool_cache;
pub mod unified_cache_manager;
pub mod unified_memory_bus;

// Re-export key types
pub use agent_memory_cache::{AgentMemoryCache, CacheMode, CacheStats, ExecutionPolicy};
pub use boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent, ToolInfo};
pub use companion::{CompanionDelta, CompanionState};
pub use bus::{AgentBus, AgentEnvelope, AgentTarget, BusPayload};
pub use config::{AppConfig, CacheModeConfig, MainAgentConfig, RuntimeConfig, UnifiedCacheConfig};
pub use context::AgentContext;
pub use context::{
    DomainStateContextProvider, EntityContextProvider, MemoryContextProvider,
    MultiTurnContextProvider, SystemInstructionContextProvider,
};
pub use context_provider::{ContextProvider, PromptFragment, PromptPriority};
pub use domain::DomainModule;
pub use effect::AgentEffect;
pub use error::{AgentTeamsError, ApiResponse, Result};
pub use event::{EventBus, SystemEvent};
pub use hash::cosine_similarity;
pub use hook::{Hook, HookContext, HookData, HookPoint, HookPriority, HookRegistry, HookResult};
pub use memory::{
    CompressionStrategy, MemoryConfig, MemoryEntry, MemoryEvent, MemoryEventHandler, MemoryKind,
    MemoryQuery, MemoryRelation, MemoryRelationType, MemoryRetrievalResult,
};
pub use memory_event_bus::{MemoryChangeEvent, MemoryEventBus};
pub use memory_store::{EmbeddingError, EmbeddingProvider, MemoryStore};
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
pub use unified_cache_manager::{CacheManagerConfig, CacheError, UnifiedCacheManager};
pub use unified_memory_bus::{CacheMetrics, SharedMemoryCache, UnifiedMemoryBus};
