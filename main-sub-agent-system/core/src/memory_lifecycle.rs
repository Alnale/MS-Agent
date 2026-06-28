use chrono::Utc;
use std::sync::Arc;

use crate::error::AgentTeamsError;
use crate::error::Result;
use crate::memory_store::MemoryStore;

/// Quality thresholds for memory lifecycle decisions
#[derive(Debug, Clone)]
pub struct QualityThresholds {
    /// High quality: promote weight
    pub high_quality: f32,
    /// Low quality: demote weight
    pub low_quality: f32,
    /// Minimum viable quality: below this, archive
    pub min_viable: f32,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            high_quality: 0.8,
            low_quality: 0.4,
            min_viable: 0.2,
        }
    }
}

/// Action taken by the lifecycle manager on a memory entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryLifecycleAction {
    /// Weight and confidence increased
    Promoted,
    /// Minor weight adjustment
    Adjusted,
    /// Significant weight/confidence reduction
    Demoted,
    /// Memory archived (soft-deleted)
    Archived,
}

/// Feedback to apply to a specific memory
#[derive(Debug, Clone)]
pub struct QualityFeedback {
    pub memory_id: String,
    pub quality: f32,
    pub source: String,
}

/// Manages memory lifecycle based on quality feedback.
/// Adjusts weight and confidence based on compression quality scores,
/// creating a feedback loop that promotes high-quality memories and
/// degrades or archives low-quality ones.
pub struct MemoryLifecycleManager {
    store: Arc<dyn MemoryStore>,
    thresholds: QualityThresholds,
}

impl MemoryLifecycleManager {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self {
            store,
            thresholds: QualityThresholds::default(),
        }
    }

    pub fn with_thresholds(mut self, thresholds: QualityThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Apply quality feedback to a memory entry, adjusting its weight and confidence.
    pub async fn apply_quality_feedback(
        &self,
        memory_id: &str,
        quality_score: f32,
        source: &str,
    ) -> Result<MemoryLifecycleAction> {
        let mut entry = self
            .store
            .get_by_id(memory_id)
            .await?
            .ok_or_else(|| AgentTeamsError::NotFound(memory_id.to_string()))?;

        let action = if quality_score >= self.thresholds.high_quality {
            entry.weight = (entry.weight + 0.1).min(1.0);
            entry.confidence = (entry.confidence + 0.05).min(1.0);
            if entry.confidence > 0.9 {
                entry.confirmed = true;
            }
            MemoryLifecycleAction::Promoted
        } else if quality_score >= self.thresholds.low_quality {
            entry.weight = (entry.weight - 0.02).max(0.1);
            MemoryLifecycleAction::Adjusted
        } else if quality_score >= self.thresholds.min_viable {
            entry.weight = (entry.weight - 0.15).max(0.05);
            entry.confidence = (entry.confidence - 0.1).max(0.1);
            MemoryLifecycleAction::Demoted
        } else {
            entry.archived = true;
            entry.weight = 0.01;
            entry.tags.push("low_quality".to_string());
            MemoryLifecycleAction::Archived
        };

        entry.last_accessed_at = Utc::now();
        self.store.store(entry).await?;

        tracing::info!(
            memory_id = %memory_id,
            quality = %quality_score,
            action = ?action,
            source = %source,
            "Applied quality feedback"
        );

        Ok(action)
    }

    /// Batch-apply quality feedback to multiple memories.
    pub async fn batch_apply_feedback(
        &self,
        feedbacks: &[QualityFeedback],
    ) -> Result<Vec<MemoryLifecycleAction>> {
        let mut actions = Vec::with_capacity(feedbacks.len());
        for fb in feedbacks {
            actions.push(
                self.apply_quality_feedback(&fb.memory_id, fb.quality, &fb.source)
                    .await?,
            );
        }
        Ok(actions)
    }
}
