use async_trait::async_trait;
use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};
use serde_json::Value;

use agent_teams_core::error::{AgentTeamsError, Result};
use agent_teams_core::memory::{
    CompressionStrategy, MemoryEntry, MemoryKind, MemoryQuery, MemoryRelation, MemoryRelationType,
    MemoryRetrievalResult,
};
use agent_teams_core::memory_store::MemoryStore;

/// Redis-backed memory store
pub struct RedisMemoryStore {
    conn: ConnectionManager,
}

impl RedisMemoryStore {
    pub async fn connect(redis_url: &str) -> std::result::Result<Self, RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    fn entry_key(id: &str) -> String {
        format!("mem:entry:{}", id)
    }

    fn session_index_key(session_id: &str) -> String {
        format!("mem:session:{}", session_id)
    }

    fn kind_index_key(kind: &str) -> String {
        format!("mem:kind:{}", kind)
    }

    fn all_entries_key() -> &'static str {
        "mem:all"
    }

    fn relations_key(memory_id: &str) -> String {
        format!("mem:rel:{}", memory_id)
    }

    fn user_profile_key(user_id: &str) -> String {
        format!("mem:profile:{}", user_id)
    }

    fn hash_index_key(content_hash: &str) -> String {
        format!("mem:hash:{}", content_hash)
    }

    async fn store_entry_internal(
        conn: &mut ConnectionManager,
        entry: &MemoryEntry,
    ) -> Result<()> {
        let id = &entry.id;
        let entry_json = serde_json::to_string(entry).map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;
        let entry_key = Self::entry_key(id);

        let score = entry.weight;

        let mut pipe = redis::pipe();

        // Store the full entry as JSON
        pipe.set(&entry_key, &entry_json).ignore();

        // Add to global sorted set (scored by weight)
        pipe.zadd(Self::all_entries_key(), id, score as f64).ignore();

        // Add to session index
        if let Some(ref session_id) = entry.session_id {
            pipe.zadd(
                Self::session_index_key(session_id),
                id,
                entry.created_at.timestamp() as f64,
            )
            .ignore();
        }

        // Add to kind index
        pipe.zadd(Self::kind_index_key(entry.kind.as_str()), id, score as f64)
            .ignore();

        // Add to hash index if content_hash exists
        if let Some(ref hash) = entry.content_hash {
            pipe.set(Self::hash_index_key(hash), id).ignore();
        }

        let mut redis_conn = conn.clone();
        pipe.query_async::<()>(&mut redis_conn).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        Ok(())
    }

    fn entry_from_json(json_str: &str) -> Result<MemoryEntry> {
        serde_json::from_str(json_str).map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))
    }

    fn matches_query(entry: &MemoryEntry, query: &MemoryQuery) -> bool {
        if !query.kinds.is_empty() && !query.kinds.contains(&entry.kind) {
            return false;
        }
        if let Some(ref sid) = query.session_id {
            if entry.session_id.as_ref() != Some(sid) {
                return false;
            }
        }
        if let Some(ref uid) = query.user_id {
            let entry_user = entry
                .data
                .as_ref()
                .and_then(|d| d.get("user_id"))
                .and_then(|v| v.as_str());
            if entry_user != Some(uid.as_str()) {
                return false;
            }
        }
        if let Some(since) = query.since {
            if entry.created_at < since {
                return false;
            }
        }
        if entry.weight < query.min_weight {
            return false;
        }
        if query.confirmed_only && !entry.confirmed {
            return false;
        }
        true
    }
}

#[async_trait]
impl MemoryStore for RedisMemoryStore {
    async fn store(&self, entry: MemoryEntry) -> Result<()> {
        let mut conn = self.conn.clone();
        Self::store_entry_internal(&mut conn, &entry).await
    }

    async fn store_batch(&self, entries: Vec<MemoryEntry>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.clone();
        for entry in &entries {
            Self::store_entry_internal(&mut conn, entry).await?;
        }
        Ok(())
    }

    async fn retrieve(&self, query: MemoryQuery) -> Result<MemoryRetrievalResult> {
        let mut conn = self.conn.clone();

        // Choose the best index to start from
        let candidate_ids: Vec<String> = if let Some(ref session_id) = query.session_id {
            // Use session index
            conn.zrange(Self::session_index_key(session_id), 0, -1)
                .await
                .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?
        } else if query.kinds.len() == 1 {
            // Use kind index
            conn.zrevrange(Self::kind_index_key(query.kinds[0].as_str()), 0, -1)
                .await
                .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?
        } else {
            // Use global index
            conn.zrevrange(Self::all_entries_key(), 0, -1)
                .await
                .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?
        };

        // Fetch entries and filter
        let mut results: Vec<MemoryEntry> = Vec::new();
        for id in &candidate_ids {
            let entry_key = Self::entry_key(id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;

            if let Some(json) = json_str {
                if let Ok(entry) = Self::entry_from_json(&json) {
                    if Self::matches_query(&entry, &query) {
                        results.push(entry);
                    }
                }
            }

            if results.len() >= query.limit * 3 {
                break; // Fetch enough for post-filtering
            }
        }

        // Sort by weight descending
        results.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_available = results.len();
        results.truncate(query.limit);

        Ok(MemoryRetrievalResult {
            entries: results,
            total_available,
        })
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        match json_str {
            Some(json) => Ok(Some(Self::entry_from_json(&json)?)),
            None => Ok(None),
        }
    }

    async fn touch(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(json) = json_str {
            let mut entry = Self::entry_from_json(&json)?;
            entry.last_accessed_at = Utc::now();
            entry.access_count += 1;

            let updated_json = serde_json::to_string(&entry).map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
            let _: () = conn.set(&entry_key, &updated_json).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
        }
        Ok(())
    }

    async fn promote(&self, id: &str, delta: f32) -> Result<()> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(json) = json_str {
            let mut entry = Self::entry_from_json(&json)?;
            entry.weight = (entry.weight + delta).min(1.0);

            let updated_json = serde_json::to_string(&entry).map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
            let mut pipe = redis::pipe();
            pipe.set(&entry_key, &updated_json).ignore();
            // Update weight in indexes
            if let Some(ref sid) = entry.session_id {
                pipe.zadd(Self::session_index_key(sid), id, entry.weight as f64)
                    .ignore();
            }
            pipe.zadd(Self::all_entries_key(), id, entry.weight as f64)
                .ignore();
            pipe.zadd(Self::kind_index_key(entry.kind.as_str()), id, entry.weight as f64)
                .ignore();

            pipe.query_async::<()>(&mut conn).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
        }
        Ok(())
    }

    async fn decay(&self, before: DateTime<Utc>, decay_factor: f32) -> Result<usize> {
        let mut conn = self.conn.clone();
        let all_ids: Vec<String> = conn
            .zrange(Self::all_entries_key(), 0, -1)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        let mut count = 0;
        for id in &all_ids {
            let entry_key = Self::entry_key(id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;

            if let Some(json) = json_str {
                if let Ok(mut entry) = Self::entry_from_json(&json) {
                    if entry.last_accessed_at < before {
                        entry.weight *= decay_factor;
                        let updated_json = serde_json::to_string(&entry).map_err(|e| {
                            AgentTeamsError::StateStoreError(e.to_string())
                        })?;
                        let _: () = conn.set(&entry_key, &updated_json).await.map_err(|e| {
                            AgentTeamsError::StateStoreError(e.to_string())
                        })?;
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    async fn prune(&self, min_weight: f32, before: DateTime<Utc>) -> Result<usize> {
        let mut conn = self.conn.clone();
        let all_ids: Vec<String> = conn
            .zrange(Self::all_entries_key(), 0, -1)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        let mut count = 0;
        for id in &all_ids {
            let entry_key = Self::entry_key(id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;

            if let Some(json) = json_str {
                if let Ok(entry) = Self::entry_from_json(&json) {
                    if entry.weight < min_weight && entry.created_at < before {
                        // Remove from all indexes
                        let mut pipe = redis::pipe();
                        pipe.del(&entry_key).ignore();
                        pipe.zrem(Self::all_entries_key(), id).ignore();
                        if let Some(ref sid) = entry.session_id {
                            pipe.zrem(Self::session_index_key(sid), id).ignore();
                        }
                        pipe.zrem(Self::kind_index_key(entry.kind.as_str()), id)
                            .ignore();
                        if let Some(ref hash) = entry.content_hash {
                            pipe.del(Self::hash_index_key(hash)).ignore();
                        }
                        pipe.query_async::<()>(&mut conn).await.map_err(|e| {
                            AgentTeamsError::StateStoreError(e.to_string())
                        })?;
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    async fn compress(
        &self,
        session_id: &str,
        strategy: CompressionStrategy,
    ) -> Result<Vec<MemoryEntry>> {
        let kind_filter: Vec<&str> = match strategy {
            CompressionStrategy::ExtractFacts => vec!["UserFact"],
            CompressionStrategy::Summarize => vec!["DialogueTurn"],
            CompressionStrategy::UpdateProfile => vec!["InferredPreference", "UserProfile"],
            CompressionStrategy::ClusterTopics => vec!["CrossSessionTopic"],
        };

        let entries = self.list_session_memories(session_id).await?;
        let filtered: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| kind_filter.contains(&e.kind.as_str()))
            .collect();
        Ok(filtered)
    }

    async fn list_session_memories(&self, session_id: &str) -> Result<Vec<MemoryEntry>> {
        let mut conn = self.conn.clone();
        let session_key = Self::session_index_key(session_id);
        let ids: Vec<String> = conn
            .zrange(&session_key, 0, -1)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        let mut entries = Vec::new();
        for id in &ids {
            let entry_key = Self::entry_key(id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
            if let Some(json) = json_str {
                if let Ok(entry) = Self::entry_from_json(&json) {
                    entries.push(entry);
                }
            }
        }

        entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(entries)
    }

    async fn count_session_memories(&self, session_id: &str) -> Result<usize> {
        let mut conn = self.conn.clone();
        let count: usize = conn
            .zcard(Self::session_index_key(session_id))
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;
        Ok(count)
    }

    async fn get_user_profile(&self, user_id: &str) -> Result<Option<Value>> {
        let mut conn = self.conn.clone();
        let profile_key = Self::user_profile_key(user_id);
        let json_str: Option<String> = conn.get(&profile_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        match json_str {
            Some(json) => {
                let value: Value = serde_json::from_str(&json).map_err(|e| {
                    AgentTeamsError::StateStoreError(e.to_string())
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn update_user_profile(&self, user_id: &str, profile: Value) -> Result<()> {
        let mut conn = self.conn.clone();
        let profile_key = Self::user_profile_key(user_id);
        let json_str = serde_json::to_string(&profile).map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;
        let _: () = conn.set(&profile_key, &json_str).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(json) = json_str {
            if let Ok(entry) = Self::entry_from_json(&json) {
                let mut pipe = redis::pipe();
                pipe.del(&entry_key).ignore();
                pipe.zrem(Self::all_entries_key(), id).ignore();
                if let Some(ref sid) = entry.session_id {
                    pipe.zrem(Self::session_index_key(sid), id).ignore();
                }
                pipe.zrem(Self::kind_index_key(entry.kind.as_str()), id)
                    .ignore();
                if let Some(ref hash) = entry.content_hash {
                    pipe.del(Self::hash_index_key(hash)).ignore();
                }
                // Remove relations
                let rel_key = Self::relations_key(id);
                pipe.del(&rel_key).ignore();

                pipe.query_async::<()>(&mut conn).await.map_err(|e| {
                    AgentTeamsError::StateStoreError(e.to_string())
                })?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn add_relation(&self, relation: MemoryRelation) -> Result<()> {
        let mut conn = self.conn.clone();
        let rel_json = serde_json::to_string(&relation).map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        // Store relation under both source and target
        let source_key = Self::relations_key(&relation.source_id);
        let target_key = Self::relations_key(&relation.target_id);

        let rel_key = format!(
            "{}:{}:{}",
            relation.source_id, relation.target_id, relation.relation_type.as_str()
        );

        let mut pipe = redis::pipe();
        pipe.hset(&source_key, &rel_key, &rel_json).ignore();
        if relation.target_id != relation.source_id {
            pipe.hset(&target_key, &rel_key, &rel_json).ignore();
        }
        pipe.query_async::<()>(&mut conn).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        Ok(())
    }

    async fn get_related(
        &self,
        memory_id: &str,
        relation_type: Option<MemoryRelationType>,
    ) -> Result<Vec<MemoryRelation>> {
        let mut conn = self.conn.clone();
        let rel_key = Self::relations_key(memory_id);
        let all_rels: Vec<String> = conn.hvals(&rel_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        let mut results = Vec::new();
        for json_str in all_rels {
            if let Ok(relation) = serde_json::from_str::<MemoryRelation>(&json_str) {
                if relation_type
                    .as_ref()
                    .is_none_or(|rt| relation.relation_type == *rt)
                {
                    results.push(relation);
                }
            }
        }

        Ok(results)
    }

    async fn store_with_relations(
        &self,
        entry: MemoryEntry,
        relations: Vec<MemoryRelation>,
    ) -> Result<()> {
        self.store(entry).await?;
        for relation in relations {
            self.add_relation(relation).await?;
        }
        Ok(())
    }

    async fn get_summary_chain(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let entries = self.list_session_memories(session_id).await?;
        let summaries: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| e.kind == MemoryKind::Summary && !e.archived)
            .take(limit)
            .collect();
        Ok(summaries)
    }

    async fn update_quality(&self, id: &str, quality: f32, source: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(json) = json_str {
            let mut entry = Self::entry_from_json(&json)?;
            entry.confidence = quality;
            entry.weight = (entry.weight * (1.0 + quality) / 2.0).min(1.0);
            entry.source_agent = source.to_string();

            let updated_json = serde_json::to_string(&entry).map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
            let _: () = conn.set(&entry_key, &updated_json).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
        }
        Ok(())
    }

    async fn resolve_contradictions(
        &self,
        entry: &MemoryEntry,
        _threshold: f32,
    ) -> Result<Vec<String>> {
        // Find entries with same kind and different content
        let mut conn = self.conn.clone();
        let kind_key = Self::kind_index_key(entry.kind.as_str());
        let ids: Vec<String> = conn
            .zrange(&kind_key, 0, -1)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        let mut contradicted_ids = Vec::new();

        for id in &ids {
            if *id == entry.id {
                continue;
            }
            let entry_key = Self::entry_key(id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;

            if let Some(json) = json_str {
                if let Ok(existing) = Self::entry_from_json(&json) {
                    if existing.content != entry.content {
                        // Lower weight of contradicted entry
                        let mut updated = existing.clone();
                        updated.weight = (updated.weight * 0.7).max(0.1);
                        let updated_json = serde_json::to_string(&updated).map_err(|e| {
                            AgentTeamsError::StateStoreError(e.to_string())
                        })?;
                        let _: () =
                            conn.set(&entry_key, &updated_json).await.map_err(|e| {
                                AgentTeamsError::StateStoreError(e.to_string())
                            })?;

                        // Record contradiction relation
                        let _ = self
                            .add_relation(MemoryRelation {
                                source_id: entry.id.clone(),
                                target_id: id.clone(),
                                relation_type: MemoryRelationType::Contradicts,
                                strength: 0.8,
                                created_at: Utc::now(),
                            })
                            .await;

                        contradicted_ids.push(id.clone());
                    }
                }
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
        let mut conn = self.conn.clone();
        let hash_key = Self::hash_index_key(content_hash);
        let id: Option<String> = conn.get(&hash_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(entry_id) = id {
            let entry_key = Self::entry_key(&entry_id);
            let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;

            if let Some(json) = json_str {
                let entry = Self::entry_from_json(&json)?;
                if entry.kind == kind
                    && (user_id.is_none()
                        || entry
                            .data
                            .as_ref()
                            .and_then(|d| d.get("user_id"))
                            .and_then(|v| v.as_str())
                            == user_id)
                    && (session_id.is_none() || entry.session_id.as_deref() == session_id)
                {
                    return Ok(Some(entry));
                }
            }
        }
        Ok(None)
    }

    async fn clear(&self) -> Result<usize> {
        let mut conn = self.conn.clone();
        let all_ids: Vec<String> = conn
            .zrange(Self::all_entries_key(), 0, -1)
            .await
            .map_err(|e| AgentTeamsError::StateStoreError(e.to_string()))?;

        let count = all_ids.len();

        // Delete all entry keys and indexes
        let mut pipe = redis::pipe();
        for id in &all_ids {
            pipe.del(Self::entry_key(id)).ignore();
        }
        pipe.del(Self::all_entries_key()).ignore();
        // We can't enumerate all session/kind index keys easily,
        // but entries are gone so they'll be empty
        pipe.query_async::<()>(&mut conn).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        Ok(count)
    }

    async fn archive(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let entry_key = Self::entry_key(id);
        let json_str: Option<String> = conn.get(&entry_key).await.map_err(|e| {
            AgentTeamsError::StateStoreError(e.to_string())
        })?;

        if let Some(json) = json_str {
            let mut entry = Self::entry_from_json(&json)?;
            entry.archived = true;
            let updated_json = serde_json::to_string(&entry).map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
            let _: () = conn.set(&entry_key, &updated_json).await.map_err(|e| {
                AgentTeamsError::StateStoreError(e.to_string())
            })?;
        }
        Ok(())
    }
}
