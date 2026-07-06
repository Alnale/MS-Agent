use agent_core::effect::AgentEffect;
use agent_core::error::Result;
use agent_core::state::{ApplyResult, StateStore};

/// State applier: persists effects to the state store
pub struct StateApplier {
    store: Box<dyn StateStore>,
}

impl StateApplier {
    pub fn new(store: Box<dyn StateStore>) -> Self {
        Self { store }
    }

    /// Apply effects to state store
    pub async fn apply(&self, effects: &[AgentEffect]) -> Result<ApplyResult> {
        self.store.apply_effects(effects).await
    }
}
