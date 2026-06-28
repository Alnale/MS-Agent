use std::sync::Arc;

use crate::error::Result;
use crate::memory::MemoryEntry;
use crate::memory_store::EmbeddingProvider;

/// A memory entry with its computed rerank score
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub entry: MemoryEntry,
    pub final_score: f32,
}

/// Memory reranker: refines initial retrieval results using embedding-based scoring
/// and filters out low-quality memories via an admission threshold.
pub struct MemoryReranker {
    embedding_provider: Arc<dyn EmbeddingProvider>,
    /// Minimum score for a memory to be included in working memory
    admission_threshold: f32,
    /// Weight for cross-encoder style score (embedding similarity to query)
    cross_weight: f32,
    /// Weight for the memory's existing weight/score
    memory_weight: f32,
}

impl MemoryReranker {
    pub fn new(embedding_provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            embedding_provider,
            admission_threshold: 0.25,
            cross_weight: 0.5,
            memory_weight: 0.5,
        }
    }

    pub fn with_admission_threshold(mut self, threshold: f32) -> Self {
        self.admission_threshold = threshold;
        self
    }

    /// Rerank candidates by computing embedding similarity to the query.
    /// Prefer `rerank_with_embedding` when the query embedding is already available.
    pub async fn rerank(
        &self,
        query: &str,
        candidates: Vec<MemoryEntry>,
    ) -> Result<Vec<ScoredMemory>> {
        let query_embedding = self
            .embedding_provider
            .embed(query)
            .await
            .map_err(|e| crate::error::AgentTeamsError::Internal(e.to_string()))?;
        self.rerank_with_embedding(&query_embedding, candidates)
    }

    /// Rerank candidates using a pre-computed query embedding.
    /// Avoids redundant embedding API calls when the caller already has the embedding.
    pub fn rerank_with_embedding(
        &self,
        query_embedding: &[f32],
        candidates: Vec<MemoryEntry>,
    ) -> Result<Vec<ScoredMemory>> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut scored: Vec<ScoredMemory> = candidates
            .into_iter()
            .map(|entry| {
                let cross_score = entry
                    .embedding
                    .as_ref()
                    .map(|emb| cosine_similarity(emb, query_embedding))
                    .unwrap_or(0.0);

                let final_score =
                    self.cross_weight * cross_score + self.memory_weight * entry.weight;

                ScoredMemory { entry, final_score }
            })
            .filter(|s| s.final_score >= self.admission_threshold)
            .collect();

        scored.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(scored)
    }
}

use crate::hash::cosine_similarity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
