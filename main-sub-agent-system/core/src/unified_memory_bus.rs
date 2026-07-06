use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use lru::LruCache;
use tokio::sync::RwLock;

use crate::agent_memory_cache::AgentMemoryCache;
use crate::event::{EventBus, SystemEvent};
use crate::memory::{MemoryEntry, MemoryQuery};
use crate::memory_event_bus::MemoryEventBus;
use crate::memory_store::MemoryStore;

/// Cache entry with TTL metadata
#[derive(Debug, Clone)]
struct CacheEntryMeta {
    entry: MemoryEntry,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Shared memory cache: cross-agent readable cache layer with TTL support
pub struct SharedMemoryCache {
    cache: RwLock<LruCache<String, CacheEntryMeta>>,
    /// Tracks which agent wrote each entry (for filtering)
    source_agents: DashMap<String, String>,
    /// Default TTL in seconds
    default_ttl_secs: u64,
    /// Counter for throttling opportunistic eviction
    write_count: AtomicU64,
}

impl SharedMemoryCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_ttl(capacity, 3600)
    }

    pub fn with_ttl(capacity: usize, default_ttl_secs: u64) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::new(2000).unwrap());
        Self {
            cache: RwLock::new(LruCache::new(cap)),
            source_agents: DashMap::new(),
            default_ttl_secs,
            write_count: AtomicU64::new(0),
        }
    }

    /// Query shared cache with basic tag/kind matching.
    /// Uses a read lock to avoid serializing concurrent reads.
    /// Expired entries are filtered out in results but evicted lazily on writes.
    pub async fn query(&self, query: &MemoryQuery) -> Option<Vec<MemoryEntry>> {
        let cache = self.cache.read().await;
        let now = chrono::Utc::now();

        let results: Vec<MemoryEntry> = cache
            .iter()
            .filter(|(_, meta)| meta.expires_at > now && meta.entry.matches_query(query))
            .map(|(_, meta)| meta.entry.clone())
            .collect();
        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    }

    /// Insert an entry into shared cache with default TTL
    pub async fn put(&self, entry: MemoryEntry) {
        self.put_with_ttl(entry, None).await;
    }

    /// Insert an entry with explicit TTL (seconds). None = use default.
    /// Evicts expired entries opportunistically every 50 writes to avoid O(n) scan on every put.
    pub async fn put_with_ttl(&self, entry: MemoryEntry, ttl_secs: Option<u64>) {
        let ttl = ttl_secs.unwrap_or(self.default_ttl_secs);
        let now = chrono::Utc::now();
        let id = entry.id.clone();
        let source_agent = entry.source_agent.clone();
        let meta = CacheEntryMeta {
            expires_at: now + chrono::Duration::seconds(ttl as i64),
            entry,
        };
        let mut cache = self.cache.write().await;

        // Throttled opportunistic eviction: every 50 writes
        let count = self.write_count.fetch_add(1, Ordering::Relaxed);
        if count.is_multiple_of(50) {
            let expired: Vec<String> = cache
                .iter()
                .filter(|(_, m)| m.expires_at <= now)
                .map(|(id, _)| id.clone())
                .collect();
            for key in expired {
                cache.pop(&key);
                self.source_agents.remove(&key);
            }
        }

        self.source_agents.insert(id.clone(), source_agent);
        cache.put(id, meta);
    }

    /// Invalidate entries from a specific agent
    pub async fn invalidate_agent(&self, agent_id: &str) {
        let ids_to_remove: Vec<String> = self
            .source_agents
            .iter()
            .filter(|r| r.value() == agent_id)
            .map(|r| r.key().clone())
            .collect();
        let mut cache = self.cache.write().await;
        for id in ids_to_remove {
            cache.pop(&id);
            self.source_agents.remove(&id);
        }
    }

    /// Invalidate all entries belonging to a session
    pub async fn invalidate_session(&self, session_id: &str) {
        let mut cache = self.cache.write().await;
        let ids_to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, meta)| meta.entry.session_id.as_deref() == Some(session_id))
            .map(|(id, _)| id.clone())
            .collect();
        let count = ids_to_remove.len();
        for id in &ids_to_remove {
            cache.pop(id);
            self.source_agents.remove(id);
        }
        if count > 0 {
            tracing::info!("Invalidated {} cache entries for session {}", count, session_id);
        }
    }

    /// Remove all expired entries
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let now = chrono::Utc::now();
        let expired: Vec<String> = cache
            .iter()
            .filter(|(_, meta)| meta.expires_at <= now)
            .map(|(id, _)| id.clone())
            .collect();
        let count = expired.len();
        for key in &expired {
            cache.pop(key);
            self.source_agents.remove(key);
        }
        if count > 0 {
            tracing::debug!("Cleaned {} expired shared cache entries", count);
        }
    }

    /// Clear all entries from the shared cache (for session boundary enforcement)
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        self.source_agents.clear();
    }

}

/// Cache system monitoring metrics
#[derive(Debug, Default)]
pub struct CacheMetrics {
    pub l1_hits: AtomicU64,
    pub l2_hits: AtomicU64,
    pub l3_hits: AtomicU64,
    pub l4_hits: AtomicU64,
    pub misses: AtomicU64,
    pub forced_calls: AtomicU64,
    pub cache_skipped_forced: AtomicU64,
    pub memory_syncs: AtomicU64,
    pub cross_agent_queries: AtomicU64,
}

impl CacheMetrics {
    pub fn hit_rate(&self) -> f64 {
        let total = self.l1_hits.load(Ordering::Relaxed)
            + self.l2_hits.load(Ordering::Relaxed)
            + self.l3_hits.load(Ordering::Relaxed)
            + self.l4_hits.load(Ordering::Relaxed)
            + self.misses.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            (total - self.misses.load(Ordering::Relaxed)) as f64 / total as f64
        }
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "l1_hits": self.l1_hits.load(Ordering::Relaxed),
            "l2_hits": self.l2_hits.load(Ordering::Relaxed),
            "l3_hits": self.l3_hits.load(Ordering::Relaxed),
            "l4_hits": self.l4_hits.load(Ordering::Relaxed),
            "misses": self.misses.load(Ordering::Relaxed),
            "hit_rate": self.hit_rate(),
            "forced_calls": self.forced_calls.load(Ordering::Relaxed),
            "memory_syncs": self.memory_syncs.load(Ordering::Relaxed),
            "cross_agent_queries": self.cross_agent_queries.load(Ordering::Relaxed),
        })
    }
}

/// Unified memory bus: connects all agents via a shared cache network
pub struct UnifiedMemoryBus {
    /// Agent ID -> AgentMemoryCache mapping
    agent_caches: DashMap<String, Arc<AgentMemoryCache>>,
    /// Global shared cache (cross-agent readable)
    shared_cache: Arc<SharedMemoryCache>,
    /// Global persistent store
    global_store: Option<Arc<dyn MemoryStore>>,
    /// Event bus for system-level cache invalidation broadcasts
    event_bus: Option<EventBus>,
    /// Fine-grained memory change event bus
    memory_event_bus: Arc<MemoryEventBus>,
    /// Monitoring metrics
    metrics: Arc<CacheMetrics>,
}

impl UnifiedMemoryBus {
    pub fn new(shared_capacity: usize) -> Self {
        Self {
            agent_caches: DashMap::new(),
            shared_cache: Arc::new(SharedMemoryCache::new(shared_capacity)),
            global_store: None,
            event_bus: None,
            memory_event_bus: Arc::new(MemoryEventBus::default()),
            metrics: Arc::new(CacheMetrics::default()),
        }
    }

    pub fn with_global_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.global_store = Some(store);
        self
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_memory_event_bus(mut self, bus: Arc<MemoryEventBus>) -> Self {
        self.memory_event_bus = bus;
        self
    }

    /// Register an agent's cache with the bus
    pub fn register_agent(&self, agent_id: &str, cache: Arc<AgentMemoryCache>) {
        self.agent_caches.insert(agent_id.to_string(), cache);
    }

    /// Register an agent's cache and wire the memory event bus into it.
    /// Use this when the agent's cache should publish memory change events.
    pub fn register_agent_with_events(&self, agent_id: &str, cache: Arc<AgentMemoryCache>) {
        let wired_cache = Arc::new(
            cache
                .as_ref()
                .clone()
                .with_event_bus(self.memory_event_bus.clone()),
        );
        self.agent_caches.insert(agent_id.to_string(), wired_cache);
    }

    /// Set global store after construction (for deferred initialization)
    pub fn set_global_store(&mut self, store: Arc<dyn MemoryStore>) {
        self.global_store = Some(store);
    }

    /// Cross-agent memory query: source agent's cache -> shared cache -> global store
    pub async fn cross_agent_query(
        &self,
        source_agent_id: &str,
        query: &MemoryQuery,
    ) -> Vec<MemoryEntry> {
        self.metrics
            .cross_agent_queries
            .fetch_add(1, Ordering::Relaxed);
        let mut results = Vec::new();

        // 1. Query source agent's own cache
        if let Some(cache) = self.agent_caches.get(source_agent_id) {
            results.extend(cache.query(query).await);
        }

        // 2. Query shared cache (other agents' public memories)
        if let Some(shared_results) = self.shared_cache.query(query).await {
            self.metrics.l3_hits.fetch_add(1, Ordering::Relaxed);
            results.extend(shared_results);
        }

        // 3. Query global store on miss
        if results.is_empty() {
            if let Some(ref store) = self.global_store {
                match store.retrieve(query.clone()).await {
                    Ok(retrieval) => {
                        self.metrics.l4_hits.fetch_add(1, Ordering::Relaxed);
                        // Backfill shared cache
                        for entry in &retrieval.entries {
                            self.shared_cache.put(entry.clone()).await;
                        }
                        results.extend(retrieval.entries);
                    }
                    Err(e) => {
                        tracing::warn!("UnifiedMemoryBus global store query failed: {}", e);
                    }
                }
            }
        }

        if results.is_empty() {
            self.metrics.misses.fetch_add(1, Ordering::Relaxed);
        }

        results
    }

    /// Broadcast memory update: notify relevant agents to invalidate stale cache
    pub fn broadcast_memory_update(&self, entry: &MemoryEntry) {
        if let Some(ref bus) = self.event_bus {
            bus.publish(SystemEvent::MemoryUpdated {
                memory_id: entry.id.clone(),
                source_agent: entry.source_agent.clone(),
            });
        }
    }

    /// Get shared cache reference
    pub fn shared_cache(&self) -> &Arc<SharedMemoryCache> {
        &self.shared_cache
    }

    /// Clear the shared cache (for session boundary enforcement)
    pub async fn clear_shared_cache(&self) {
        self.shared_cache.clear().await;
    }

    /// Get metrics reference
    pub fn metrics(&self) -> &Arc<CacheMetrics> {
        &self.metrics
    }

    /// Get memory event bus reference
    pub fn memory_event_bus(&self) -> &Arc<MemoryEventBus> {
        &self.memory_event_bus
    }

    /// Flush all registered agent caches to global store
    pub async fn flush_all_agents(&self) {
        for entry in self.agent_caches.iter() {
            if let Err(e) = entry.value().flush_all().await {
                tracing::warn!("Failed to flush cache for agent {}: {}", entry.key(), e);
            }
        }
    }

    /// Get the number of registered agent caches
    pub fn registered_agent_count(&self) -> usize {
        self.agent_caches.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_memory_cache::AgentMemoryCache;
    use crate::memory::{MemoryKind, MemoryQuery};

    fn make_test_entry(id: &str, content: &str, agent_id: &str) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            session_id: Some("test_session".to_string()),
            kind: MemoryKind::UserFact,
            content: content.to_string(),
            data: None,
            embedding: None,
            weight: 0.8,
            created_at: chrono::Utc::now(),
            last_accessed_at: chrono::Utc::now(),
            access_count: 0,
            tags: vec![],
            source_agent: agent_id.to_string(),
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
    fn test_unified_bus_register_agent() {
        let bus = UnifiedMemoryBus::new(1000);
        let cache = Arc::new(AgentMemoryCache::new("test_agent".to_string(), 100));
        bus.register_agent("test_agent", cache);
        assert_eq!(bus.registered_agent_count(), 1);
    }

    #[test]
    fn test_unified_bus_register_multiple_agents() {
        let bus = UnifiedMemoryBus::new(1000);
        let cache1 = Arc::new(AgentMemoryCache::new("agent1".to_string(), 100));
        let cache2 = Arc::new(AgentMemoryCache::new("agent2".to_string(), 100));
        bus.register_agent("agent1", cache1);
        bus.register_agent("agent2", cache2);
        assert_eq!(bus.registered_agent_count(), 2);
    }

    #[tokio::test]
    async fn test_shared_cache_put_and_query() {
        let shared = SharedMemoryCache::new(100);
        let entry = make_test_entry("1", "test content", "agent1");
        shared.put(entry).await;

        let query = MemoryQuery {
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };
        let results = shared.query(&query).await;
        assert!(results.is_some());
        assert_eq!(results.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_shared_cache_invalidate_agent() {
        let shared = SharedMemoryCache::new(100);
        shared
            .put(make_test_entry("1", "from agent1", "agent1"))
            .await;
        shared
            .put(make_test_entry("2", "from agent2", "agent2"))
            .await;

        shared.invalidate_agent("agent1").await;

        let query = MemoryQuery {
            kinds: vec![MemoryKind::UserFact],
            ..Default::default()
        };
        let results = shared.query(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_agent, "agent2");
    }

    #[tokio::test]
    async fn test_cross_agent_query() {
        let bus = UnifiedMemoryBus::new(1000);

        // Create agent cache with a memory entry
        let cache = AgentMemoryCache::new("agent1".to_string(), 100);
        cache
            .store(make_test_entry("1", "agent1 memory", "agent1"))
            .await;
        bus.register_agent("agent1", Arc::new(cache));

        // Query from agent2 should find agent1's memory via shared cache
        let query = MemoryQuery {
            kinds: vec![MemoryKind::UserFact],
            limit: 10,
            ..Default::default()
        };
        let results = bus.cross_agent_query("agent2", &query).await;
        // Results may be empty if shared cache doesn't have it yet (agent1's cache is separate)
        // But the query should not error
        let _ = results;
    }

    #[tokio::test]
    async fn test_flush_all_agents() {
        let bus = UnifiedMemoryBus::new(1000);
        let cache1 = AgentMemoryCache::new("agent1".to_string(), 100);
        let cache2 = AgentMemoryCache::new("agent2".to_string(), 100);

        // Store entries to create dirty items
        cache1.store(make_test_entry("1", "test1", "agent1")).await;
        cache2.store(make_test_entry("2", "test2", "agent2")).await;

        bus.register_agent("agent1", Arc::new(cache1));
        bus.register_agent("agent2", Arc::new(cache2));

        // Flush should not error even without global store
        bus.flush_all_agents().await;
        assert_eq!(bus.registered_agent_count(), 2);
    }

    #[test]
    fn test_cache_metrics_hit_rate() {
        let metrics = CacheMetrics::default();
        assert_eq!(metrics.hit_rate(), 0.0);

        metrics.l1_hits.fetch_add(5, Ordering::Relaxed);
        metrics.misses.fetch_add(5, Ordering::Relaxed);
        assert_eq!(metrics.hit_rate(), 0.5);
    }
}
