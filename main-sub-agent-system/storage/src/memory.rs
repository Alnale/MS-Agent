use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

use agent_teams_core::error::Result;
use agent_teams_core::memory::{
    CompressionStrategy, MemoryEntry, MemoryKind, MemoryQuery, MemoryRelation, MemoryRelationType,
    MemoryRetrievalResult,
};
use agent_teams_core::memory_store::MemoryStore;

/// Minimum interval between cleanup_expired runs
const CLEANUP_INTERVAL_SECS: u64 = 60;

/// In-memory memory store for development/testing
pub struct InMemoryMemoryStore {
    entries: DashMap<String, MemoryEntry>,
    user_profiles: DashMap<String, Value>,
    /// TTL index: maps entry id -> expiration instant
    ttl_index: DashMap<String, Instant>,
    /// Default TTL for short-term memories
    default_ttl_secs: u64,
    /// Memory relations: (source_id, target_id, relation_type) -> relation
    relations: DashMap<String, MemoryRelation>,
    /// Inverted index: memory_id -> set of relation keys for O(1) get_related
    relation_index: DashMap<String, Vec<String>>,
    /// Last time cleanup_expired ran
    last_cleanup: Mutex<Instant>,
    /// Secondary index: kind -> set of entry IDs for O(1) kind-based lookups
    kind_index: DashMap<MemoryKind, HashSet<String>>,
    /// Secondary index: session_id -> set of entry IDs for O(1) session-based lookups
    session_index: DashMap<String, HashSet<String>>,
    /// Hash index: content_hash -> entry_id for O(1) content hash lookups
    hash_index: DashMap<String, String>,
}

impl InMemoryMemoryStore {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            user_profiles: DashMap::new(),
            ttl_index: DashMap::new(),
            default_ttl_secs: 86400, // 24h default
            relations: DashMap::new(),
            relation_index: DashMap::new(),
            last_cleanup: Mutex::new(Instant::now()),
            kind_index: DashMap::new(),
            session_index: DashMap::new(),
            hash_index: DashMap::new(),
        }
    }

    /// Add entry ID to secondary indexes
    fn index_entry(&self, entry: &MemoryEntry) {
        self.kind_index
            .entry(entry.kind.clone())
            .or_default()
            .insert(entry.id.clone());
        if let Some(ref sid) = entry.session_id {
            self.session_index
                .entry(sid.clone())
                .or_default()
                .insert(entry.id.clone());
        }
        if let Some(ref hash) = entry.content_hash {
            self.hash_index.insert(hash.clone(), entry.id.clone());
        }
    }

    /// Remove entry ID from secondary indexes
    fn unindex_entry(&self, entry: &MemoryEntry) {
        if let Some(mut ids) = self.kind_index.get_mut(&entry.kind) {
            ids.remove(&entry.id);
        }
        if let Some(ref sid) = entry.session_id {
            if let Some(mut ids) = self.session_index.get_mut(sid) {
                ids.remove(&entry.id);
            }
        }
        if let Some(ref hash) = entry.content_hash {
            self.hash_index.remove(hash);
        }
    }

    /// Get candidate entry IDs based on query filters (uses indexes for O(1) lookup)
    fn candidate_ids(&self, query: &MemoryQuery) -> Option<Vec<String>> {
        // If filtering by session_id only, use session index
        if query.kinds.is_empty() && query.tags.is_empty() {
            if let Some(ref sid) = query.session_id {
                return self.session_index.get(sid).map(|ids| ids.iter().cloned().collect());
            }
        }
        // If filtering by a single kind with no session filter, use kind index
        if query.kinds.len() == 1 && query.tags.is_empty() && query.session_id.is_none() {
            return self.kind_index.get(&query.kinds[0]).map(|ids| ids.iter().cloned().collect());
        }
        // Composite: single kind + session_id — intersect kind and session indexes
        if query.kinds.len() == 1 && query.tags.is_empty() {
            if let Some(ref sid) = query.session_id {
                let kind_ids = self.kind_index.get(&query.kinds[0])?;
                let session_ids = self.session_index.get(sid)?;
                // Intersect: iterate the smaller set, check membership in the larger
                let (smaller, larger) = if kind_ids.len() <= session_ids.len() {
                    (&*kind_ids, &*session_ids)
                } else {
                    (&*session_ids, &*kind_ids)
                };
                return Some(
                    smaller
                        .iter()
                        .filter(|id| larger.contains(*id))
                        .cloned()
                        .collect(),
                );
            }
        }
        None
    }

    pub fn with_default_ttl(mut self, ttl_secs: u64) -> Self {
        self.default_ttl_secs = ttl_secs;
        self
    }

    /// Remove expired entries from the store (throttled to once per CLEANUP_INTERVAL_SECS)
    fn cleanup_expired(&self) {
        {
            let mut last = self.last_cleanup.lock().unwrap_or_else(|e| e.into_inner());
            if last.elapsed().as_secs() < CLEANUP_INTERVAL_SECS {
                return;
            }
            *last = Instant::now();
        }
        let now = Instant::now();
        let expired: Vec<String> = self
            .ttl_index
            .iter()
            .filter(|e| *e.value() <= now)
            .map(|e| e.key().clone())
            .collect();

        for id in expired {
            self.ttl_index.remove(&id);
            if let Some((_, entry)) = self.entries.remove(&id) {
                self.unindex_entry(&entry);
            }
            // Clean up relation index
            if let Some((_, keys)) = self.relation_index.remove(&id) {
                for key in keys {
                    self.relations.remove(&key);
                }
            }
        }
    }

    fn matches_query(entry: &MemoryEntry, query: &MemoryQuery) -> bool {
        // Kind filter
        if !query.kinds.is_empty() && !query.kinds.contains(&entry.kind) {
            return false;
        }
        // Tag filter
        if !query.tags.is_empty() && !query.tags.iter().any(|t| entry.tags.contains(t)) {
            return false;
        }
        // Session filter
        if let Some(ref sid) = query.session_id {
            if entry.session_id.as_ref() != Some(sid) {
                return false;
            }
        }
        // User ID filter — check source_agent field or data JSON for user_id
        if let Some(ref uid) = query.user_id {
            let entry_user = entry
                .data
                .as_ref()
                .and_then(|d| d.get("user_id"))
                .and_then(|v| v.as_str());
            if entry_user != Some(uid.as_str()) {
                // Also allow if source_agent matches user_id pattern
                if !entry.source_agent.contains(uid) {
                    return false;
                }
            }
        }
        // Time filter
        if let Some(since) = query.since {
            if entry.created_at < since {
                return false;
            }
        }
        // Weight filter
        if entry.weight < query.min_weight {
            return false;
        }
        // Confirmed-only filter: exclude unverified agent outputs
        if query.confirmed_only && !entry.confirmed {
            return false;
        }
        true
    }
}

impl Default for InMemoryMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryStore for InMemoryMemoryStore {
    async fn store(&self, entry: MemoryEntry) -> Result<()> {
        // Set TTL for dialogue turns (short-term memories)
        if entry.kind == MemoryKind::DialogueTurn {
            let expiry = Instant::now() + std::time::Duration::from_secs(self.default_ttl_secs);
            self.ttl_index.insert(entry.id.clone(), expiry);
        }
        self.index_entry(&entry);
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    async fn store_batch(&self, entries: Vec<MemoryEntry>) -> Result<()> {
        let now = Instant::now();
        let expiry = now + std::time::Duration::from_secs(self.default_ttl_secs);
        for entry in entries {
            if entry.kind == MemoryKind::DialogueTurn {
                self.ttl_index.insert(entry.id.clone(), expiry);
            }
            self.index_entry(&entry);
            self.entries.insert(entry.id.clone(), entry);
        }
        Ok(())
    }

    async fn retrieve(&self, query: MemoryQuery) -> Result<MemoryRetrievalResult> {
        self.cleanup_expired();

        let mut matches: Vec<MemoryEntry> = if let Some(candidate_ids) = self.candidate_ids(&query) {
            // Use index for fast lookup
            candidate_ids
                .iter()
                .filter_map(|id| self.entries.get(id).map(|e| e.value().clone()))
                .filter(|e| Self::matches_query(e, &query))
                .collect()
        } else {
            // Fallback to full scan
            self.entries
                .iter()
                .filter(|e| Self::matches_query(e.value(), &query))
                .map(|e| e.value().clone())
                .collect()
        };

        // Sort by weight descending (simple relevance for in-memory)
        matches.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total_available = matches.len();
        matches.truncate(query.limit);

        Ok(MemoryRetrievalResult {
            entries: matches,
            total_available,
        })
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<MemoryEntry>> {
        Ok(self.entries.get(id).map(|e| e.value().clone()))
    }

    async fn touch(&self, id: &str) -> Result<()> {
        if let Some(mut entry) = self.entries.get_mut(id) {
            entry.last_accessed_at = Utc::now();
            entry.access_count += 1;
        }
        Ok(())
    }

    async fn promote(&self, id: &str, delta: f32) -> Result<()> {
        if let Some(mut entry) = self.entries.get_mut(id) {
            entry.weight = (entry.weight + delta).min(1.0);
        }
        Ok(())
    }

    async fn decay(&self, before: DateTime<Utc>, decay_factor: f32) -> Result<usize> {
        let mut count = 0;
        for mut entry in self.entries.iter_mut() {
            if entry.last_accessed_at < before {
                entry.weight *= decay_factor;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn prune(&self, min_weight: f32, before: DateTime<Utc>) -> Result<usize> {
        let entries_to_remove: Vec<(String, MemoryEntry)> = self
            .entries
            .iter()
            .filter(|e| e.weight < min_weight && e.created_at < before)
            .map(|e| (e.id.clone(), e.value().clone()))
            .collect();

        let count = entries_to_remove.len();
        for (id, entry) in &entries_to_remove {
            self.entries.remove(id);
            self.ttl_index.remove(id);
            self.unindex_entry(entry);
        }
        Ok(count)
    }

    async fn compress(
        &self,
        session_id: &str,
        strategy: CompressionStrategy,
    ) -> Result<Vec<MemoryEntry>> {
        let session_entries: Vec<MemoryEntry> = self
            .entries
            .iter()
            .filter(|e| e.session_id.as_deref() == Some(session_id))
            .map(|e| e.value().clone())
            .collect();

        match strategy {
            CompressionStrategy::ExtractFacts => {
                // Extract entries marked as confirmed facts
                let facts: Vec<MemoryEntry> = session_entries
                    .into_iter()
                    .filter(|e| e.confirmed || e.kind == MemoryKind::UserFact)
                    .collect();
                Ok(facts)
            }
            CompressionStrategy::Summarize => {
                // Return dialogue turns for external summarization
                let turns: Vec<MemoryEntry> = session_entries
                    .into_iter()
                    .filter(|e| e.kind == MemoryKind::DialogueTurn)
                    .collect();
                Ok(turns)
            }
            CompressionStrategy::UpdateProfile => {
                let profile_data: Vec<MemoryEntry> = session_entries
                    .into_iter()
                    .filter(|e| {
                        e.kind == MemoryKind::InferredPreference
                            || e.kind == MemoryKind::UserProfile
                    })
                    .collect();
                Ok(profile_data)
            }
            CompressionStrategy::ClusterTopics => {
                // Group by tags
                let topics: Vec<MemoryEntry> = session_entries
                    .into_iter()
                    .filter(|e| e.kind == MemoryKind::CrossSessionTopic)
                    .collect();
                Ok(topics)
            }
        }
    }

    async fn list_session_memories(&self, session_id: &str) -> Result<Vec<MemoryEntry>> {
        self.cleanup_expired();
        let mut entries: Vec<MemoryEntry> = if let Some(ids) = self.session_index.get(session_id) {
            ids.iter()
                .filter_map(|id| self.entries.get(id).map(|e| e.value().clone()))
                .collect()
        } else {
            Vec::new()
        };
        entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(entries)
    }

    async fn count_session_memories(&self, session_id: &str) -> Result<usize> {
        self.cleanup_expired();
        let count = self
            .session_index
            .get(session_id)
            .map(|ids| ids.len())
            .unwrap_or(0);
        Ok(count)
    }

    async fn get_user_profile(&self, user_id: &str) -> Result<Option<Value>> {
        Ok(self.user_profiles.get(user_id).map(|v| v.value().clone()))
    }

    async fn update_user_profile(&self, user_id: &str, profile: Value) -> Result<()> {
        self.user_profiles.insert(user_id.to_string(), profile);
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        if let Some((_, entry)) = self.entries.remove(id) {
            self.ttl_index.remove(id);
            self.unindex_entry(&entry);
            if let Some((_, keys)) = self.relation_index.remove(id) {
                for key in keys {
                    self.relations.remove(&key);
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn add_relation(&self, relation: MemoryRelation) -> Result<()> {
        let key = format!(
            "{}:{}:{}",
            relation.source_id,
            relation.target_id,
            relation.relation_type.as_str()
        );
        // Update inverted index for both source and target
        self.relation_index
            .entry(relation.source_id.clone())
            .or_default()
            .push(key.clone());
        if relation.target_id != relation.source_id {
            self.relation_index
                .entry(relation.target_id.clone())
                .or_default()
                .push(key.clone());
        }
        self.relations.insert(key, relation);
        Ok(())
    }

    async fn get_related(
        &self,
        memory_id: &str,
        relation_type: Option<MemoryRelationType>,
    ) -> Result<Vec<MemoryRelation>> {
        // Use inverted index for O(1) lookup
        let keys = self
            .relation_index
            .get(memory_id)
            .map(|v| v.clone())
            .unwrap_or_default();
        let results: Vec<MemoryRelation> = keys
            .iter()
            .filter_map(|key| self.relations.get(key).map(|r| r.value().clone()))
            .filter(|r| {
                relation_type
                    .as_ref()
                    .is_none_or(|rt| r.relation_type == *rt)
            })
            .collect();
        Ok(results)
    }

    async fn store_with_relations(
        &self,
        entry: MemoryEntry,
        relations: Vec<MemoryRelation>,
    ) -> Result<()> {
        // Store entry
        self.store(entry).await?;
        // Store relations
        for relation in relations {
            self.add_relation(relation).await?;
        }
        Ok(())
    }

    async fn get_summary_chain(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.cleanup_expired();
        let mut entries: Vec<MemoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.session_id.as_deref() == Some(session_id)
                    && e.kind == MemoryKind::Summary
                    && !e.archived
            })
            .map(|e| e.value().clone())
            .collect();
        entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        entries.truncate(limit);
        Ok(entries)
    }

    async fn update_quality(&self, id: &str, quality: f32, _source: &str) -> Result<()> {
        if let Some(mut entry) = self.entries.get_mut(id) {
            entry.confidence = quality;
            entry.weight = (entry.weight * (1.0 + quality) / 2.0).min(1.0);
        }
        Ok(())
    }

    async fn resolve_contradictions(
        &self,
        entry: &MemoryEntry,
        _threshold: f32,
    ) -> Result<Vec<String>> {
        // For in-memory store, find entries with same kind and different content
        // Use kind_index for O(1) lookup instead of full scan
        let mut contradicted_ids = Vec::new();
        let similar: Vec<(String, String)> = self
            .kind_index
            .get(&entry.kind)
            .map(|ids| {
                ids.iter()
                    .filter(|id| **id != entry.id)
                    .filter_map(|id| {
                        self.entries.get(id).map(|e| (e.id.clone(), e.content.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (id, content) in similar {
            if content != entry.content {
                // Lower weight
                if let Some(mut existing) = self.entries.get_mut(&id) {
                    existing.weight = (existing.weight * 0.7).max(0.1);
                }
                // Record contradiction
                let key = format!("{}:{}:contradicts", entry.id, id);
                self.relations.insert(
                    key,
                    MemoryRelation {
                        source_id: entry.id.clone(),
                        target_id: id.clone(),
                        relation_type: MemoryRelationType::Contradicts,
                        strength: 0.8,
                        created_at: Utc::now(),
                    },
                );
                contradicted_ids.push(id);
            }
        }

        Ok(contradicted_ids)
    }

    async fn find_by_content_hash(
        &self,
        content_hash: &str,
        kind: MemoryKind,
        user_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Option<MemoryEntry>> {
        // Use hash_index for O(1) lookup by content_hash
        if let Some(entry_id) = self.hash_index.get(content_hash) {
            if let Some(entry) = self.entries.get(entry_id.value()) {
                let e = entry.value();
                if e.kind == kind
                    && (user_id.is_none()
                        || e.data
                            .as_ref()
                            .and_then(|d| d.get("user_id"))
                            .and_then(|v| v.as_str())
                            == user_id)
                    && (session_id.is_none()
                        || e.session_id.as_deref() == session_id)
                {
                    return Ok(Some(e.clone()));
                }
            }
        }
        Ok(None)
    }

    async fn clear(&self) -> Result<usize> {
        let count = self.entries.len();
        self.entries.clear();
        self.ttl_index.clear();
        self.relations.clear();
        self.relation_index.clear();
        self.kind_index.clear();
        self.session_index.clear();
        self.hash_index.clear();
        self.user_profiles.clear();
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, session: &str, kind: MemoryKind, weight: f32) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            session_id: Some(session.to_string()),
            kind,
            content: format!("content of {}", id),
            data: None,
            embedding: None,
            weight,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 0,
            tags: vec![],
            source_agent: "test".to_string(),
            confirmed: false,
            content_hash: None,
            confidence: 1.0,
            parent_id: None,
            version: 1,
            archived: false,
            compressed_from: vec![],
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let store = InMemoryMemoryStore::new();
        let entry = make_entry("1", "s1", MemoryKind::UserFact, 0.8);
        store.store(entry).await.unwrap();

        let result = store
            .retrieve(MemoryQuery {
                session_id: Some("s1".to_string()),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].id, "1");
    }

    #[tokio::test]
    async fn test_kind_filter() {
        let store = InMemoryMemoryStore::new();
        store
            .store(make_entry("1", "s1", MemoryKind::UserFact, 0.8))
            .await
            .unwrap();
        store
            .store(make_entry("2", "s1", MemoryKind::DialogueTurn, 0.5))
            .await
            .unwrap();

        let result = store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::UserFact],
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].kind, MemoryKind::UserFact);
    }

    #[tokio::test]
    async fn test_touch_increments_count() {
        let store = InMemoryMemoryStore::new();
        store
            .store(make_entry("1", "s1", MemoryKind::UserFact, 0.5))
            .await
            .unwrap();

        store.touch("1").await.unwrap();
        let entry = store.get_by_id("1").await.unwrap().unwrap();
        assert_eq!(entry.access_count, 1);
    }

    #[tokio::test]
    async fn test_promote_weight() {
        let store = InMemoryMemoryStore::new();
        store
            .store(make_entry("1", "s1", MemoryKind::UserFact, 0.5))
            .await
            .unwrap();

        store.promote("1", 0.3).await.unwrap();
        let entry = store.get_by_id("1").await.unwrap().unwrap();
        assert!((entry.weight - 0.8).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_prune() {
        let store = InMemoryMemoryStore::new();
        store
            .store(make_entry("1", "s1", MemoryKind::UserFact, 0.05))
            .await
            .unwrap();

        let pruned = store
            .prune(0.1, Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(pruned, 1);
        assert!(store.get_by_id("1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_user_profile() {
        let store = InMemoryMemoryStore::new();
        let profile = serde_json::json!({"name": "test_user", "preferences": ["dark_mode"]});
        store
            .update_user_profile("user1", profile.clone())
            .await
            .unwrap();

        let retrieved = store.get_user_profile("user1").await.unwrap().unwrap();
        assert_eq!(retrieved["name"], "test_user");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = InMemoryMemoryStore::new();
        store
            .store(make_entry("1", "s1", MemoryKind::UserFact, 0.5))
            .await
            .unwrap();

        assert!(store.delete("1").await.unwrap());
        assert!(store.get_by_id("1").await.unwrap().is_none());
    }
}
