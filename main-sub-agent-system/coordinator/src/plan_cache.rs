use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use lru::LruCache;

use agent_core::plan::ExecutionPlan;

/// Cached plan with expiration time
struct CachedPlan {
    plan: Arc<ExecutionPlan>,
    created_at: Instant,
    ttl: Duration,
}

/// Plan cache with dependency tracking for memory-aware invalidation
///
/// When a plan is stored with memory tags, the cache tracks which tags
/// the plan depends on. When those tags are invalidated, all dependent
/// plans are also evicted.
pub struct PlanCache {
    cache: Mutex<LruCache<String, CachedPlan>>,
    /// dependency tag -> list of plan keys that depend on it
    dependency_index: DashMap<String, Vec<String>>,
}

impl PlanCache {
    pub fn new(capacity: usize, _default_ttl_secs: u64) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::new(500).unwrap());
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            dependency_index: DashMap::new(),
        }
    }

    pub async fn get(&self, key: &str) -> Option<ExecutionPlan> {
        let arc = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = cache.get(key) {
                if cached.created_at.elapsed() < cached.ttl {
                    Some(Arc::clone(&cached.plan))
                } else {
                    cache.pop(key);
                    None
                }
            } else {
                None
            }
        };
        arc.map(|a| (*a).clone())
    }

    pub async fn put(&self, key: String, plan: ExecutionPlan, ttl_secs: u64) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.put(
            key,
            CachedPlan {
                plan: Arc::new(plan),
                created_at: Instant::now(),
                ttl: Duration::from_secs(ttl_secs),
            },
        );
    }

    /// Store a plan with dependency tags for memory-aware invalidation
    pub async fn put_with_dependencies(
        &self,
        key: String,
        plan: ExecutionPlan,
        ttl_secs: u64,
        memory_tags: Vec<String>,
    ) {
        // Store the plan
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.put(
                key.clone(),
                CachedPlan {
                    plan: Arc::new(plan),
                    created_at: Instant::now(),
                    ttl: Duration::from_secs(ttl_secs),
                },
            );
        }

        // Record dependencies
        for tag in memory_tags {
            self.dependency_index
                .entry(tag)
                .or_default()
                .push(key.clone());
        }
    }

    /// Invalidate all plans that depend on the given memory tag
    pub async fn invalidate_by_tag(&self, tag: &str) {
        if let Some((_, keys)) = self.dependency_index.remove(tag) {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            for key in &keys {
                cache.pop(key);
            }
        }
    }

    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.pop(key);
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.clear();
        self.dependency_index.clear();
    }

    pub async fn len(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub async fn is_empty(&self) -> bool {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    /// Remove expired entries
    pub async fn cleanup(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for (key, cached) in cache.iter() {
            if now.saturating_duration_since(cached.created_at) >= cached.ttl {
                to_remove.push(key.clone());
            }
        }

        for key in to_remove {
            cache.pop(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::pipeline::StageMode;
    use agent_core::plan::PlanStage;

    fn make_test_plan() -> ExecutionPlan {
        ExecutionPlan {
            stages: vec![PlanStage {
                name: "test".to_string(),
                sub_agent_ids: vec!["test".to_string()],
                mode: StageMode::Sequential,
                required: false,
                timeout_ms: Some(5000),
                message_override: None,
            }],
            strategy: "test".to_string(),
            estimated_duration_ms: 1000,
            confidence: 0.9,
            nodes: Vec::new(),
            tool_intent: None,
        }
    }

    #[tokio::test]
    async fn test_plan_cache_put_and_get() {
        let cache = PlanCache::new(10, 300);
        cache.put("key1".to_string(), make_test_plan(), 300).await;

        let cached = cache.get("key1").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().stages.len(), 1);
    }

    #[tokio::test]
    async fn test_plan_cache_miss() {
        let cache = PlanCache::new(10, 300);
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_plan_cache_expiration() {
        let cache = PlanCache::new(10, 0); // TTL = 0 seconds
        cache.put("key1".to_string(), make_test_plan(), 0).await;

        // Should be expired immediately
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(cache.get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_plan_cache_invalidate() {
        let cache = PlanCache::new(10, 300);
        cache.put("key1".to_string(), make_test_plan(), 300).await;
        assert!(cache.get("key1").await.is_some());

        cache.invalidate("key1").await;
        assert!(cache.get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_plan_cache_cleanup() {
        let cache = PlanCache::new(10, 0); // TTL = 0 seconds
        cache.put("key1".to_string(), make_test_plan(), 0).await;
        cache.put("key2".to_string(), make_test_plan(), 0).await;

        tokio::time::sleep(Duration::from_millis(10)).await;
        cache.cleanup().await;

        assert_eq!(cache.len().await, 0);
    }

    #[tokio::test]
    async fn test_plan_cache_put_with_dependencies() {
        let cache = PlanCache::new(10, 300);
        cache
            .put_with_dependencies(
                "plan1".to_string(),
                make_test_plan(),
                300,
                vec!["tag_a".to_string(), "tag_b".to_string()],
            )
            .await;

        // Plan should be retrievable
        assert!(cache.get("plan1").await.is_some());
    }

    #[tokio::test]
    async fn test_plan_cache_invalidate_by_tag() {
        let cache = PlanCache::new(10, 300);
        cache
            .put_with_dependencies(
                "plan1".to_string(),
                make_test_plan(),
                300,
                vec!["tag_a".to_string()],
            )
            .await;
        cache
            .put_with_dependencies(
                "plan2".to_string(),
                make_test_plan(),
                300,
                vec!["tag_b".to_string()],
            )
            .await;

        // Invalidating tag_a should remove plan1 but not plan2
        cache.invalidate_by_tag("tag_a").await;
        assert!(cache.get("plan1").await.is_none());
        assert!(cache.get("plan2").await.is_some());
    }

    #[tokio::test]
    async fn test_plan_cache_invalidate_by_shared_tag() {
        let cache = PlanCache::new(10, 300);
        cache
            .put_with_dependencies(
                "plan1".to_string(),
                make_test_plan(),
                300,
                vec!["shared_tag".to_string()],
            )
            .await;
        cache
            .put_with_dependencies(
                "plan2".to_string(),
                make_test_plan(),
                300,
                vec!["shared_tag".to_string(), "other_tag".to_string()],
            )
            .await;

        // Invalidating shared_tag should remove both plans
        cache.invalidate_by_tag("shared_tag").await;
        assert!(cache.get("plan1").await.is_none());
        assert!(cache.get("plan2").await.is_none());
    }

    #[tokio::test]
    async fn test_plan_cache_clear_clears_dependencies() {
        let cache = PlanCache::new(10, 300);
        cache
            .put_with_dependencies(
                "plan1".to_string(),
                make_test_plan(),
                300,
                vec!["tag_a".to_string()],
            )
            .await;

        cache.clear().await;
        assert_eq!(cache.len().await, 0);
        // dependency_index should also be cleared
        assert!(cache.dependency_index.is_empty());
    }
}
