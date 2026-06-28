use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::AgentContext;

/// Priority levels for prompt fragments
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PromptPriority {
    Critical = 0,
    World = 10,
    Narrative = 20,
    Mechanical = 30,
    History = 40,
}

/// A fragment of prompt text to be assembled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptFragment {
    pub source: String,
    pub content: String,
    pub priority: PromptPriority,
}

/// Context provider trait — injects context into agent prompts
#[async_trait]
pub trait ContextProvider: Send + Sync {
    fn id(&self) -> &str;
    fn priority(&self) -> PromptPriority {
        PromptPriority::Mechanical
    }
    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment>;
}
