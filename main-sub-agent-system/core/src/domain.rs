use std::sync::Arc;

use async_trait::async_trait;

use crate::agent_memory_cache::AgentMemoryCache;
use crate::bus::AgentBus;
use crate::context::AgentContext;
use crate::context_provider::ContextProvider;
use crate::pipeline::PipelineStage;
use crate::provider::LlmProvider;
use crate::registry::AgentRegistry;
use crate::routing::RoutingTable;

/// Domain module trait — encapsulates domain-specific logic
#[async_trait]
pub trait DomainModule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;

    async fn create_agents(
        &self,
        registry: &AgentRegistry,
        provider: Arc<dyn LlmProvider>,
    ) -> Vec<String>;

    /// Create agents with pre-configured caches from UnifiedCacheManager.
    /// Default implementation ignores the cache factory and falls back to create_agents().
    async fn create_agents_with_cache(
        &self,
        registry: &AgentRegistry,
        provider: Arc<dyn LlmProvider>,
        _cache_factory: &(dyn Fn(&str) -> AgentMemoryCache + Sync),
    ) -> Vec<String> {
        self.create_agents(registry, provider).await
    }

    fn context_providers(&self) -> Vec<Arc<dyn ContextProvider>> {
        Vec::new()
    }

    fn agent_prompt_extension(&self, _agent_id: &str) -> Option<String> {
        None
    }

    fn extra_pipeline_stages(&self) -> Vec<PipelineStage> {
        Vec::new()
    }

    fn routing_table(&self) -> Option<RoutingTable> {
        None
    }

    async fn on_bus_connected(&self, _bus: &dyn AgentBus, _agent_ids: &[String]) {}

    async fn on_unload(&self, _bus: &dyn AgentBus, _agent_ids: &[String]) {}

    fn validate_context(&self, _ctx: &AgentContext) -> std::result::Result<(), String> {
        Ok(())
    }
}
