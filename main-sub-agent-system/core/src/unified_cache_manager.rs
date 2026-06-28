use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;

use crate::agent_memory_cache::AgentMemoryCache;
use crate::memory::{MemoryEntry, MemoryQuery};
use crate::memory_event_bus::{MemoryChangeEvent, MemoryEventBus};
use crate::memory_store::MemoryStore;
use crate::unified_memory_bus::SharedMemoryCache;

/// Cache manager configuration
#[derive(Debug, Clone)]
pub struct CacheManagerConfig {
    /// Default TTL in seconds for cache entries
    pub default_ttl_secs: u64,
    /// Session timeout in seconds (unused for now, reserved for session lifecycle)
    pub session_timeout_secs: u64,
    /// Background cleanup interval in seconds
    pub cleanup_interval_secs: u64,
    /// Whether cross-session cache lookup is allowed
    pub enable_cross_session_cache: bool,
    /// Shared cache capacity
    pub shared_cache_capacity: usize,
}

impl Default for CacheManagerConfig {
    fn default() -> Self {
        Self {
            default_ttl_secs: 3600,
            session_timeout_secs: 86400,
            cleanup_interval_secs: 600,
            enable_cross_session_cache: true,
            shared_cache_capacity: 2000,
        }
    }
}

/// Cache operation error
#[derive(Debug)]
pub enum CacheError {
    VersionConflict(String),
    StorageError(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::VersionConflict(id) => write!(f, "Version conflict for memory entry {}", id),
            CacheError::StorageError(msg) => write!(f, "Storage error: {}", msg),
        }
    }
}

impl std::error::Error for CacheError {}

/// Unified cache manager: coordinates all cache layers with TTL, session isolation, and event bus
pub struct UnifiedCacheManager {
    shared_cache: Arc<SharedMemoryCache>,
    agent_caches: DashMap<String, Arc<AgentMemoryCache>>,
    event_bus: Arc<MemoryEventBus>,
    config: CacheManagerConfig,
    global_store: Option<Arc<dyn MemoryStore>>,
    cleanup_started: AtomicBool,
}

impl UnifiedCacheManager {
    pub fn new(config: CacheManagerConfig, event_bus: Arc<MemoryEventBus>) -> Self {
        Self {
            shared_cache: Arc::new(SharedMemoryCache::with_ttl(
                config.shared_cache_capacity,
                config.default_ttl_secs,
            )),
            agent_caches: DashMap::new(),
            event_bus,
            config,
            global_store: None,
            cleanup_started: AtomicBool::new(false),
        }
    }

    pub fn with_global_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.global_store = Some(store);
        self
    }

    /// Register an agent's cache
    pub fn register_agent_cache(&self, agent_id: &str, cache: Arc<AgentMemoryCache>) {
        self.agent_caches.insert(agent_id.to_string(), cache);
    }

    /// Get the shared cache reference
    pub fn shared_cache(&self) -> &Arc<SharedMemoryCache> {
        &self.shared_cache
    }

    /// Get the event bus reference
    pub fn event_bus(&self) -> &Arc<MemoryEventBus> {
        &self.event_bus
    }

    /// Unified query: agent local cache -> shared cache -> global store
    pub async fn query(&self, agent_id: &str, query: &MemoryQuery) -> Vec<MemoryEntry> {
        // If session-scoped and cross-session disabled, only use agent cache
        if query.session_id.is_some() && !self.config.enable_cross_session_cache {
            if let Some(cache) = self.agent_caches.get(agent_id) {
                return cache.query(query).await;
            }
            return Vec::new();
        }

        let mut results = Vec::new();

        // 1. Agent local cache
        if let Some(cache) = self.agent_caches.get(agent_id) {
            results.extend(cache.query(query).await);
        }

        // 2. Shared cache (exclude duplicates)
        if let Some(shared_results) = self.shared_cache.query(query).await {
            let existing: std::collections::HashSet<String> =
                results.iter().map(|e| e.id.clone()).collect();
            results.extend(shared_results.into_iter().filter(|e| !existing.contains(&e.id)));
        }

        results
    }

    /// Store with version check and event notification
    pub async fn store(&self, agent_id: &str, entry: MemoryEntry) -> std::result::Result<(), CacheError> {
        // Version check (optimistic lock) against agent cache
        if let Some(cache) = self.agent_caches.get(agent_id) {
            if let Err(e) = cache.store_with_version(entry.clone()).await {
                return Err(CacheError::VersionConflict(e.to_string()));
            }
        }

        // Update shared cache
        self.shared_cache.put(entry.clone()).await;

        // Publish event
        self.event_bus.publish(MemoryChangeEvent::Stored {
            agent_id: agent_id.to_string(),
            session_id: entry.session_id.clone(),
            memory_kind: entry.kind.clone(),
            tags: entry.tags.clone(),
            content_hash: entry.content_hash.clone().unwrap_or_default(),
        });

        Ok(())
    }

    /// End a session: clean up all cache layers for the given session
    pub async fn end_session(&self, session_id: &str) {
        // Shared cache
        self.shared_cache.invalidate_session(session_id).await;

        // Agent caches
        for entry in self.agent_caches.iter() {
            entry.value().clear_session(session_id).await;
        }

        // Event
        self.event_bus.publish(MemoryChangeEvent::SessionEnded {
            session_id: session_id.to_string(),
        });

        tracing::info!("Session {} ended, all caches cleaned", session_id);
    }

    /// Start background cleanup task (idempotent)
    pub async fn start_cleanup_task(&self) {
        if self.cleanup_started.swap(true, Ordering::Relaxed) {
            return;
        }

        let interval_secs = self.config.cleanup_interval_secs;
        let shared = self.shared_cache.clone();
        let agents = self.agent_caches.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                ticker.tick().await;
                // Shared cache TTL cleanup
                shared.cleanup_expired().await;
                // Agent cache TTL cleanup
                for entry in agents.iter() {
                    entry.value().cleanup_expired().await;
                }
            }
        });

        tracing::info!(
            "Cache cleanup task started (interval={}s)",
            self.config.cleanup_interval_secs
        );
    }
}
