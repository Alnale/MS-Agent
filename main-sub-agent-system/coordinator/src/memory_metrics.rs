use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use agent_core::error::Result;
use agent_core::memory::{MemoryEntry, MemoryQuery, MemoryRetrievalResult};
use agent_core::memory_store::MemoryStore;
use async_trait::async_trait;

/// Metrics collector for memory system observability
pub struct MemoryMetrics {
    /// Memory retrieval latency in microseconds
    pub retrieval_latency_us: AtomicU64,
    /// Embedding generation latency in microseconds
    pub embedding_latency_us: AtomicU64,
    /// Cache hit count
    pub cache_hits: AtomicU64,
    /// Cache miss count
    pub cache_misses: AtomicU64,
    /// Current working memory token count
    pub working_tokens: AtomicU64,
    /// Total compression runs
    pub compression_runs: AtomicU64,
    /// Total retrieval count
    pub retrieval_count: AtomicU64,
    /// Total store count
    pub store_count: AtomicU64,
    /// Long-term memory entry count
    pub long_term_count: AtomicU64,
}

impl MemoryMetrics {
    pub fn new() -> Self {
        Self {
            retrieval_latency_us: AtomicU64::new(0),
            embedding_latency_us: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            working_tokens: AtomicU64::new(0),
            compression_runs: AtomicU64::new(0),
            retrieval_count: AtomicU64::new(0),
            store_count: AtomicU64::new(0),
            long_term_count: AtomicU64::new(0),
        }
    }

    /// Record a retrieval operation timing
    pub fn record_retrieval(&self, latency: std::time::Duration) {
        self.retrieval_latency_us
            .fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
        self.retrieval_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an embedding operation timing
    pub fn record_embedding(&self, latency: std::time::Duration) {
        self.embedding_latency_us
            .fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
    }

    /// Record a cache hit
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cache miss
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Update working memory token count
    pub fn set_working_tokens(&self, tokens: u64) {
        self.working_tokens.store(tokens, Ordering::Relaxed);
    }

    /// Update long-term memory count
    pub fn set_long_term_count(&self, count: u64) {
        self.long_term_count.store(count, Ordering::Relaxed);
    }

    /// Record a compression run
    pub fn record_compression(&self) {
        self.compression_runs.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a store operation
    pub fn record_store(&self) {
        self.store_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get average retrieval latency in milliseconds
    pub fn avg_retrieval_latency_ms(&self) -> f64 {
        let count = self.retrieval_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let total_us = self.retrieval_latency_us.load(Ordering::Relaxed);
        (total_us as f64 / count as f64) / 1000.0
    }

    /// Get cache hit rate (0.0 - 1.0)
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            return 0.0;
        }
        hits as f64 / total as f64
    }

    /// Get a snapshot of all metrics as a structured log
    pub fn log_snapshot(&self) {
        tracing::info!(
            retrieval_count = self.retrieval_count.load(Ordering::Relaxed),
            avg_retrieval_latency_ms = self.avg_retrieval_latency_ms(),
            cache_hit_rate = self.cache_hit_rate(),
            working_tokens = self.working_tokens.load(Ordering::Relaxed),
            long_term_count = self.long_term_count.load(Ordering::Relaxed),
            compression_runs = self.compression_runs.load(Ordering::Relaxed),
            store_count = self.store_count.load(Ordering::Relaxed),
            "Memory metrics snapshot"
        );
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.retrieval_latency_us.store(0, Ordering::Relaxed);
        self.embedding_latency_us.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.working_tokens.store(0, Ordering::Relaxed);
        self.compression_runs.store(0, Ordering::Relaxed);
        self.retrieval_count.store(0, Ordering::Relaxed);
        self.store_count.store(0, Ordering::Relaxed);
        self.long_term_count.store(0, Ordering::Relaxed);
    }
}

impl Default for MemoryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Instrumented MemoryStore wrapper that records metrics
pub struct InstrumentedMemoryStore {
    inner: Arc<dyn MemoryStore>,
    metrics: Arc<MemoryMetrics>,
}

impl InstrumentedMemoryStore {
    pub fn new(inner: Arc<dyn MemoryStore>, metrics: Arc<MemoryMetrics>) -> Self {
        Self { inner, metrics }
    }
}

#[async_trait]
impl MemoryStore for InstrumentedMemoryStore {
    async fn store(&self, entry: MemoryEntry) -> Result<()> {
        self.metrics.record_store();
        self.inner.store(entry).await
    }

    async fn store_batch(&self, entries: Vec<MemoryEntry>) -> Result<()> {
        self.metrics.record_store();
        self.inner.store_batch(entries).await
    }

    async fn retrieve(&self, query: MemoryQuery) -> Result<MemoryRetrievalResult> {
        let start = Instant::now();
        let result = self.inner.retrieve(query).await;
        self.metrics.record_retrieval(start.elapsed());
        result
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<MemoryEntry>> {
        self.inner.get_by_id(id).await
    }

    async fn touch(&self, id: &str) -> Result<()> {
        self.inner.touch(id).await
    }

    async fn promote(&self, id: &str, delta: f32) -> Result<()> {
        self.inner.promote(id, delta).await
    }

    async fn decay(
        &self,
        before: chrono::DateTime<chrono::Utc>,
        decay_factor: f32,
    ) -> Result<usize> {
        self.inner.decay(before, decay_factor).await
    }

    async fn prune(&self, min_weight: f32, before: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        self.inner.prune(min_weight, before).await
    }

    async fn compress(
        &self,
        session_id: &str,
        strategy: agent_core::memory::CompressionStrategy,
    ) -> Result<Vec<MemoryEntry>> {
        self.metrics.record_compression();
        self.inner.compress(session_id, strategy).await
    }

    async fn list_session_memories(&self, session_id: &str) -> Result<Vec<MemoryEntry>> {
        self.inner.list_session_memories(session_id).await
    }

    async fn get_user_profile(&self, user_id: &str) -> Result<Option<serde_json::Value>> {
        self.inner.get_user_profile(user_id).await
    }

    async fn update_user_profile(&self, user_id: &str, profile: serde_json::Value) -> Result<()> {
        self.inner.update_user_profile(user_id, profile).await
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        self.inner.delete(id).await
    }

    async fn add_relation(&self, relation: agent_core::memory::MemoryRelation) -> Result<()> {
        self.inner.add_relation(relation).await
    }

    async fn get_related(
        &self,
        memory_id: &str,
        relation_type: Option<agent_core::memory::MemoryRelationType>,
    ) -> Result<Vec<agent_core::memory::MemoryRelation>> {
        self.inner.get_related(memory_id, relation_type).await
    }

    async fn store_with_relations(
        &self,
        entry: MemoryEntry,
        relations: Vec<agent_core::memory::MemoryRelation>,
    ) -> Result<()> {
        self.metrics.record_store();
        self.inner.store_with_relations(entry, relations).await
    }

    async fn get_summary_chain(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.inner.get_summary_chain(session_id, limit).await
    }

    async fn update_quality(&self, id: &str, quality: f32, source: &str) -> Result<()> {
        self.inner.update_quality(id, quality, source).await
    }

    async fn clear(&self) -> Result<usize> {
        self.inner.clear().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_metrics_recording() {
        let metrics = MemoryMetrics::new();

        metrics.record_retrieval(Duration::from_millis(10));
        metrics.record_retrieval(Duration::from_millis(20));
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(metrics.retrieval_count.load(Ordering::Relaxed), 2);
        assert!(metrics.avg_retrieval_latency_ms() > 0.0);
        assert_eq!(metrics.cache_hit_rate(), 0.5);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = MemoryMetrics::new();
        metrics.record_retrieval(Duration::from_millis(10));
        metrics.record_cache_hit();

        metrics.reset();

        assert_eq!(metrics.retrieval_count.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_working_tokens() {
        let metrics = MemoryMetrics::new();
        metrics.set_working_tokens(1500);
        assert_eq!(metrics.working_tokens.load(Ordering::Relaxed), 1500);
    }
}
