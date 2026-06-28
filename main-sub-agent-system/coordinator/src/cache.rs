use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::Instant;

use lru::LruCache;

use agent_teams_core::boxed_agent::AgentOutput;

/// Default TTL for response cache entries (120 seconds)
pub const DEFAULT_RESPONSE_CACHE_TTL_SECS: u64 = 120;

struct CachedEntry {
    output: AgentOutput,
    inserted_at: Instant,
    ttl_secs: u64,
}

/// Response cache using lru::LruCache with TTL support
pub struct ResponseCache {
    cache: Mutex<LruCache<String, CachedEntry>>,
    default_ttl_secs: u64,
}

impl ResponseCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_ttl(capacity, DEFAULT_RESPONSE_CACHE_TTL_SECS)
    }

    pub fn with_ttl(capacity: usize, ttl_secs: u64) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::new(500).unwrap());
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            default_ttl_secs: ttl_secs.max(1),
        }
    }

    pub async fn get(&self, key: &str) -> Option<AgentOutput> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(key) {
            if entry.inserted_at.elapsed().as_secs() < entry.ttl_secs {
                return Some(entry.output.clone());
            }
            // Expired — remove it
            cache.pop(key);
        }
        None
    }

    pub async fn put(&self, key: String, response: AgentOutput) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.put(key, CachedEntry {
            output: response,
            inserted_at: Instant::now(),
            ttl_secs: self.default_ttl_secs,
        });
    }

    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.pop(key);
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.clear();
    }

    pub async fn len(&self) -> usize {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub async fn is_empty(&self) -> bool {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = ResponseCache::new(10);
        let output = AgentOutput {
            content: "test".to_string(),
            quality: 0.9,
            ..Default::default()
        };
        cache.put("key1".to_string(), output).await;

        let cached = cache.get("key1").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().content, "test");
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = ResponseCache::new(10);
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = ResponseCache::new(10);
        cache.put("key1".to_string(), AgentOutput::default()).await;
        assert!(cache.get("key1").await.is_some());

        cache.invalidate("key1").await;
        assert!(cache.get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = ResponseCache::new(4);
        for i in 0..5 {
            cache.put(format!("key{}", i), AgentOutput::default()).await;
        }
        assert!(cache.len().await <= 4);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = ResponseCache::new(10);
        cache.put("key1".to_string(), AgentOutput::default()).await;
        cache.put("key2".to_string(), AgentOutput::default()).await;

        cache.clear().await;
        assert_eq!(cache.len().await, 0);
        assert!(cache.get("key1").await.is_none());
    }
}
