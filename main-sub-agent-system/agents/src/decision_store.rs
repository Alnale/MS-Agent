use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::main_agent::DecisionRecord;

/// Persistent storage for decision records with async I/O
pub struct DecisionStore {
    storage_path: PathBuf,
    records: Arc<RwLock<Vec<DecisionRecord>>>,
    max_records: usize,
}

impl DecisionStore {
    /// Create a new DecisionStore with file-based persistence
    pub async fn new(storage_path: &str, max_records: usize) -> Self {
        let path = PathBuf::from(storage_path);
        let records = Self::load_from_file(&path).await.unwrap_or_default();

        Self {
            storage_path: path,
            records: Arc::new(RwLock::new(records)),
            max_records,
        }
    }

    /// Add a new decision record
    pub async fn add_record(&self, record: DecisionRecord) {
        let mut records = self.records.write().await;
        records.push(record);

        // Trim to max records
        if records.len() > self.max_records {
            let excess = records.len() - self.max_records;
            records.drain(0..excess);
        }

        // Save to file asynchronously
        let records_clone = records.clone();
        let path = self.storage_path.clone();
        tokio::spawn(async move {
            Self::save_to_file(&path, &records_clone).await;
        });
    }

    /// Get all records
    pub async fn get_records(&self) -> Vec<DecisionRecord> {
        self.records.read().await.clone()
    }

    /// Get records for a specific agent
    pub async fn get_records_for_agent(&self, agent_id: &str) -> Vec<DecisionRecord> {
        self.records
            .read()
            .await
            .iter()
            .filter(|r| r.agents_called.contains(&agent_id.to_string()))
            .cloned()
            .collect()
    }

    /// Get recent records (last N)
    pub async fn get_recent_records(&self, count: usize) -> Vec<DecisionRecord> {
        let records = self.records.read().await;
        let start = if records.len() > count {
            records.len() - count
        } else {
            0
        };
        records[start..].to_vec()
    }

    /// Calculate average quality for an agent
    pub async fn average_quality_for_agent(&self, agent_id: &str) -> f32 {
        let records = self.get_records_for_agent(agent_id).await;
        if records.is_empty() {
            return 0.0;
        }

        let sum: f32 = records.iter().map(|r| r.outcome_quality).sum();
        sum / records.len() as f32
    }

    /// Calculate success rate for an agent
    pub async fn success_rate_for_agent(&self, agent_id: &str) -> f32 {
        let records = self.get_records_for_agent(agent_id).await;
        if records.is_empty() {
            return 0.0;
        }

        let successes = records
            .iter()
            .filter(|r| !r.agents_errored.contains(&agent_id.to_string()))
            .count();

        successes as f32 / records.len() as f32
    }

    /// Get average duration for an agent
    pub async fn average_duration_for_agent(&self, agent_id: &str) -> u64 {
        let records = self.get_records_for_agent(agent_id).await;
        if records.is_empty() {
            return 0;
        }

        let sum: u64 = records.iter().map(|r| r.actual_duration_ms).sum();
        sum / records.len() as u64
    }

    /// Load records from file asynchronously
    async fn load_from_file(path: &PathBuf) -> Option<Vec<DecisionRecord>> {
        if !path.exists() {
            return None;
        }

        let content = tokio::fs::read_to_string(path).await.ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save records to file asynchronously
    async fn save_to_file(path: &PathBuf, records: &[DecisionRecord]) {
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                tracing::error!("Failed to create decision store directory {:?}: {}", parent, e);
                return;
            }
        }

        match serde_json::to_string_pretty(records) {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(path, content).await {
                    tracing::error!("Failed to write decision store {:?}: {}", path, e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to serialize decision records: {}", e);
            }
        }
    }

    /// Clear all records
    pub async fn clear(&self) {
        let mut records = self.records.write().await;
        records.clear();

        let path = self.storage_path.clone();
        tokio::spawn(async move {
            Self::save_to_file(&path, &[]).await;
        });
    }

    /// Get the number of records
    pub async fn len(&self) -> usize {
        self.records.read().await.len()
    }

    /// Check if the store is empty
    pub async fn is_empty(&self) -> bool {
        self.records.read().await.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_record(agent_id: &str, quality: f32) -> DecisionRecord {
        DecisionRecord {
            input_hash: "test_hash".to_string(),
            plan: agent_core::plan::ExecutionPlan {
                stages: vec![],
                strategy: "test".to_string(),
                estimated_duration_ms: 1000,
                confidence: 0.9,
                nodes: Vec::new(),
                tool_intent: None,
            },
            outcome_quality: quality,
            actual_duration_ms: 1000,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            agents_called: vec![agent_id.to_string()],
            agents_errored: vec![],
        }
    }

    #[tokio::test]
    async fn test_decision_store_add_and_get() {
        let temp_dir = std::env::temp_dir();
        let unique = format!("test_decision_store_{}.json", std::process::id());
        let path = temp_dir.join(&unique);
        let store = DecisionStore::new(path.to_str().unwrap(), 100).await;

        let record = create_test_record("test_agent", 0.9);
        store.add_record(record).await;

        assert_eq!(store.len().await, 1);
        assert!(!store.is_empty().await);

        let records = store.get_records().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome_quality, 0.9);

        // Cleanup
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn test_decision_store_max_records() {
        let temp_dir = std::env::temp_dir();
        let unique = format!("test_decision_store_max_{}.json", std::process::id());
        let path = temp_dir.join(&unique);
        let store = DecisionStore::new(path.to_str().unwrap(), 2).await;

        store.add_record(create_test_record("agent1", 0.8)).await;
        store.add_record(create_test_record("agent2", 0.9)).await;
        store.add_record(create_test_record("agent3", 0.7)).await;

        assert_eq!(store.len().await, 2);

        // Cleanup
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn test_decision_store_agent_stats() {
        let temp_dir = std::env::temp_dir();
        let unique = format!("test_decision_store_stats_{}.json", std::process::id());
        let path = temp_dir.join(&unique);
        let store = DecisionStore::new(path.to_str().unwrap(), 100).await;

        store.add_record(create_test_record("agent1", 0.8)).await;
        store.add_record(create_test_record("agent1", 0.9)).await;
        store.add_record(create_test_record("agent2", 0.7)).await;

        let avg_quality = store.average_quality_for_agent("agent1").await;
        assert!((avg_quality - 0.85).abs() < 0.01);

        let success_rate = store.success_rate_for_agent("agent1").await;
        assert!((success_rate - 1.0).abs() < 0.01);

        let avg_duration = store.average_duration_for_agent("agent1").await;
        assert_eq!(avg_duration, 1000);

        // Cleanup
        let _ = tokio::fs::remove_file(path).await;
    }
}
