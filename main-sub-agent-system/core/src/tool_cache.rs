//! Tool Result Cache
//!
//! Caches results from expensive tool calls to avoid redundant API calls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::Value;

use crate::hash::fnv1a_hash_str;
use crate::tool::{ToolCall, ToolResult};

/// Per-tool TTL configuration
#[derive(Debug, Clone)]
pub struct ToolCacheConfig {
    /// Default TTL for tool results (seconds)
    pub default_ttl_secs: u64,
    /// Per-tool TTL overrides (tool_name -> ttl_secs)
    pub tool_ttl_overrides: std::collections::HashMap<String, u64>,
    /// Max cache capacity
    pub capacity: usize,
    /// Tools that should NEVER be cached (side-effect tools)
    pub excluded_tools: Vec<String>,
}

impl Default for ToolCacheConfig {
    fn default() -> Self {
        let tool_ttl_overrides = std::collections::HashMap::new();

        Self {
            default_ttl_secs: 120, // 2 minutes default
            tool_ttl_overrides,
            capacity: 500,
            excluded_tools: vec![
                "xxt".to_string(),       // Browser automation - always side effects
                "file".to_string(),      // File operations - always side effects
                "datetime".to_string(),  // Time-sensitive, always fresh
            ],
        }
    }
}

/// Cached tool result with metadata
struct CachedToolResult {
    result: ToolResult,
    created_at: Instant,
    ttl: Duration,
    hit_count: AtomicU64,
}

/// Tool result cache with per-tool TTL and semantic key generation
pub struct ToolResultCache {
    cache: DashMap<String, CachedToolResult>,
    config: ToolCacheConfig,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl ToolResultCache {
    pub fn new(config: ToolCacheConfig) -> Self {
        Self {
            cache: DashMap::new(),
            config,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Generate a cache key for a tool call
    /// Key = hash(tool_name + sorted_arguments)
    fn cache_key(&self, call: &ToolCall) -> String {
        // Sort arguments for consistent hashing
        let args_str = if let Value::Object(ref map) = call.arguments {
            let mut pairs: Vec<(&String, &Value)> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| (*k).clone());
            pairs.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&")
        } else {
            call.arguments.to_string()
        };

        fnv1a_hash_str(&[&call.name, &args_str])
    }

    /// Check if a tool call is cacheable
    fn is_cacheable(&self, call: &ToolCall) -> bool {
        !self.config.excluded_tools.contains(&call.name)
    }

    /// Get TTL for a specific tool
    fn tool_ttl(&self, tool_name: &str) -> Duration {
        let secs = self.config.tool_ttl_overrides
            .get(tool_name)
            .copied()
            .unwrap_or(self.config.default_ttl_secs);
        Duration::from_secs(secs)
    }

    /// Try to get a cached result for a tool call
    pub fn get(&self, call: &ToolCall) -> Option<ToolResult> {
        if !self.is_cacheable(call) {
            return None;
        }

        let key = self.cache_key(call);
        if let Some(entry) = self.cache.get_mut(&key) {
            if entry.created_at.elapsed() < entry.ttl {
                entry.hit_count.fetch_add(1, Ordering::Relaxed);
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.result.clone());
            }
            // Expired — remove it
            drop(entry);
            self.cache.remove(&key);
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Store a tool result in the cache
    pub fn put(&self, call: &ToolCall, result: &ToolResult) {
        if !self.is_cacheable(call) {
            return;
        }

        // Only cache successful results
        if !result.success {
            return;
        }

        // Evict if at capacity
        if self.cache.len() >= self.config.capacity {
            self.evict_expired();
            // If still at capacity, evict oldest by removing some entries
            if self.cache.len() >= self.config.capacity {
                let to_evict = self.config.capacity / 10; // Evict 10%
                let keys: Vec<String> = self.cache.iter()
                    .take(to_evict)
                    .map(|e| e.key().clone())
                    .collect();
                for key in keys {
                    self.cache.remove(&key);
                }
            }
        }

        let key = self.cache_key(call);
        let ttl = self.tool_ttl(&call.name);

        self.cache.insert(key, CachedToolResult {
            result: result.clone(),
            created_at: Instant::now(),
            ttl,
            hit_count: AtomicU64::new(0),
        });
    }

    /// Evict all expired entries
    fn evict_expired(&self) {
        let now = Instant::now();
        let expired: Vec<String> = self.cache.iter()
            .filter(|e| now.saturating_duration_since(e.created_at) >= e.ttl)
            .map(|e| e.key().clone())
            .collect();

        for key in expired {
            self.cache.remove(&key);
        }
    }

    /// Invalidate cache entries for a specific tool
    pub fn invalidate_tool(&self, tool_name: &str) {
        let keys: Vec<String> = self.cache.iter()
            .filter(|e| e.result.name == tool_name)
            .map(|e| e.key().clone())
            .collect();

        for key in keys {
            self.cache.remove(&key);
        }
    }

    /// Clear all cached results
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> ToolCacheStats {
        ToolCacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            size: self.cache.len(),
            capacity: self.config.capacity,
        }
    }
}

/// Tool cache statistics
#[derive(Debug, Clone)]
pub struct ToolCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub size: usize,
    pub capacity: usize,
}

impl ToolCacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cache_hit() {
        let cache = ToolResultCache::new(ToolCacheConfig::default());

        // "search" is not in excluded_tools (which excludes xxt/file/datetime
        // as side-effectful tools), so it is cacheable.
        let call = ToolCall {
            id: "test1".to_string(),
            name: "search".to_string(),
            arguments: json!({"query": "rust async"}),
        };

        let result = ToolResult {
            call_id: "test1".to_string(),
            name: "search".to_string(),
            success: true,
            output: json!({"content": "hello world"}),
            error: None,
            execution_duration_ms: 100,
        };

        // Miss on first call
        assert!(cache.get(&call).is_none());

        // Store
        cache.put(&call, &result);

        // Hit on second call
        let cached = cache.get(&call);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().output, json!({"content": "hello world"}));
    }

    #[test]
    fn test_excluded_tools() {
        let cache = ToolResultCache::new(ToolCacheConfig::default());

        let call = ToolCall {
            id: "test2".to_string(),
            name: "xxt".to_string(),
            arguments: json!({"command": "login"}),
        };

        let result = ToolResult {
            call_id: "test2".to_string(),
            name: "xxt".to_string(),
            success: true,
            output: json!({"status": "ok"}),
            error: None,
            execution_duration_ms: 500,
        };

        cache.put(&call, &result);
        assert!(cache.get(&call).is_none()); // xxt is excluded
    }

    #[test]
    fn test_ttl_expiration() {
        let mut config = ToolCacheConfig::default();
        config.default_ttl_secs = 0; // Immediate expiration
        config.tool_ttl_overrides.clear(); // Clear per-tool overrides so default applies
        let cache = ToolResultCache::new(config);

        let call = ToolCall {
            id: "test3".to_string(),
            name: "file".to_string(),
            arguments: json!({"action": "read", "path": "/tmp/test.txt"}),
        };

        let result = ToolResult {
            call_id: "test3".to_string(),
            name: "file".to_string(),
            success: true,
            output: json!({"body": "hello"}),
            error: None,
            execution_duration_ms: 50,
        };

        cache.put(&call, &result);
        // Should be expired immediately (ttl=0)
        assert!(cache.get(&call).is_none());
    }
}
