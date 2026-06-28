use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lru::LruCache;

use agent_teams_core::boxed_agent::AgentOutput;

/// Tiered TTL configuration for SubAgent cache
#[derive(Debug, Clone)]
pub struct SubAgentTtlConfig {
    /// High-frequency agents (e.g. knowledge) TTL
    pub hot_ttl_secs: u64,
    /// Medium-frequency agents TTL
    pub warm_ttl_secs: u64,
    /// Low-frequency agents TTL
    pub cold_ttl_secs: u64,
    /// Extend TTL after N hits
    pub hit_extension_threshold: u64,
    /// TTL extension ratio per threshold
    pub hit_extension_ratio: f32,
    /// Per-agent TTL overrides (agent_id -> ttl_secs)
    pub agent_ttl_overrides: std::collections::HashMap<String, u64>,
}

impl Default for SubAgentTtlConfig {
    fn default() -> Self {
        Self {
            hot_ttl_secs: 300,  // 5 minutes
            warm_ttl_secs: 120, // 2 minutes
            cold_ttl_secs: 60,  // 1 minute
            hit_extension_threshold: 3,
            hit_extension_ratio: 1.5,
            agent_ttl_overrides: std::collections::HashMap::new(),
        }
    }
}

/// Cached sub-agent result with metadata
struct CachedResult {
    output: AgentOutput,
    created_at: Instant,
    ttl: Duration,
    /// Hit count for dynamic TTL extension
    hit_count: AtomicU64,
    /// Source agent ID
    agent_id: String,
    /// Whether this is a "soft" cache (force-mode write, skip in force-mode reads)
    is_soft: bool,
}

/// Enhanced sub-agent result cache with tiered TTL and soft cache support
pub struct SubAgentCache {
    cache: Mutex<LruCache<String, CachedResult>>,
    /// Tiered TTL configuration
    ttl_config: SubAgentTtlConfig,
    /// Hit counter for stats
    hits: AtomicU64,
    /// Miss counter for stats
    misses: AtomicU64,
}

impl SubAgentCache {
    pub fn new(capacity: usize, _ttl_secs: u64) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            ttl_config: SubAgentTtlConfig::default(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn with_ttl_config(mut self, config: SubAgentTtlConfig) -> Self {
        self.ttl_config = config;
        self
    }

    /// Get cached result. In force_mode, soft cache entries are skipped.
    pub async fn get(&self, key: &str, force_mode: bool) -> Option<AgentOutput> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(meta) = cache.get(key) {
            // Force mode: soft cache not usable (needs re-execution for memory refresh)
            if force_mode && meta.is_soft {
                tracing::debug!("Force mode: skipping soft cache for key {}", key);
                return None;
            }

            let elapsed = meta.created_at.elapsed();
            let effective_ttl = if meta.hit_count.load(Ordering::Relaxed)
                >= self.ttl_config.hit_extension_threshold
            {
                Duration::from_secs(
                    (meta.ttl.as_secs() as f32 * self.ttl_config.hit_extension_ratio) as u64,
                )
            } else {
                meta.ttl
            };

            if elapsed < effective_ttl {
                meta.hit_count.fetch_add(1, Ordering::Relaxed);
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(meta.output.clone());
            } else {
                cache.pop(key);
            }
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Put cached result with agent-aware TTL
    pub async fn put(&self, key: String, output: AgentOutput, agent_id: &str, force_mode: bool) {
        let ttl = self.agent_ttl(agent_id);
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.put(
            key,
            CachedResult {
                output,
                created_at: Instant::now(),
                ttl,
                hit_count: AtomicU64::new(0),
                agent_id: agent_id.to_string(),
                is_soft: force_mode,
            },
        );
    }

    /// Legacy put interface for backward compatibility
    pub async fn put_simple(&self, key: String, output: AgentOutput, ttl_secs: u64) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.put(
            key,
            CachedResult {
                output,
                created_at: Instant::now(),
                ttl: Duration::from_secs(ttl_secs),
                hit_count: AtomicU64::new(0),
                agent_id: String::new(),
                is_soft: false,
            },
        );
    }

    /// Legacy get interface for backward compatibility
    pub async fn get_simple(&self, key: &str) -> Option<AgentOutput> {
        self.get(key, false).await
    }

    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.pop(key);
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.clear();
    }

    /// Invalidate cached results whose agent_id matches any of the changed tags.
    /// Used when memory context changes affect specific agents.
    pub async fn invalidate_by_context_change(&self, changed_tags: &[String]) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut to_remove = Vec::new();

        for (key, cached) in cache.iter() {
            if changed_tags.iter().any(|tag| tag == &cached.agent_id) {
                to_remove.push(key.clone());
            }
        }

        for key in to_remove {
            cache.pop(&key);
        }
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

    /// Get cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let h = self.hits.load(Ordering::Relaxed);
        let m = self.misses.load(Ordering::Relaxed);
        let total = h + m;
        if total == 0 {
            0.0
        } else {
            h as f64 / total as f64
        }
    }

    /// Get tiered TTL for a specific agent
    /// Priority: per-agent override > tier-based default
    fn agent_ttl(&self, agent_id: &str) -> Duration {
        // Check per-agent override first
        if let Some(&override_secs) = self.ttl_config.agent_ttl_overrides.get(agent_id) {
            return Duration::from_secs(override_secs);
        }
        // Fall back to tier-based defaults
        match agent_id {
            "knowledge" | "sentiment" | "tool_agent" => {
                Duration::from_secs(self.ttl_config.hot_ttl_secs)
            }
            "task_planner" => {
                Duration::from_secs(self.ttl_config.warm_ttl_secs)
            }
            _ => Duration::from_secs(self.ttl_config.cold_ttl_secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sub_agent_cache_put_and_get() {
        let cache = SubAgentCache::new(10, 300);
        let output = AgentOutput {
            content: "test result".to_string(),
            quality: 0.9,
            ..Default::default()
        };
        cache
            .put("key1".to_string(), output, "knowledge", false)
            .await;

        let cached = cache.get("key1", false).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().content, "test result");
    }

    #[tokio::test]
    async fn test_sub_agent_cache_miss() {
        let cache = SubAgentCache::new(10, 300);
        assert!(cache.get("nonexistent", false).await.is_none());
    }

    #[tokio::test]
    async fn test_sub_agent_cache_force_mode_skips_soft() {
        let cache = SubAgentCache::new(10, 300);
        let output = AgentOutput::default();
        cache
            .put("key1".to_string(), output, "knowledge", true)
            .await; // soft cache

        // Normal mode should find it
        assert!(cache.get("key1", false).await.is_some());
        // Force mode should skip soft cache
        assert!(cache.get("key1", true).await.is_none());
    }

    #[tokio::test]
    async fn test_sub_agent_cache_legacy_interface() {
        let cache = SubAgentCache::new(10, 300);
        let output = AgentOutput {
            content: "legacy test".to_string(),
            ..Default::default()
        };
        cache.put_simple("key1".to_string(), output, 300).await;

        let cached = cache.get_simple("key1").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().content, "legacy test");
    }

    #[tokio::test]
    async fn test_sub_agent_cache_tiered_ttl() {
        let cache = SubAgentCache::new(10, 300);
        let output = AgentOutput::default();

        // knowledge agent gets hot TTL (300s)
        cache
            .put("k1".to_string(), output.clone(), "knowledge", false)
            .await;
        // sentiment agent gets warm TTL (120s)
        cache
            .put("s1".to_string(), output.clone(), "sentiment", false)
            .await;
        // unknown agent gets cold TTL (60s)
        cache.put("u1".to_string(), output, "unknown", false).await;

        assert!(cache.get("k1", false).await.is_some());
        assert!(cache.get("s1", false).await.is_some());
        assert!(cache.get("u1", false).await.is_some());
    }

    #[tokio::test]
    async fn test_sub_agent_cache_invalidate_by_context_change() {
        let cache = SubAgentCache::new(10, 300);
        let output = AgentOutput::default();

        cache
            .put("k1".to_string(), output.clone(), "knowledge", false)
            .await;
        cache
            .put("s1".to_string(), output.clone(), "sentiment", false)
            .await;
        cache
            .put("k2".to_string(), output, "knowledge", false)
            .await;

        // Invalidate knowledge agent's cached results
        cache
            .invalidate_by_context_change(&["knowledge".to_string()])
            .await;

        assert!(cache.get("k1", false).await.is_none());
        assert!(cache.get("k2", false).await.is_none());
        assert!(cache.get("s1", false).await.is_some());
    }
}
