//! Tool execution engine and agent tool loop
//!
//! Re-exports from `agent-toolkit` and `agent-core` for backward compatibility.

// Re-export from toolkit (the canonical location for AgentToolLoop)
pub use agent_toolkit::AgentToolLoop;
// Re-export from core for backward compatibility
pub use agent_core::tool_engine::{
    ToolExecutionEngine, ToolMetrics, PerToolMetrics, ToolHealthStatus, HealthStatus,
};
