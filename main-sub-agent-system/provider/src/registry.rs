use dashmap::DashMap;
use std::sync::{Arc, RwLock};

use agent_core::provider::LlmProvider;

/// Registry for LLM providers
pub struct ProviderRegistry {
    providers: DashMap<String, Arc<dyn LlmProvider>>,
    default_provider_id: RwLock<Option<String>>,
    default_provider_cache: RwLock<Option<Arc<dyn LlmProvider>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: DashMap::new(),
            default_provider_id: RwLock::new(None),
            default_provider_cache: RwLock::new(None),
        }
    }

    /// Register a provider. First registered provider becomes the default.
    pub fn register(&self, provider: Arc<dyn LlmProvider>) {
        let id = provider.id().to_string();
        self.providers.insert(id.clone(), provider);
        let mut default = self.default_provider_id.write().unwrap_or_else(|e| e.into_inner());
        if default.is_none() {
            *default = Some(id);
            if let Some(p) = self.providers.iter().next().map(|r| r.value().clone()) {
                *self.default_provider_cache.write().unwrap_or_else(|e| e.into_inner()) = Some(p);
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.get(id).map(|p| p.value().clone())
    }

    pub fn get_default(&self) -> Option<Arc<dyn LlmProvider>> {
        // Fast path: return cached Arc without locking providers map
        {
            let cached = self.default_provider_cache.read().unwrap_or_else(|e| e.into_inner());
            if cached.is_some() {
                return cached.clone();
            }
        }
        // Slow path: look up by id and cache
        let id = self.default_provider_id.read().unwrap_or_else(|e| e.into_inner()).clone();
        let provider = id.and_then(|id| self.get(&id));
        if let Some(ref p) = provider {
            *self.default_provider_cache.write().unwrap_or_else(|e| e.into_inner()) = Some(p.clone());
        }
        provider
    }

    pub fn set_default(&self, id: &str) {
        *self.default_provider_id.write().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
        // Invalidate cache so next get_default picks up the new provider
        *self.default_provider_cache.write().unwrap_or_else(|e| e.into_inner()) = self.get(id);
    }

    pub fn list(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|e| e.key().clone())
            .collect::<Vec<_>>()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
