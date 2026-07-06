use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};
use serde_json::Value;

use agent_core::effect::AgentEffect;
use agent_core::error::{AgentTeamsError, Result};
use agent_core::state::{ApplyResult, StateStore};

/// Redis-backed state store
pub struct RedisStateStore {
    conn: ConnectionManager,
}

impl RedisStateStore {
    pub async fn connect(redis_url: &str) -> std::result::Result<Self, RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    fn state_key(key: &str) -> String {
        format!("state:{}", key)
    }


}

#[async_trait]
impl StateStore for RedisStateStore {
    async fn get(&self, key: &str) -> Result<Option<Value>> {
        let mut conn = self.conn.clone();
        let redis_key = Self::state_key(key);
        let data: Option<String> = conn.get(&redis_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        match data {
            Some(json_str) => {
                let value: Value = serde_json::from_str(&json_str).map_err(|e| {
                    AgentTeamsError::StateStoreError(e.to_string())
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, value: Value) -> Result<()> {
        let mut conn = self.conn.clone();
        let redis_key = Self::state_key(key);
        let json_str = serde_json::to_string(&value).map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        let _: () = conn.set(&redis_key, &json_str).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        // Add to prefix index for list_keys support
        // Index under every possible prefix
        let _: () = conn
            .sadd("state_keys", key)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        let mut conn = self.conn.clone();
        let redis_key = Self::state_key(key);
        let count: i32 = conn.del(&redis_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if count > 0 {
            let _: () = conn
                .srem("state_keys", key)
                .await
                .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn apply_effects(&self, effects: &[AgentEffect]) -> Result<ApplyResult> {
        let mut conn = self.conn.clone();
        let mut applied = 0;
        let mut skipped = 0;
        let errors: Vec<String> = Vec::new();

        // Use a Redis pipeline for atomicity
        let mut pipe = redis::pipe();

        for effect in effects {
            match effect {
                AgentEffect::TextChange { field, value, .. } => {
                    let json_str = serde_json::to_string(&Value::String(value.clone()))
                        .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
                    let redis_key = Self::state_key(field);
                    pipe.set(&redis_key, &json_str).ignore();
                    pipe.sadd("state_keys", field).ignore();
                    applied += 1;
                }
                AgentEffect::NumericChange { field, delta, .. } => {
                    // INCRBYFLOAT is atomic in Redis — avoids the lost-update
                    // race where two concurrent requests both read the same
                    // baseline value and one delta is silently dropped. The
                    // previous code did `GET ... await ... SET` which yielded
                    // between the read and the write. Stores a plain numeric
                    // string (e.g. "42.5") which serde_json parses back into
                    // Value::Number on get(), preserving the prior format.
                    //
                    // Executed as a standalone command (not in the pipeline)
                    // because redis 0.27's Pipeline doesn't expose incr_by_float.
                    let redis_key = Self::state_key(field);
                    let _: f64 = redis::cmd("INCRBYFLOAT")
                        .arg(&redis_key)
                        .arg(*delta)
                        .query_async(&mut conn)
                        .await
                        .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
                    pipe.sadd("state_keys", field).ignore();
                    applied += 1;
                }
                AgentEffect::MemoryUpdate { key, value, .. } => {
                    let json_str = serde_json::to_string(value)
                        .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
                    let redis_key = Self::state_key(key);
                    pipe.set(&redis_key, &json_str).ignore();
                    pipe.sadd("state_keys", key).ignore();
                    applied += 1;
                }
                AgentEffect::ConfigChange { key, value, .. } => {
                    let json_str = serde_json::to_string(value)
                        .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
                    let redis_key = Self::state_key(key);
                    pipe.set(&redis_key, &json_str).ignore();
                    pipe.sadd("state_keys", key).ignore();
                    applied += 1;
                }
                _ => {
                    skipped += 1;
                }
            }
        }

        // Execute the pipeline for batch write
        if applied > 0 {
            pipe.query_async::<()>(&mut conn).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
        }

        Ok(ApplyResult {
            applied,
            skipped,
            errors,
        })
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let mut conn = self.conn.clone();

        // Get all keys from the set and filter by prefix
        let all_keys: Vec<String> = conn.smembers("state_keys").await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        let mut matching: Vec<String> = all_keys
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .collect();

        matching.sort();
        Ok(matching)
    }
}
