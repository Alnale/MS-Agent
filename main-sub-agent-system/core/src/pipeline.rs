use serde::{Deserialize, Serialize};

use crate::provider::ThinkingConfig;

/// Pipeline definition — can be static (from config) or dynamic (from MainAgent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDef {
    pub stages: Vec<PipelineStage>,
    pub total_timeout_ms: u64,
    #[serde(default)]
    pub thinking: ThinkingConfig,
    #[serde(default)]
    pub critic_enabled: bool,
}

impl Default for PipelineDef {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            total_timeout_ms: 90_000,
            thinking: ThinkingConfig::default(),
            critic_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    pub name: String,
    pub mode: StageMode,
    pub agent_ids: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StageMode {
    Parallel,
    Sequential,
}
