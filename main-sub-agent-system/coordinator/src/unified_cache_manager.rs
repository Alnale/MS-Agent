use std::sync::Arc;

use dashmap::DashMap;

use agent_teams_core::agent_memory_cache::AgentMemoryCache;
use agent_teams_core::memory_store::MemoryStore;
use agent_teams_core::unified_memory_bus::{SharedMemoryCache, UnifiedMemoryBus};

use crate::plan_cache::PlanCache;
use crate::sub_agent_cache::SubAgentCache;

/// Unified cache manager: coordinates PlanCache, SubAgentCache, and AgentMemoryCache
///
/// This is the coordinator-level cache orchestrator that ensures all agents
/// share a consistent cache infrastructure. It works alongside `UnifiedMemoryBus`
/// (which handles cross-agent memory sharing) by managing the result-level
/// and plan-level caches.
pub struct UnifiedCacheManager {
    /// Global PlanCache
    plan_cache: PlanCache,
    /// Per-agent SubAgentCache (keyed by agent_id)
    sub_agent_caches: DashMap<String, Arc<SubAgentCache>>,
    /// Shared memory cache (cross-agent readable)
    shared_cache: Arc<SharedMemoryCache>,
    /// Global persistent store
    global_store: Option<Arc<dyn MemoryStore>>,
    /// Unified memory bus for cross-agent coordination
    memory_bus: Arc<UnifiedMemoryBus>,
}

impl UnifiedCacheManager {
    pub fn new(
        plan_cache: PlanCache,
        shared_cache: Arc<SharedMemoryCache>,
        global_store: Option<Arc<dyn MemoryStore>>,
        memory_bus: Arc<UnifiedMemoryBus>,
    ) -> Self {
        Self {
            plan_cache,
            sub_agent_caches: DashMap::new(),
            shared_cache,
            global_store,
            memory_bus,
        }
    }

    /// Create or get an AgentMemoryCache for the specified agent.
    /// The cache is wired with shared_cache (L3) and global_store (L4).
    pub fn get_or_create_agent_cache(&self, agent_id: &str, capacity: usize) -> AgentMemoryCache {
        let mut cache = AgentMemoryCache::new(agent_id.to_string(), capacity)
            .with_shared_cache(Arc::clone(&self.shared_cache));
        if let Some(ref store) = self.global_store {
            cache = cache.with_global_store(store.clone());
        }
        cache
    }

    /// Create or get a SubAgentCache for the specified agent.
    pub fn get_or_create_sub_agent_cache(
        &self,
        agent_id: &str,
        capacity: usize,
        ttl_secs: u64,
    ) -> Arc<SubAgentCache> {
        self.sub_agent_caches
            .entry(agent_id.to_string())
            .or_insert_with(|| Arc::new(SubAgentCache::new(capacity, ttl_secs)))
            .clone()
    }

    /// Invalidate caches related to a specific agent and content change.
    /// When an agent's memory changes, dependent plan caches and sub-agent
    /// result caches should be invalidated.
    pub async fn invalidate_related(&self, agent_id: &str, content_hash: &str) {
        // Invalidate plan cache entries that depend on this agent's tags
        self.plan_cache.invalidate_by_tag(agent_id).await;

        // Invalidate the agent's sub-agent result cache
        if let Some(cache) = self.sub_agent_caches.get(agent_id) {
            cache.clear().await;
        }

        tracing::debug!(
            "Invalidated caches for agent={}, content_hash={}",
            agent_id,
            content_hash
        );
    }

    /// Get reference to the plan cache
    pub fn plan_cache(&self) -> &PlanCache {
        &self.plan_cache
    }

    /// Get reference to the unified memory bus
    pub fn memory_bus(&self) -> &Arc<UnifiedMemoryBus> {
        &self.memory_bus
    }

    /// Get reference to the shared memory cache
    pub fn shared_cache(&self) -> &Arc<SharedMemoryCache> {
        &self.shared_cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_cache_manager_create_agent_cache() {
        let plan_cache = PlanCache::new(100, 300);
        let shared_cache = Arc::new(SharedMemoryCache::new(1000));
        let bus = Arc::new(UnifiedMemoryBus::new(1000));
        let mgr = UnifiedCacheManager::new(plan_cache, shared_cache, None, bus);

        let cache = mgr.get_or_create_agent_cache("knowledge", 100);
        assert_eq!(cache.agent_id(), "knowledge");
    }

    #[test]
    fn test_unified_cache_manager_create_sub_agent_cache() {
        let plan_cache = PlanCache::new(100, 300);
        let shared_cache = Arc::new(SharedMemoryCache::new(1000));
        let bus = Arc::new(UnifiedMemoryBus::new(1000));
        let mgr = UnifiedCacheManager::new(plan_cache, shared_cache, None, bus);

        let cache1 = mgr.get_or_create_sub_agent_cache("knowledge", 100, 300);
        let cache2 = mgr.get_or_create_sub_agent_cache("knowledge", 100, 300);
        // Same instance returned for same agent_id
        assert!(Arc::ptr_eq(&cache1, &cache2));
    }

    #[test]
    fn test_unified_cache_manager_different_agents_get_different_caches() {
        let plan_cache = PlanCache::new(100, 300);
        let shared_cache = Arc::new(SharedMemoryCache::new(1000));
        let bus = Arc::new(UnifiedMemoryBus::new(1000));
        let mgr = UnifiedCacheManager::new(plan_cache, shared_cache, None, bus);

        let cache1 = mgr.get_or_create_sub_agent_cache("knowledge", 100, 300);
        let cache2 = mgr.get_or_create_sub_agent_cache("sentiment", 100, 120);
        assert!(!Arc::ptr_eq(&cache1, &cache2));
    }

    #[tokio::test]
    async fn test_unified_cache_cross_agent_sync() {
        use agent_teams_core::memory::{MemoryEntry, MemoryKind, MemoryQuery};

        let plan_cache = PlanCache::new(100, 300);
        let shared_cache = Arc::new(SharedMemoryCache::new(1000));
        let bus = Arc::new(UnifiedMemoryBus::new(1000));
        let mgr = UnifiedCacheManager::new(plan_cache, shared_cache.clone(), None, bus);

        // Create caches for two agents via the manager
        let knowledge_cache = mgr.get_or_create_agent_cache("knowledge", 100);

        // Knowledge agent stores a memory in its local cache
        let entry = MemoryEntry {
            id: "test_memory_1".to_string(),
            session_id: Some("session1".to_string()),
            kind: MemoryKind::UserFact,
            content: "User prefers dark mode".to_string(),
            data: None,
            embedding: None,
            weight: 0.8,
            created_at: chrono::Utc::now(),
            last_accessed_at: chrono::Utc::now(),
            access_count: 0,
            tags: vec!["preference".to_string()],
            source_agent: "knowledge".to_string(),
            confirmed: true,
            content_hash: None,
            confidence: 0.9,
            parent_id: None,
            version: 1,
            archived: false,
            compressed_from: vec![],
        };
        knowledge_cache.store(entry.clone()).await;

        // Propagate to shared cache (simulates event bus propagation)
        shared_cache.put(entry).await;

        // The shared cache should now have the entry
        let query = MemoryQuery {
            kinds: vec![MemoryKind::UserFact],
            limit: 10,
            ..Default::default()
        };
        let shared_results = shared_cache.query(&query).await;
        assert!(
            shared_results.is_some(),
            "Cross-agent memory sync failed: shared cache should have the entry"
        );
        let results = shared_results.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "User prefers dark mode");
        assert_eq!(results[0].source_agent, "knowledge");
    }
}
