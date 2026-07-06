use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::AgentMessage;
use crate::pipeline::StageMode;

/// Execution plan produced by MainAgent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub stages: Vec<PlanStage>,
    pub strategy: String,
    pub estimated_duration_ms: u64,
    pub confidence: f32,
    /// New: PlanNode-based nodes for unified Agent+Tool orchestration
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<PlanNode>,
    /// Detected tool intent from MainAgent — allows bypassing task_planner's
    /// LLM decision when the tool to call is already known with high confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_intent: Option<ToolIntent>,
}

/// Tool intent metadata carried from MainAgent to task_planner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIntent {
    /// Tool name (e.g. "xxt", "file", "media")
    pub tool_name: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Suggested subcommand (e.g. "crawl" for xxt)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
}

impl Default for ExecutionPlan {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            strategy: String::new(),
            estimated_duration_ms: 0,
            confidence: 0.0,
            nodes: Vec::new(),
            tool_intent: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStage {
    pub name: String,
    pub sub_agent_ids: Vec<String>,
    pub mode: StageMode,
    pub required: bool,
    pub timeout_ms: Option<u64>,
    pub message_override: Option<AgentMessage>,
}

/// Plan node for unified Agent+Tool orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlanNode {
    /// Call a SubAgent
    Agent {
        agent_id: String,
        input_transform: Option<String>,
    },
    /// Call a Tool directly
    Tool {
        tool_name: String,
        arguments_source: ArgumentSource,
    },
    /// Conditional branch
    Condition {
        expression: String,
        then_branch: Vec<PlanNode>,
        else_branch: Vec<PlanNode>,
    },
    /// Parallel execution group
    Parallel(Vec<PlanNode>),
    /// Sequential execution group
    Sequential(Vec<PlanNode>),
}

/// Argument source for PlanNode::Tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArgumentSource {
    /// Extract field from original user message
    FromUserMessage { field: String },
    /// Extract from upstream node result (node_index, json_path)
    FromUpstream {
        node_index: usize,
        json_path: String,
    },
    /// Extract from execution context
    FromContext { key: String },
    /// Static value
    Static(Value),
}

/// Execution state for PlanNode-based execution
#[derive(Debug, Clone, Default)]
pub struct PlanExecutionState {
    /// Results of each executed node
    pub node_results: Vec<Value>,
    /// Current node index being executed
    pub current_index: usize,
}
