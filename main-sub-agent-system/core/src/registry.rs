use std::sync::Arc;

use dashmap::DashMap;

use crate::boxed_agent::BoxedAgent;

/// Shared boxed agent type
pub type SharedAgent = Arc<dyn BoxedAgent>;

/// Thread-safe registry for all agents
pub struct AgentRegistry {
    agents: DashMap<String, SharedAgent>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: DashMap::new(),
        }
    }

    /// Register an agent
    pub async fn register(&self, agent: SharedAgent) {
        let id = agent.id().to_string();
        tracing::info!("Registering agent: {}", id);
        self.agents.insert(id, agent);
    }

    /// Get an agent by ID
    pub async fn get(&self, id: &str) -> Option<SharedAgent> {
        self.agents.get(id).map(|a| a.clone())
    }

    /// Unregister an agent
    pub async fn unregister(&self, id: &str) -> bool {
        let removed = self.agents.remove(id).is_some();
        if removed {
            tracing::info!("Unregistered agent: {}", id);
        }
        removed
    }

    /// List all registered agent IDs
    pub async fn list(&self) -> Vec<String> {
        self.agents
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all agents
    pub async fn all(&self) -> Vec<SharedAgent> {
        self.agents
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Count of registered agents
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
