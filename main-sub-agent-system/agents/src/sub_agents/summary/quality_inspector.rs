use std::collections::HashMap;
use std::sync::Arc;

use agent_teams_core::error::Result;
use agent_teams_core::memory::{MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::MemoryStore;

/// Statistics for a single session's summaries
#[derive(Debug, Default)]
pub struct SessionSummaryStats {
    pub summary_count: usize,
    pub avg_confidence: f32,
    pub avg_weight: f32,
    pub hit_count: u32,
}

/// Report from a quality inspection pass
#[derive(Debug)]
pub struct InspectionReport {
    pub total_sessions: usize,
    pub problematic_sessions: Vec<String>,
    pub session_stats: HashMap<String, SessionSummaryStats>,
}

/// Result of a recompression attempt
#[derive(Debug)]
pub struct RecompressionResult {
    pub session_id: String,
    pub success: bool,
}

/// Inspects memory quality across sessions by analyzing summary hit rates,
/// confidence, and weight. Identifies sessions with low-quality summaries
/// that may benefit from recompression.
pub struct MemoryQualityInspector {
    memory_store: Arc<dyn MemoryStore>,
    /// Minimum acceptable retrieval hit rate
    min_hit_rate: u32,
}

impl MemoryQualityInspector {
    pub fn new(memory_store: Arc<dyn MemoryStore>) -> Self {
        Self {
            memory_store,
            min_hit_rate: 1,
        }
    }

    pub fn with_min_hit_rate(mut self, min_hit_rate: u32) -> Self {
        self.min_hit_rate = min_hit_rate;
        self
    }

    /// Inspect all sessions' summary quality
    pub async fn inspect_all_sessions(&self) -> Result<InspectionReport> {
        let summaries = self
            .memory_store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                limit: 1000,
                min_weight: 0.0,
                ..Default::default()
            })
            .await?;

        let mut session_stats: HashMap<String, SessionSummaryStats> = HashMap::new();

        for summary in &summaries.entries {
            if let Some(ref session_id) = summary.session_id {
                let stats = session_stats.entry(session_id.clone()).or_default();
                stats.summary_count += 1;
                stats.avg_confidence += summary.confidence;
                stats.avg_weight += summary.weight;
                stats.hit_count += summary.access_count;
            }
        }

        // Calculate averages
        for stats in session_stats.values_mut() {
            if stats.summary_count > 0 {
                stats.avg_confidence /= stats.summary_count as f32;
                stats.avg_weight /= stats.summary_count as f32;
            }
        }

        // Identify problematic sessions
        let problematic_sessions: Vec<String> = session_stats
            .iter()
            .filter(|(_, stats)| {
                (stats.hit_count < self.min_hit_rate && stats.summary_count > 1)
                    || stats.avg_confidence < 0.5
            })
            .map(|(id, _)| id.clone())
            .collect();

        Ok(InspectionReport {
            total_sessions: session_stats.len(),
            problematic_sessions,
            session_stats,
        })
    }

    /// Get session IDs that need recompression
    pub async fn get_problematic_sessions(&self) -> Result<Vec<String>> {
        let report = self.inspect_all_sessions().await?;
        Ok(report.problematic_sessions)
    }
}
