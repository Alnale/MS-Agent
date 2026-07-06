pub mod memory;
pub mod redis;
pub mod redis_memory;

use async_trait::async_trait;
use serde_json::Value;

use agent_core::effect::AgentEffect;
use agent_core::error::Result;
use agent_core::state::{ApplyResult, StateStore};

/// In-memory state store (for development/testing)
pub struct InMemoryStateStore {
    data: dashmap::DashMap<String, Value>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self {
            data: dashmap::DashMap::new(),
        }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateStore for InMemoryStateStore {
    async fn get(&self, key: &str) -> Result<Option<Value>> {
        Ok(self.data.get(key).map(|v| v.clone()))
    }

    async fn set(&self, key: &str, value: Value) -> Result<()> {
        self.data.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        Ok(self.data.remove(key).is_some())
    }

    async fn apply_effects(&self, effects: &[AgentEffect]) -> Result<ApplyResult> {
        let mut applied = 0;
        let mut skipped = 0;
        let errors = Vec::new();

        for effect in effects {
            match effect {
                AgentEffect::TextChange { field, value, .. } => {
                    self.data
                        .insert(field.clone(), Value::String(value.clone()));
                    applied += 1;
                }
                AgentEffect::NumericChange { field, delta, .. } => {
                    let current = self.data.get(field).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    self.data.insert(
                        field.clone(),
                        Value::Number(
                            serde_json::Number::from_f64(current + delta)
                                .unwrap_or(serde_json::Number::from(0)),
                        ),
                    );
                    applied += 1;
                }
                AgentEffect::MemoryUpdate { key, value, .. } => {
                    self.data.insert(key.clone(), value.clone());
                    applied += 1;
                }
                AgentEffect::ConfigChange { key, value, .. } => {
                    self.data.insert(key.clone(), value.clone());
                    applied += 1;
                }
                _ => {
                    skipped += 1;
                }
            }
        }

        Ok(ApplyResult {
            applied,
            skipped,
            errors,
        })
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .data
            .iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .map(|entry| entry.key().clone())
            .collect())
    }
}
