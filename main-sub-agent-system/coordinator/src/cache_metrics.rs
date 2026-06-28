use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Cache performance metrics using atomic counters
///
/// Tracks hit/miss rates across all cache tiers, SubAgent invocation
/// counts, and forced execution events.
#[derive(Debug)]
pub struct CacheMetrics {
    pub plan_cache_hits: AtomicU64,
    pub plan_cache_misses: AtomicU64,
    pub sub_agent_cache_hits: AtomicU64,
    pub sub_agent_cache_misses: AtomicU64,
    pub response_cache_hits: AtomicU64,
    pub response_cache_misses: AtomicU64,
    pub memory_cache_l1_hits: AtomicU64,
    pub memory_cache_l2_hits: AtomicU64,
    pub memory_cache_l3_hits: AtomicU64,
    pub memory_cache_l4_hits: AtomicU64,
    pub forced_sub_agent_calls: AtomicU64,
    /// Should always be 0 when force_sub_agent=true
    pub skipped_sub_agent_calls: AtomicU64,
    /// Tool result cache hits
    pub tool_cache_hits: AtomicU64,
    /// Tool result cache misses
    pub tool_cache_misses: AtomicU64,
}

impl CacheMetrics {
    pub fn new() -> Self {
        Self {
            plan_cache_hits: AtomicU64::new(0),
            plan_cache_misses: AtomicU64::new(0),
            sub_agent_cache_hits: AtomicU64::new(0),
            sub_agent_cache_misses: AtomicU64::new(0),
            response_cache_hits: AtomicU64::new(0),
            response_cache_misses: AtomicU64::new(0),
            memory_cache_l1_hits: AtomicU64::new(0),
            memory_cache_l2_hits: AtomicU64::new(0),
            memory_cache_l3_hits: AtomicU64::new(0),
            memory_cache_l4_hits: AtomicU64::new(0),
            forced_sub_agent_calls: AtomicU64::new(0),
            skipped_sub_agent_calls: AtomicU64::new(0),
            tool_cache_hits: AtomicU64::new(0),
            tool_cache_misses: AtomicU64::new(0),
        }
    }

    /// Export all metrics as a JSON value
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "plan_cache_hits": self.plan_cache_hits.load(Ordering::Relaxed),
            "plan_cache_misses": self.plan_cache_misses.load(Ordering::Relaxed),
            "sub_agent_cache_hits": self.sub_agent_cache_hits.load(Ordering::Relaxed),
            "sub_agent_cache_misses": self.sub_agent_cache_misses.load(Ordering::Relaxed),
            "response_cache_hits": self.response_cache_hits.load(Ordering::Relaxed),
            "response_cache_misses": self.response_cache_misses.load(Ordering::Relaxed),
            "memory_cache_l1_hits": self.memory_cache_l1_hits.load(Ordering::Relaxed),
            "memory_cache_l2_hits": self.memory_cache_l2_hits.load(Ordering::Relaxed),
            "memory_cache_l3_hits": self.memory_cache_l3_hits.load(Ordering::Relaxed),
            "memory_cache_l4_hits": self.memory_cache_l4_hits.load(Ordering::Relaxed),
            "forced_sub_agent_calls": self.forced_sub_agent_calls.load(Ordering::Relaxed),
            "skipped_sub_agent_calls": self.skipped_sub_agent_calls.load(Ordering::Relaxed),
            "tool_cache_hits": self.tool_cache_hits.load(Ordering::Relaxed),
            "tool_cache_misses": self.tool_cache_misses.load(Ordering::Relaxed),
        })
    }
}

impl Default for CacheMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Whether response cache is enabled (as context only, never skip execution)
    pub response_cache_enabled: bool,
    /// Response cache capacity
    pub response_cache_capacity: usize,
    /// Whether plan cache is enabled
    pub plan_cache_enabled: bool,
    /// Plan cache TTL in seconds
    pub plan_cache_ttl_secs: u64,
    /// SubAgent result cache capacity
    pub sub_agent_cache_capacity: usize,
    /// SubAgent result cache default TTL in seconds
    pub sub_agent_cache_ttl_secs: u64,
    /// Hot cache (L1) capacity
    pub hot_cache_capacity: usize,
    /// Warm cache (L2) capacity
    pub warm_cache_capacity: usize,
    /// Shared cache (L3) capacity
    pub shared_cache_capacity: usize,
    /// Whether global store (L4) is enabled
    pub global_store_enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            response_cache_enabled: true,
            response_cache_capacity: 500,
            plan_cache_enabled: true,
            plan_cache_ttl_secs: 300,
            sub_agent_cache_capacity: 1000,
            sub_agent_cache_ttl_secs: 60,
            hot_cache_capacity: 100,
            warm_cache_capacity: 200,
            shared_cache_capacity: 10000,
            global_store_enabled: true,
        }
    }
}

/// Shared metrics handle for passing to HTTP handlers
pub type SharedCacheMetrics = Arc<CacheMetrics>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_metrics_new() {
        let metrics = CacheMetrics::new();
        assert_eq!(metrics.plan_cache_hits.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.skipped_sub_agent_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cache_metrics_report() {
        let metrics = CacheMetrics::new();
        metrics.plan_cache_hits.fetch_add(5, Ordering::Relaxed);
        metrics
            .forced_sub_agent_calls
            .fetch_add(3, Ordering::Relaxed);

        let report = metrics.report();
        assert_eq!(report["plan_cache_hits"], 5);
        assert_eq!(report["forced_sub_agent_calls"], 3);
        assert_eq!(report["skipped_sub_agent_calls"], 0);
    }

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert!(config.response_cache_enabled);
        assert!(config.plan_cache_enabled);
        assert!(config.global_store_enabled);
        assert_eq!(config.response_cache_capacity, 500);
        assert_eq!(config.shared_cache_capacity, 10000);
    }
}
