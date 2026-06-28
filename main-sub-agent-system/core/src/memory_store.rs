use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::error::Result;
use crate::memory::{
    CompressionStrategy, MemoryEntry, MemoryKind, MemoryQuery, MemoryRelation, MemoryRelationType,
    MemoryRetrievalResult,
};

/// Embedding provider trait for generating vector representations
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
        // Default: sequential single embed
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Embedding dimension
    fn dimensions(&self) -> usize;

    /// Model identifier
    fn model_id(&self) -> &str;
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("Rate limited")]
    RateLimited,
    #[error("Provider unavailable: {0}")]
    Unavailable(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("{0}")]
    Other(String),
}

/// Memory storage trait — unified interface for short-term and long-term memory
#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Store a single memory entry
    async fn store(&self, entry: MemoryEntry) -> Result<()>;

    /// Batch store
    async fn store_batch(&self, entries: Vec<MemoryEntry>) -> Result<()>;

    /// Semantic retrieval (core interface)
    async fn retrieve(&self, query: MemoryQuery) -> Result<MemoryRetrievalResult>;

    /// Get by ID
    async fn get_by_id(&self, id: &str) -> Result<Option<MemoryEntry>>;

    /// Update access stats (touch)
    async fn touch(&self, id: &str) -> Result<()>;

    /// Promote weight (user confirmed or important mark)
    async fn promote(&self, id: &str, delta: f32) -> Result<()>;

    /// Decay old memory weights (scheduled task)
    async fn decay(&self, before: DateTime<Utc>, decay_factor: f32) -> Result<usize>;

    /// Prune low-weight memories (cleanup)
    async fn prune(&self, min_weight: f32, before: DateTime<Utc>) -> Result<usize>;

    /// Execute compression strategy
    async fn compress(
        &self,
        session_id: &str,
        strategy: CompressionStrategy,
    ) -> Result<Vec<MemoryEntry>>;

    /// List all memories for a session
    async fn list_session_memories(&self, session_id: &str) -> Result<Vec<MemoryEntry>>;

    /// Get user profile
    async fn get_user_profile(&self, user_id: &str) -> Result<Option<Value>>;

    /// Update user profile
    async fn update_user_profile(&self, user_id: &str, profile: Value) -> Result<()>;

    /// Delete a memory entry
    async fn delete(&self, id: &str) -> Result<bool>;

    /// Add a relation between two memories
    async fn add_relation(&self, relation: MemoryRelation) -> Result<()>;

    /// Get related memories for a given memory id
    async fn get_related(
        &self,
        memory_id: &str,
        relation_type: Option<MemoryRelationType>,
    ) -> Result<Vec<MemoryRelation>>;

    /// Resolve contradictions: find memories that contradict the given entry
    /// and lower their weight. Returns the IDs of contradicted memories.
    async fn resolve_contradictions(
        &self,
        entry: &MemoryEntry,
        threshold: f32,
    ) -> Result<Vec<String>> {
        let _ = (entry, threshold);
        Ok(Vec::new())
    }

    /// Count memories for a session
    async fn count_session_memories(&self, session_id: &str) -> Result<usize> {
        let memories = self.list_session_memories(session_id).await?;
        Ok(memories.len())
    }

    /// Find a memory entry by exact content hash.
    /// Returns the first matching entry, or None if no match is found.
    async fn find_by_content_hash(
        &self,
        content_hash: &str,
        kind: MemoryKind,
        user_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Option<MemoryEntry>> {
        // Default: retrieve with a generous limit and filter in Rust
        let query = MemoryQuery {
            kinds: vec![kind],
            limit: 100,
            min_weight: 0.0,
            user_id: user_id.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            ..Default::default()
        };
        let result = self.retrieve(query).await?;
        Ok(result
            .entries
            .into_iter()
            .find(|e| e.content_hash.as_deref() == Some(content_hash)))
    }

    /// Store entry with relations in a single transaction
    async fn store_with_relations(
        &self,
        entry: MemoryEntry,
        relations: Vec<MemoryRelation>,
    ) -> Result<()>;

    /// Get summary chain for a session (ordered by time)
    async fn get_summary_chain(&self, session_id: &str, limit: usize) -> Result<Vec<MemoryEntry>>;

    /// Update memory quality metadata (confidence and weight)
    async fn update_quality(&self, id: &str, quality: f32, source: &str) -> Result<()>;

    /// Clear all memories from the store
    async fn clear(&self) -> Result<usize>;

    /// Archive a memory entry (soft delete - marks as archived but doesn't remove)
    async fn archive(&self, id: &str) -> Result<()> {
        // Default implementation: do nothing
        // Implementors should override this to mark entries as archived
        let _ = id;
        Ok(())
    }
}
