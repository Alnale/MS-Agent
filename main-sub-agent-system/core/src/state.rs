use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::effect::AgentEffect;
use crate::error::Result;

/// Result of applying effects to state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyResult {
    pub applied: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// State store trait — persistence backend
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Get state by key
    async fn get(&self, key: &str) -> Result<Option<Value>>;

    /// Set state by key
    async fn set(&self, key: &str, value: Value) -> Result<()>;

    /// Delete state by key
    async fn delete(&self, key: &str) -> Result<bool>;

    /// Apply effects to state
    async fn apply_effects(&self, effects: &[AgentEffect]) -> Result<ApplyResult>;

    /// List keys with a prefix
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>>;
}
