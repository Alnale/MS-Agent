use std::sync::Arc;

use agent_core::bus::AgentBus;
use agent_core::registry::SharedAgent;

/// Bus dispatcher: manages agent subscriptions to the bus
pub struct BusDispatcher {
    bus: Arc<dyn AgentBus>,
}

impl BusDispatcher {
    pub fn new(bus: Arc<dyn AgentBus>) -> Self {
        Self { bus }
    }

    /// Register an agent with the bus
    pub async fn register_agent(&self, agent: SharedAgent) {
        let agent_id = agent.id().to_string();
        let mut receiver = self.bus.subscribe(agent_id.clone()).await;

        tokio::spawn(async move {
            while let Some(_envelope) = receiver.recv().await {
                // Bus message handling would go here
                // For now, this is a placeholder for inter-agent communication
                tracing::debug!("Agent {} received bus message", agent_id);
            }
        });
    }

    /// Unregister an agent from the bus
    pub async fn unregister_agent(&self, agent_id: &str) {
        let _ = self.bus.unsubscribe(agent_id).await;
    }
}
