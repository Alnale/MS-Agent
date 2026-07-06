use std::sync::Arc;

use agent_core::error::Result;
use agent_core::memory::MemoryEntry;
use agent_core::memory_store::MemoryStore;

/// Result of a chain optimization operation
#[derive(Debug)]
pub struct ChainOptimizationResult {
    /// Number of summaries merged
    pub merged: usize,
    /// Number of summaries split
    pub split: usize,
}

/// Optimizes summary chains by merging fragmented short summaries
/// and splitting overloaded ones to improve coherence and retrieval quality.
pub struct SummaryChainOptimizer {
    memory_store: Arc<dyn MemoryStore>,
    /// Maximum characters for a summary to be considered "short"
    short_threshold: usize,
    /// Minimum number of consecutive short summaries to trigger merge
    min_merge_group: usize,
}

impl SummaryChainOptimizer {
    pub fn new(memory_store: Arc<dyn MemoryStore>) -> Self {
        Self {
            memory_store,
            short_threshold: 100,
            min_merge_group: 2,
        }
    }

    pub fn with_short_threshold(mut self, threshold: usize) -> Self {
        self.short_threshold = threshold;
        self
    }

    /// Optimize a session's summary chain: merge fragmented summaries
    pub async fn optimize_summary_chain(
        &self,
        session_id: &str,
    ) -> Result<ChainOptimizationResult> {
        let summaries = self.memory_store.get_summary_chain(session_id, 100).await?;

        if summaries.len() < 3 {
            return Ok(ChainOptimizationResult {
                merged: 0,
                split: 0,
            });
        }

        // Detect fragmented groups: consecutive short summaries
        let mut to_merge: Vec<Vec<MemoryEntry>> = Vec::new();
        let mut current_group: Vec<MemoryEntry> = Vec::new();

        for summary in &summaries {
            if summary.content.len() < self.short_threshold {
                current_group.push(summary.clone());
            } else {
                if current_group.len() >= self.min_merge_group {
                    to_merge.push(current_group.clone());
                }
                current_group.clear();
            }
        }
        // Handle trailing group
        if current_group.len() >= self.min_merge_group {
            to_merge.push(current_group);
        }

        let mut merged_count = 0;
        for group in &to_merge {
            let combined_content: String = group
                .iter()
                .map(|s| s.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            // Create merged summary entry
            let merged_entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: agent_core::memory::MemoryKind::Summary,
                content: combined_content,
                data: Some(serde_json::json!({
                    "merged_from": group.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
                    "merge_type": "fragmentation_fix",
                })),
                embedding: None,
                weight: 0.7, // Slightly higher weight for merged
                created_at: chrono::Utc::now(),
                last_accessed_at: chrono::Utc::now(),
                access_count: 0,
                tags: vec!["merged".to_string()],
                source_agent: "chain_optimizer".to_string(),
                confirmed: false,
                content_hash: None,
                confidence: 0.8,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: group.iter().map(|s| s.id.clone()).collect(),
            };

            // Store merged, archive fragments
            if let Err(e) = self.memory_store.store(merged_entry).await {
                tracing::warn!("Failed to store merged summary: {}", e);
                continue;
            }

            for fragment in group {
                let _ = self.memory_store.delete(&fragment.id).await;
            }

            merged_count += group.len();
        }

        Ok(ChainOptimizationResult {
            merged: merged_count,
            split: 0,
        })
    }
}
