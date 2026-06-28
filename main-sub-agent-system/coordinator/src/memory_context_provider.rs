use async_trait::async_trait;

use agent_teams_core::context::AgentContext;
use agent_teams_core::context_provider::{ContextProvider, PromptFragment, PromptPriority};

use crate::memory_manager::MemoryManager;

/// Coordinator-level context provider that injects per-request working memory
/// from AgentContext into agent prompts. Reads from the request-scoped
/// ctx.working_memory (populated by prepare_pipeline) to ensure session isolation.
pub struct MemoryContextProvider;

impl MemoryContextProvider {
    pub fn new(_memory_manager: std::sync::Arc<MemoryManager>) -> Self {
        Self
    }
}

#[async_trait]
impl ContextProvider for MemoryContextProvider {
    fn id(&self) -> &str {
        "coordinator_memory"
    }

    fn priority(&self) -> PromptPriority {
        PromptPriority::World
    }

    async fn provide(&self, ctx: &AgentContext) -> Option<PromptFragment> {
        let memories = &ctx.working_memory;
        tracing::info!(
            "MemoryContextProvider: {} memories in working memory (session={})",
            memories.len(),
            ctx.session_id
        );

        if memories.is_empty() {
            return None;
        }

        let memory_prompt = MemoryManager::build_memory_prompt(memories);
        if memory_prompt.is_empty() {
            tracing::info!("MemoryContextProvider: memory prompt is empty");
            return None;
        }

        tracing::info!(
            "MemoryContextProvider: providing memory prompt ({} chars)",
            memory_prompt.len()
        );
        Some(PromptFragment {
            source: "coordinator_memory".to_string(),
            content: memory_prompt,
            priority: PromptPriority::World,
        })
    }
}
