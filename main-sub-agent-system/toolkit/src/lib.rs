//! Agent Toolkit — reusable tool execution infrastructure for LLM-powered agents
//!
//! This crate provides the core building blocks for tool-calling agents:
//!
//! - [`ToolExecutionEngine`] — resilient tool execution with circuit breaker, retry, caching
//! - [`AgentToolLoop`] — ReAct pattern loop (LLM reasons → calls tools → repeats)
//!
//! # Quick start
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use agent_core::tool::{ToolBuilder, ToolExecutor, ToolCall, ToolResult, ToolExecutionContext, tool_success};
//! use agent_core::tool_engine::ToolExecutionEngine;
//! use agent_core::provider::LlmProvider;
//! use agent_toolkit::AgentToolLoop;
//!
//! // 1. Define a tool
//! struct MyTool;
//! #[async_trait::async_trait]
//! impl ToolExecutor for MyTool {
//!     fn executor_id(&self) -> &str { "my_tool" }
//!     fn list_tools(&self) -> Vec<agent_core::tool::Tool> {
//!         vec![ToolBuilder::new("my_tool").description("Does something").executor("my_tool").build()]
//!     }
//!     async fn execute(&self, call: &ToolCall, _ctx: &ToolExecutionContext) -> agent_core::error::Result<ToolResult> {
//!         Ok(tool_success(call, serde_json::json!({"result": "ok"}), 0))
//!     }
//! }
//!
//! // 2. Register tools and run
//! // let registry = agent_core::tool::UnifiedToolRegistry::new();
//! // registry.register_executor(Arc::new(MyTool));
//! // let engine = Arc::new(ToolExecutionEngine::new(Arc::new(registry)));
//! // let loop = AgentToolLoop::new(provider, engine);
//! // let (output, history) = loop.run(messages, tools, &ctx).await?;
//! ```

mod agent_loop;

pub use agent_loop::AgentToolLoop;

// Re-export core types for convenience
pub use agent_core::tool_engine::{
    ToolExecutionEngine, ToolMetrics, PerToolMetrics, ToolHealthStatus, HealthStatus,
};
pub use agent_core::tool_param_infer::{ConversationContext, ParamInferrer};
pub use agent_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolResult, ToolExecutor, ToolExecutionContext,
    UnifiedToolRegistry, ToolStatusEvent, ResourcePool, tool_success, tool_error,
};
pub use agent_core::provider::{
    LlmProvider, ChatMessage, CompletionRequest, CompletionResponse, CompletionChunk,
    ToolChoice, TokenUsage,
};
pub use agent_core::boxed_agent::AgentOutput;
pub use agent_core::error::{AgentTeamsError, Result};
