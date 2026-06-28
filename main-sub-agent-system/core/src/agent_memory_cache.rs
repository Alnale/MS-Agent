use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use lru::LruCache;

use crate::error::Result;
use crate::memory::{MemoryEntry, MemoryQuery};
use crate::memory_event_bus::{MemoryChangeEvent, MemoryEventBus};
use crate::memory_store::MemoryStore;
use crate::unified_memory_bus::SharedMemoryCache;

/// Cache operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// Normal read-write caching
    #[default]
    ReadWrite,
    /// Write-only (for forced refresh scenarios)
    WriteThrough,
    /// Bypass cache entirely (for debugging)
    Bypass,
}

/// Cache hit/miss statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hot_hits: AtomicU64,
    pub warm_hits: AtomicU64,
    pub shared_hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hot_hits.load(Ordering::Relaxed)
            + self.warm_hits.load(Ordering::Relaxed)
            + self.misses.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let hits = self.hot_hits.load(Ordering::Relaxed) + self.warm_hits.load(Ordering::Relaxed);
        hits as f64 / total as f64
    }
}

/// Agent-level multi-tier memory cache
///
/// Each agent gets its own independent cache with:
/// - L1 (hot): Lock-free DashMap for current session context
/// - L2 (warm): LRU cache for cross-session frequently used memories
/// - L3 (global): Backing MemoryStore for cache misses
pub struct AgentMemoryCache {
    agent_id: String,
    /// L1: Hot cache (session-scoped, lock-free)
    hot_cache: DashMap<String, MemoryEntry>,
    /// L1 max capacity (0 = unlimited)
    hot_max_size: usize,
    /// L2: Warm cache (cross-session, LRU) — Arc-wrapped for Clone support
    warm_cache: Arc<Mutex<LruCache<String, MemoryEntry>>>,
    /// L3: Shared cache (cross-agent, read-through)
    shared_cache: Option<Arc<SharedMemoryCache>>,
    /// L4: Global store fallback
    global_store: Option<Arc<dyn MemoryStore>>,
    /// Cache operation mode
    cache_mode: CacheMode,
    /// Statistics
    stats: Arc<CacheStats>,
    /// Dirty entries that need syncing to global store
    dirty: DashMap<String, MemoryEntry>,
    /// Event bus for publishing memory change events (optional)
    event_bus: Option<Arc<MemoryEventBus>>,
}

impl Clone for AgentMemoryCache {
    fn clone(&self) -> Self {
        Self {
            agent_id: self.agent_id.clone(),
            hot_cache: self.hot_cache.clone(),
            hot_max_size: self.hot_max_size,
            warm_cache: Arc::clone(&self.warm_cache),
            shared_cache: self.shared_cache.clone(),
            global_store: self.global_store.clone(),
            cache_mode: self.cache_mode,
            stats: Arc::clone(&self.stats),
            dirty: self.dirty.clone(),
            event_bus: self.event_bus.clone(),
        }
    }
}

impl AgentMemoryCache {
    pub fn new(agent_id: String, capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::new(100).unwrap());

        Self {
            agent_id,
            hot_cache: DashMap::new(),
            hot_max_size: 500, // Default L1 limit
            warm_cache: Arc::new(Mutex::new(LruCache::new(cap))),
            shared_cache: None,
            global_store: None,
            cache_mode: CacheMode::default(),
            stats: Arc::new(CacheStats::default()),
            dirty: DashMap::new(),
            event_bus: None,
        }
    }

    /// Set L1 hot cache max size (0 = unlimited)
    pub fn with_hot_max_size(mut self, max_size: usize) -> Self {
        self.hot_max_size = max_size;
        self
    }

    pub fn with_global_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.global_store = Some(store);
        self
    }

    pub fn with_shared_cache(mut self, shared: Arc<SharedMemoryCache>) -> Self {
        self.shared_cache = Some(shared);
        self
    }

    pub fn with_cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    pub fn with_event_bus(mut self, bus: Arc<MemoryEventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Evict least-accessed entries from L1 hot cache if it exceeds max size.
    /// Uses LFU heuristic: evicts non-dirty entries with lowest access_count first.
    fn evict_hot_if_needed(&self) {
        if self.hot_max_size == 0 {
            return; // Unlimited
        }
        let current_size = self.hot_cache.len();
        if current_size <= self.hot_max_size {
            return;
        }
        let to_evict = current_size - self.hot_max_size + (self.hot_max_size / 10); // Evict 10% extra for headroom

        // Collect non-dirty entries as eviction candidates, sorted by access_count (ascending)
        let mut candidates: Vec<(String, u32)> = self.hot_cache
            .iter()
            .filter(|e| !self.dirty.contains_key(e.key()))
            .map(|e| (e.key().clone(), e.value().access_count))
            .collect();
        candidates.sort_by_key(|(_, count)| *count);

        let mut evicted = 0;
        for (key, _) in candidates.into_iter().take(to_evict) {
            if self.hot_cache.remove(&key).is_some() {
                evicted += 1;
            }
        }

        if evicted > 0 {
            self.stats.evictions.fetch_add(evicted as u64, Ordering::Relaxed);
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Multi-tier query: L1 (hot) -> L2 (warm) -> L3 (shared) -> L4 (global)
    pub async fn query(&self, query: &MemoryQuery) -> Vec<MemoryEntry> {
        if self.cache_mode == CacheMode::Bypass {
            return self.query_global(query).await;
        }

        // L1: Hot cache (session-scoped)
        let l1_results = self.query_l1(query);
        if !l1_results.is_empty() {
            self.stats.hot_hits.fetch_add(1, Ordering::Relaxed);
            return l1_results;
        }

        // L2: Warm cache (cross-session, LRU)
        if let Some(l2_results) = self.query_l2(query).await {
            self.stats.warm_hits.fetch_add(1, Ordering::Relaxed);
            // Backfill L1
            for entry in &l2_results {
                self.hot_cache.insert(entry.id.clone(), entry.clone());
            }
            return l2_results;
        }

        // L3: Shared cache (cross-agent)
        if let Some(ref shared) = self.shared_cache {
            if let Some(l3_results) = shared.query(query).await {
                // Backfill L1 and L2
                for entry in &l3_results {
                    self.hot_cache.insert(entry.id.clone(), entry.clone());
                    let mut warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
                    warm.put(entry.id.clone(), entry.clone());
                }
                return l3_results;
            }
        }

        // L4: Global store
        self.query_global(query).await
    }

    fn query_l1(&self, query: &MemoryQuery) -> Vec<MemoryEntry> {
        if let Some(ref sid) = query.session_id {
            self.hot_cache
                .iter()
                .filter(|e| {
                    e.value().session_id.as_ref() == Some(sid)
                        && Self::matches_query(e.value(), query)
                })
                .map(|e| e.value().clone())
                .collect()
        } else {
            self.hot_cache
                .iter()
                .filter(|e| Self::matches_query(e.value(), query))
                .map(|e| e.value().clone())
                .collect()
        }
    }

    async fn query_l2(&self, query: &MemoryQuery) -> Option<Vec<MemoryEntry>> {
        let warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
        let results: Vec<MemoryEntry> = warm
            .iter()
            .filter(|(_, entry)| Self::matches_query(entry, query))
            .map(|(_, entry)| entry.clone())
            .collect();
        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    }

    async fn query_global(&self, query: &MemoryQuery) -> Vec<MemoryEntry> {
        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        if let Some(ref store) = self.global_store {
            match store.retrieve(query.clone()).await {
                Ok(retrieval) => {
                    // Backfill all layers — acquire warm lock once
                    {
                        let mut warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
                        for entry in &retrieval.entries {
                            self.hot_cache.insert(entry.id.clone(), entry.clone());
                            warm.put(entry.id.clone(), entry.clone());
                        }
                    }
                    if let Some(ref shared) = self.shared_cache {
                        for entry in &retrieval.entries {
                            shared.put(entry.clone()).await;
                        }
                    }
                    retrieval.entries
                }
                Err(e) => {
                    tracing::warn!(
                        "Global store query failed for agent {}: {}",
                        self.agent_id,
                        e
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Store memory (write-through: L1 -> L2 -> async L3)
    pub async fn store(&self, entry: MemoryEntry) {
        // Write L1
        self.hot_cache.insert(entry.id.clone(), entry.clone());
        self.evict_hot_if_needed();

        // Write L2
        {
            let mut warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
            warm.put(entry.id.clone(), entry.clone());
        }

        // Mark dirty for async global sync
        self.dirty.insert(entry.id.clone(), entry.clone());

        // Publish memory change event
        if let Some(ref bus) = self.event_bus {
            bus.publish(MemoryChangeEvent::Stored {
                agent_id: self.agent_id.clone(),
                session_id: entry.session_id.clone(),
                memory_kind: entry.kind.clone(),
                tags: entry.tags.clone(),
                content_hash: entry.content_hash.clone().unwrap_or_default(),
            });
        }

        // Async write to L3
        self.flush_to_global(entry).await;
    }

    /// Batch flush all dirty entries to global store
    pub async fn flush_all(&self) -> Result<usize> {
        let dirty_entries: Vec<MemoryEntry> =
            self.dirty.iter().map(|e| e.value().clone()).collect();

        let count = dirty_entries.len();

        if let Some(ref store) = self.global_store {
            if let Err(e) = store.store_batch(dirty_entries.clone()).await {
                tracing::warn!(
                    "Batch flush failed for agent {}, falling back to individual: {}",
                    self.agent_id,
                    e
                );
                // Fallback to individual stores
                for entry in &dirty_entries {
                    if let Err(e) = store.store(entry.clone()).await {
                        tracing::warn!(
                            "Failed to flush memory {} for agent {}: {}",
                            entry.id,
                            self.agent_id,
                            e
                        );
                    }
                }
            }
            // Clear all dirty entries on success
            for entry in &dirty_entries {
                self.dirty.remove(&entry.id);
            }
        }

        Ok(count)
    }

    /// Preload memories into hot cache (for session initialization)
    pub async fn preload(&self, entries: Vec<MemoryEntry>) {
        for entry in entries {
            self.hot_cache.insert(entry.id.clone(), entry);
        }
    }

    /// Clear hot cache (session end)
    pub fn clear_hot(&self) {
        self.hot_cache.clear();
    }

    /// Clear all cache layers for a specific session
    pub async fn clear_session(&self, session_id: &str) {
        // L1 hot cache
        self.hot_cache.retain(|_, entry| {
            entry.session_id.as_deref() != Some(session_id)
        });

        // L2 warm cache
        let mut warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
        let to_remove: Vec<String> = warm
            .iter()
            .filter(|(_, entry)| entry.session_id.as_deref() == Some(session_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_remove {
            warm.pop(&id);
        }

        // Dirty entries
        self.dirty.retain(|_, entry| {
            entry.session_id.as_deref() != Some(session_id)
        });

        tracing::debug!("Cleared session {} from agent {} cache", session_id, self.agent_id);
    }

    /// Remove entries older than the given duration from L1 hot cache
    pub async fn cleanup_expired(&self) {
        let now = chrono::Utc::now();
        let threshold = chrono::Duration::hours(1);

        self.hot_cache.retain(|_, entry| {
            entry.last_accessed_at + threshold > now
        });

        // L2 warm cache: evict entries not accessed in 24h
        let warm_threshold = chrono::Duration::hours(24);
        let mut warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
        let to_remove: Vec<String> = warm
            .iter()
            .filter(|(_, entry)| entry.last_accessed_at + warm_threshold <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_remove {
            warm.pop(&id);
        }
    }

    /// Store with optimistic version check. Rejects if existing version is newer.
    pub async fn store_with_version(&self, entry: MemoryEntry) -> Result<()> {
        // Check L1
        if let Some(existing) = self.hot_cache.get(&entry.id) {
            if existing.version > entry.version {
                return Err(crate::error::AgentTeamsError::StateVersionConflict {
                    expected: entry.version as u64,
                    actual: existing.version as u64,
                });
            }
        }

        // Check L2
        {
            let warm = self.warm_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = warm.peek(&entry.id) {
                if existing.version > entry.version {
                    return Err(crate::error::AgentTeamsError::StateVersionConflict {
                        expected: entry.version as u64,
                        actual: existing.version as u64,
                    });
                }
            }
        }

        self.store(entry).await;
        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> Arc<CacheStats> {
        self.stats.clone()
    }

    async fn flush_to_global(&self, entry: MemoryEntry) {
        if let Some(ref store) = self.global_store {
            let store = store.clone();
            let id = entry.id.clone();
            let agent_id = self.agent_id.clone();
            let dirty = self.dirty.clone();
            tokio::spawn(async move {
                if let Err(e) = store.store(entry).await {
                    tracing::warn!("Async flush failed for {} (agent {}): {}", id, agent_id, e);
                } else {
                    dirty.remove(&id);
                }
            });
        }
    }

    fn matches_query(entry: &MemoryEntry, query: &MemoryQuery) -> bool {
        if !query.kinds.is_empty() && !query.kinds.contains(&entry.kind) {
            return false;
        }
        if !query.tags.is_empty() && !query.tags.iter().any(|t| entry.tags.contains(t)) {
            return false;
        }
        if entry.weight < query.min_weight {
            return false;
        }
        if let Some(ref sid) = query.session_id {
            if entry.session_id.as_ref() != Some(sid) {
                return false;
            }
        }
        if query.confirmed_only && !entry.confirmed {
            return false;
        }
        true
    }
}

/// Execution policy for controlling SubAgent invocation behavior
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    /// Force SubAgent calls (never skip)
    pub force_sub_agent: bool,
    /// Allow caching complete responses (false = only cache memory, not responses)
    pub allow_response_cache: bool,
    /// Allow caching execution plans
    pub allow_plan_cache: bool,
    /// Minimum number of SubAgent calls required
    pub min_sub_agent_calls: usize,
    /// Cache operation mode
    pub cache_mode: CacheMode,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            force_sub_agent: true,
            allow_response_cache: false,
            allow_plan_cache: true,
            min_sub_agent_calls: 1,
            cache_mode: CacheMode::ReadWrite,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryKind, MemoryQuery};

    fn make_test_entry(id: &str, content: &str, kind: MemoryKind) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            session_id: Some("test_session".to_string()),
            kind,
            content: content.to_string(),
            data: None,
            embedding: None,
            weight: 0.8,
            created_at: chrono::Utc::now(),
            last_accessed_at: chrono::Utc::now(),
            access_count: 0,
            tags: vec![],
            source_agent: "test".to_string(),
            confirmed: true,
            content_hash: None,
            confidence: 1.0,
            parent_id: None,
            version: 1,
            archived: false,
            compressed_from: vec![],
        }
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);

        stats.hot_hits.fetch_add(3, Ordering::Relaxed);
        stats.misses.fetch_add(1, Ordering::Relaxed);
        assert_eq!(stats.hit_rate(), 0.75);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_store_and_query() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        let entry = make_test_entry("1", "User prefers dark mode", MemoryKind::UserFact);

        cache.store(entry).await;

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };

        let results = cache.query(&query).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "User prefers dark mode");
    }

    #[tokio::test]
    async fn test_agent_memory_cache_hot_hit() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        let entry = make_test_entry("1", "test", MemoryKind::UserFact);
        cache.store(entry).await;

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };

        cache.query(&query).await;
        assert_eq!(cache.stats().hot_hits.load(Ordering::Relaxed), 1);
        assert_eq!(cache.stats().misses.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_miss() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };

        let results = cache.query(&query).await;
        assert!(results.is_empty());
        assert_eq!(cache.stats().misses.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_preload() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        let entries = vec![
            make_test_entry("1", "fact1", MemoryKind::UserFact),
            make_test_entry("2", "fact2", MemoryKind::UserFact),
        ];
        cache.preload(entries).await;

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };

        let results = cache.query(&query).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_clear_hot() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        cache
            .store(make_test_entry("1", "test", MemoryKind::UserFact))
            .await;

        // Clear L1 hot cache only
        cache.clear_hot();

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };

        // After clearing hot cache, entry is still in L2 warm cache
        let results = cache.query(&query).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "test");
        // L2 warm hit should be recorded (not a hot hit since L1 was cleared)
        assert_eq!(cache.stats().warm_hits.load(Ordering::Relaxed), 1);
        assert_eq!(cache.stats().hot_hits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_execution_policy_default() {
        let policy = ExecutionPolicy::default();
        assert!(policy.force_sub_agent);
        assert!(!policy.allow_response_cache);
        assert!(policy.allow_plan_cache);
        assert_eq!(policy.min_sub_agent_calls, 1);
    }

    #[test]
    fn test_execution_policy_custom() {
        let policy = ExecutionPolicy {
            force_sub_agent: false,
            allow_response_cache: true,
            allow_plan_cache: false,
            min_sub_agent_calls: 0,
            cache_mode: CacheMode::Bypass,
        };
        assert!(!policy.force_sub_agent);
        assert!(policy.allow_response_cache);
        assert_eq!(policy.cache_mode, CacheMode::Bypass);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_tier_consistency() {
        // Verify that storing in L1 (hot) and querying returns consistent results
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        let entry = make_test_entry("1", "consistent data", MemoryKind::UserFact);
        cache.store(entry).await;

        // Query should hit L1 (hot cache)
        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };
        let results = cache.query(&query).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "consistent data");
        assert_eq!(cache.stats().hot_hits.load(Ordering::Relaxed), 1);

        // Clear hot cache, should fall through to L2 (warm)
        cache.clear_hot();
        let results = cache.query(&query).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "consistent data");
        assert_eq!(cache.stats().warm_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_agent_memory_cache_clone_independence() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);
        let entry = make_test_entry("1", "original", MemoryKind::UserFact);
        cache.store(entry).await;

        // Clone should share the same underlying data
        let cloned = cache.clone();
        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };
        let results = cloned.query(&query).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "original");
    }

    #[tokio::test]
    async fn test_agent_memory_cache_multiple_entries() {
        let cache = AgentMemoryCache::new("test_agent".to_string(), 100);

        for i in 0..10 {
            let entry = make_test_entry(
                &i.to_string(),
                &format!("entry_{}", i),
                MemoryKind::UserFact,
            );
            cache.store(entry).await;
        }

        let query = MemoryQuery {
            session_id: Some("test_session".to_string()),
            kinds: vec![MemoryKind::UserFact],
            limit: 20,
            ..Default::default()
        };
        let results = cache.query(&query).await;
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn test_execution_policy_force_sub_agent_must_be_true() {
        // This test documents the invariant: default policy must force SubAgent calls
        let policy = ExecutionPolicy::default();
        assert!(
            policy.force_sub_agent,
            "SECURITY INVARIANT: ExecutionPolicy::default().force_sub_agent must be true. \
             SubAgent calls must never be skippable by default."
        );
        assert!(
            policy.min_sub_agent_calls >= 1,
            "SECURITY INVARIANT: min_sub_agent_calls must be at least 1."
        );
    }
}
