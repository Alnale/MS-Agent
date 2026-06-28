use serde::{Deserialize, Serialize};
use serde_json::Value;

/// All possible effects an Agent can produce.
/// Agents never directly modify state — they produce effects,
/// which are aggregated and applied by the coordinator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEffect {
    TextChange {
        field: String,
        value: String,
        agent_id: String,
    },
    NumericChange {
        field: String,
        delta: f64,
        agent_id: String,
    },
    StatusChange {
        field: String,
        old_value: String,
        new_value: String,
        agent_id: String,
    },
    InfoFragment {
        content: String,
        agent_id: String,
        priority: i32,
        hash_key: Option<String>,
        category: Option<String>,
    },
    DialogueLine {
        speaker: String,
        content: String,
        hash_key: Option<String>,
        agent_id: String,
    },
    TaskItem {
        task_id: String,
        description: String,
        status: String,
        agent_id: String,
    },
    MemoryUpdate {
        key: String,
        value: Value,
        agent_id: String,
    },
    EventEmit {
        event_type: String,
        data: Value,
        agent_id: String,
    },
    ToolTrigger {
        tool_name: String,
        input: Value,
        agent_id: String,
    },
    ReviewNote {
        severity: ReviewSeverity,
        message: String,
        agent_id: String,
        target_agent_id: Option<String>,
    },
    ConfigChange {
        key: String,
        value: Value,
        agent_id: String,
    },
    RoutingSuggestion {
        suggested_agent_id: String,
        reason: String,
        confidence: f32,
        agent_id: String,
    },
    Custom {
        effect_type: String,
        data: Value,
        agent_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewSeverity {
    Info,
    Warning,
    Critical,
}

impl AgentEffect {
    pub fn agent_id(&self) -> &str {
        match self {
            Self::TextChange { agent_id, .. }
            | Self::NumericChange { agent_id, .. }
            | Self::StatusChange { agent_id, .. }
            | Self::InfoFragment { agent_id, .. }
            | Self::DialogueLine { agent_id, .. }
            | Self::TaskItem { agent_id, .. }
            | Self::MemoryUpdate { agent_id, .. }
            | Self::EventEmit { agent_id, .. }
            | Self::ToolTrigger { agent_id, .. }
            | Self::ReviewNote { agent_id, .. }
            | Self::ConfigChange { agent_id, .. }
            | Self::RoutingSuggestion { agent_id, .. }
            | Self::Custom { agent_id, .. } => agent_id,
        }
    }
}
