use std::sync::Arc;

use agent_teams_core::config::AppConfig;
use agent_teams_core::context_provider::ContextProvider;
use agent_teams_core::domain::DomainModule;
use agent_teams_core::event::EventBus;
use agent_teams_core::hook::HookRegistry;
use agent_teams_core::memory_store::{EmbeddingError, EmbeddingProvider};
use agent_teams_core::registry::AgentRegistry;
use agent_teams_core::tool::UnifiedToolRegistry;


use agent_teams_agents::main_agent::{MainAgent, MainAgentConfig};
use agent_teams_coordinator::memory_manager::MemoryManager;
use agent_teams_coordinator::MainAgentCoordinator;
use agent_teams_provider::registry::ProviderRegistry;
use agent_teams_storage::memory::InMemoryMemoryStore;

/// Feature-hashing TF-IDF embedding provider for development/testing.
/// Produces deterministic vectors that capture term-level similarity without external APIs.
/// Uses the hashing trick: maps token n-grams to vector dimensions via FNV-1a hash,
/// weighted by log-frequency (TF-IDF approximation).
pub struct HashEmbeddingProvider {
    dimensions: usize,
}

impl HashEmbeddingProvider {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    /// FNV-1a hash for feature hashing
    fn fnv1a_hash(bytes: &[u8]) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        for &b in bytes {
            hash ^= b as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }

    /// Tokenize text into lowercased word-like segments, including CJK unigrams
    fn tokenize(text: &str) -> Vec<String> {
        let lower = text.to_ascii_lowercase();
        let mut tokens = Vec::new();
        let mut current = String::new();

        for ch in lower.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                current.push(ch);
            } else {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                // CJK characters as individual unigrams
                if !ch.is_ascii() && !ch.is_whitespace() && !ch.is_control() {
                    tokens.push(ch.to_string());
                }
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for HashEmbeddingProvider {
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError> {
        let mut embedding = vec![0.0f32; self.dimensions];
        let tokens = Self::tokenize(text);

        if tokens.is_empty() {
            return Ok(embedding);
        }

        // Count term frequencies for TF weighting
        let mut tf: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for token in &tokens {
            *tf.entry(token.clone()).or_insert(0) += 1;
        }

        // Feature hashing with signed hash trick:
        // - Hash token to dimension index
        // - Hash token to sign (+1 or -1)
        // - Weight by log(1 + tf) — sublinear TF scaling
        for (token, count) in &tf {
            let idx_hash = Self::fnv1a_hash(token.as_bytes());
            let sign_hash = Self::fnv1a_hash(&idx_hash.to_le_bytes());
            let idx = (idx_hash as usize) % self.dimensions;
            let sign = if sign_hash % 2 == 0 { 1.0f32 } else { -1.0f32 };
            let weight = (1.0 + *count as f32).ln();
            embedding[idx] += sign * weight;
        }

        // Add bigram features for better context capture
        for window in tokens.windows(2) {
            let bigram = format!("{}|{}", window[0], window[1]);
            let idx_hash = Self::fnv1a_hash(bigram.as_bytes());
            let sign_hash = Self::fnv1a_hash(&idx_hash.to_le_bytes());
            let idx = (idx_hash as usize) % self.dimensions;
            let sign = if sign_hash % 2 == 0 { 1.0f32 } else { -1.0f32 };
            embedding[idx] += sign * 0.5; // Lower weight for bigrams
        }

        // L2 normalize
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut embedding {
                *val /= norm;
            }
        }

        Ok(embedding)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        "tfidf-hash-embedding-v1"
    }
}

/// Runtime builder: configures and starts the agent system
pub struct RuntimeBuilder {
    config: AppConfig,
    provider_registry: ProviderRegistry,
    registry: Arc<AgentRegistry>,
    hook_registry: Arc<HookRegistry>,
    tool_registry: Arc<UnifiedToolRegistry>,
    context_providers: Vec<Arc<dyn ContextProvider>>,
    event_bus: EventBus,
}

impl RuntimeBuilder {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            provider_registry: ProviderRegistry::new(),
            registry: Arc::new(AgentRegistry::new()),
            hook_registry: Arc::new(HookRegistry::new()),
            tool_registry: Arc::new(UnifiedToolRegistry::new()),
            context_providers: Vec::new(),
            event_bus: EventBus::new(256),
        }
    }

    pub async fn with_provider(
        self,
        provider: Arc<dyn agent_teams_core::provider::LlmProvider>,
    ) -> Self {
        self.provider_registry.register(provider);
        self
    }

    pub fn with_context_provider(mut self, cp: Arc<dyn ContextProvider>) -> Self {
        self.context_providers.push(cp);
        self
    }

    /// Build the coordinator, returning the tool registry separately so it can
    /// be shared with the HTTP layer.
    pub async fn build(self) -> Result<(MainAgentCoordinator, Arc<AgentRegistry>, Arc<UnifiedToolRegistry>), String> {
        // Validate configuration
        if let Err(e) = self.config.validate() {
            return Err(format!("Configuration validation failed: {}", e));
        }

        let default_provider = match self.provider_registry.get_default().or_else(|| {
            self.provider_registry
                .list()
                .first()
                .and_then(|id| self.provider_registry.get(id))
        }) {
            Some(p) => p,
            None => {
                return Err("No LLM provider registered. Check config.json: ensure at least one provider has a valid api_key set via ${{ENV_VAR}} or directly.".to_string());
            }
        };

        // Discover and register external tools (MCP, OpenAPI)
        if let Some(ref external_tools) = self.config.external_tools {
            for source in external_tools {
                match source {
                    agent_teams_core::tool::ExternalToolSource::Mcp { endpoint, transport } => {
                        let transport_type = match transport {
                            agent_teams_core::tool::McpTransport::Sse => {
                                agent_teams_agents::tool_discovery::mcp::McpTransport::Sse
                            }
                            agent_teams_core::tool::McpTransport::Stdio => {
                                agent_teams_agents::tool_discovery::mcp::McpTransport::Stdio
                            }
                        };
                        match agent_teams_agents::tool_discovery::mcp::McpToolAdapter::connect(
                            endpoint,
                            transport_type,
                        )
                        .await
                        {
                            Ok(adapter) => {
                                if let Err(e) =
                                    adapter.register_tools(&self.tool_registry).await
                                {
                                    tracing::warn!(
                                        "Failed to register MCP tools from '{}': {}",
                                        endpoint,
                                        e
                                    );
                                } else {
                                    tracing::info!(
                                        "Registered MCP tools from '{}'",
                                        endpoint
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to connect to MCP server '{}': {}",
                                    endpoint,
                                    e
                                );
                            }
                        }
                    }
                    agent_teams_core::tool::ExternalToolSource::OpenApi { url, auth: _ } => {
                        match agent_teams_agents::tool_discovery::openapi::OpenApiImporter::import_from_url(
                            url,
                            &self.tool_registry,
                        )
                        .await
                        {
                            Ok(tools) => {
                                tracing::info!(
                                    "Imported {} tools from OpenAPI spec '{}'",
                                    tools.len(),
                                    url
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to import OpenAPI tools from '{}': {}",
                                    url,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        // Create domain module and extract routing table
        let domain = agent_teams_agents::domain_cs::DomainModuleCS::new(default_provider.clone())
            .with_tool_registry(self.tool_registry.clone())
            .with_sub_agents_config(self.config.sub_agents.clone());
        let routing_table = domain.routing_table();

        // Get default model from provider config
        let default_model = self.config.providers.default_model();

        let main_agent_config = MainAgentConfig {
            thinking_enabled: self.config.main_agent.thinking.enabled,
            thinking_budget_tokens: self.config.main_agent.thinking.budget_tokens,
            critic_enabled: self.config.main_agent.critic.enabled,
            max_refinement_rounds: self.config.main_agent.critic.max_refinement_rounds,
            total_timeout_ms: self.config.main_agent.total_timeout_ms,
            plan_cache_ttl_secs: self.config.main_agent.plan_cache.ttl_secs,
            plan_cache_capacity: self.config.main_agent.plan_cache.capacity,
            default_model: default_model.clone(),
            ..MainAgentConfig::default()
        };

        let main_agent = Arc::new(
            MainAgent::new(
                default_provider.clone(),
                main_agent_config.clone(),
                routing_table,
            )
            .await
            .with_tool_registry(self.tool_registry.clone()),
        );

        // Register SubAgent descriptors on the coordinator's MainAgent
        for descriptor in agent_teams_agents::domain_cs::DomainModuleCS::sub_agent_descriptors() {
            main_agent.register_descriptor(descriptor).await;
        }

        // Initialize memory system FIRST so we can wire global_store into the bus
        let memory_config = self
            .config
            .memory
            .clone()
            .unwrap_or_default();
        let memory_manager: Option<Arc<MemoryManager>> = if memory_config.enabled {
            tracing::info!("Initializing memory system...");

            let short_term_store = Arc::new(
                InMemoryMemoryStore::new().with_default_ttl(memory_config.short_term_ttl_secs),
            );
            let long_term_store = Arc::new(InMemoryMemoryStore::new());

            let embedding_provider: Arc<dyn EmbeddingProvider> =
                Arc::new(HashEmbeddingProvider::new(128));

            let mm = Arc::new(
                MemoryManager::new(
                    short_term_store,
                    long_term_store,
                    embedding_provider,
                    memory_config,
                )
                .with_llm_provider(default_provider.clone()),
            );

            agent_teams_coordinator::memory_manager::start_memory_maintenance(mm.clone());
            Some(mm)
        } else {
            tracing::info!("Memory system disabled in config");
            None
        };

        // Create unified cache infrastructure for SubAgents
        let shared_cache = Arc::new(
            agent_teams_core::unified_memory_bus::SharedMemoryCache::new(
                main_agent_config.shared_cache_capacity,
            ),
        );

        // Build UnifiedMemoryBus with global_store wired in from the start
        let cache_config = self
            .config
            .unified_cache
            .clone()
            .unwrap_or_default();
        let mut bus_builder = agent_teams_core::unified_memory_bus::UnifiedMemoryBus::new(
            main_agent_config.shared_cache_capacity,
        )
        .with_memory_event_bus(Arc::new(
            agent_teams_core::memory_event_bus::MemoryEventBus::new(
                main_agent_config.memory_event_bus_capacity,
            ),
        ));
        if cache_config.enable_unified_bus {
            if let Some(ref mm) = memory_manager {
                bus_builder = bus_builder.with_global_store(mm.long_term_store().clone());
            }
        }
        let unified_bus = Arc::new(bus_builder);

        let global_store: Option<Arc<dyn agent_teams_core::memory_store::MemoryStore>> =
            memory_manager.as_ref().map(|mm| mm.long_term_store().clone());

        let cache_manager = agent_teams_coordinator::UnifiedCacheManager::new(
            agent_teams_coordinator::plan_cache::PlanCache::new(
                main_agent_config.plan_cache_capacity,
                main_agent_config.plan_cache_ttl_secs,
            ),
            shared_cache,
            global_store,
            unified_bus,
        );

        // Register all domain agents with caches from the manager
        let agent_ids = if main_agent_config.unified_cache_enabled {
            domain
                .create_agents_with_cache(&self.registry, default_provider.clone(), &|agent_id| {
                    cache_manager.get_or_create_agent_cache(agent_id, 100)
                })
                .await
        } else {
            domain
                .create_agents(&self.registry, default_provider.clone())
                .await
        };
        tracing::info!(
            "Registered {} agents from domain: {:?}",
            agent_ids.len(),
            agent_ids
        );

        let mut coordinator = MainAgentCoordinator::new(main_agent, self.registry.clone())
            .with_hooks(self.hook_registry)
            .with_event_bus(self.event_bus)
            .with_context_providers(self.context_providers)
            .with_unified_bus(cache_manager.memory_bus().clone())
            .with_tool_registry(self.tool_registry.clone());

        // Wire memory manager to coordinator
        if let Some(mm) = memory_manager {
            coordinator = coordinator.with_memory_manager(mm);
        }

        // Register SubAgent memory caches with the bus
        if cache_config.enable_unified_bus {
            coordinator.register_sub_agent_caches().await;
            tracing::info!(
                "Unified memory bus enabled (shared_capacity={})",
                cache_config.shared_cache_capacity
            );
        }

        // Wire in CriticAgent if critic is enabled in config
        if self.config.main_agent.critic.enabled {
            let critic = agent_teams_coordinator::critic::CriticAgent::new(
                default_provider.clone(),
                self.config.main_agent.critic.max_refinement_rounds,
                &default_model,
            )
            .with_thinking(
                self.config.main_agent.critic.thinking.as_ref().is_some_and(|t| t.enabled),
                self.config.main_agent.critic.thinking.as_ref().map_or(0, |t| t.budget_tokens),
            );
            coordinator = coordinator.with_critic(critic);
            tracing::info!(
                "CriticAgent enabled (max {} rounds, thinking={})",
                self.config.main_agent.critic.max_refinement_rounds,
                self.config.main_agent.critic.thinking.as_ref().is_some_and(|t| t.enabled)
            );
        }

        // Apply cost optimization config
        if let Some(cost_config) = self.config.cost_optimization.clone() {
            coordinator = coordinator.with_cost_config(cost_config.clone());
            tracing::info!(
                "Cost optimization enabled: skip_thinking_for_simple={}, skip_critic_for_simple={}",
                cost_config.skip_thinking_for_simple,
                cost_config.skip_critic_for_simple
            );
        }

        // Apply degradation config
        if let Some(degradation_config) = self.config.degradation.clone() {
            coordinator = coordinator.with_degradation_config(degradation_config);
            tracing::info!("Degradation config loaded");
        }

        Ok((coordinator, self.registry, self.tool_registry))
    }

    pub fn registry(&self) -> &Arc<AgentRegistry> {
        &self.registry
    }

    pub fn hook_registry(&self) -> &Arc<HookRegistry> {
        &self.hook_registry
    }

    pub fn tool_registry(&self) -> &Arc<UnifiedToolRegistry> {
        &self.tool_registry
    }
}
