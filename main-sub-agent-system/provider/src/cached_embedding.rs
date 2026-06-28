use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Mutex;

use async_trait::async_trait;
use lru::LruCache;

use agent_teams_core::memory_store::{EmbeddingError, EmbeddingProvider};

/// Cached embedding provider that wraps any EmbeddingProvider with an LRU cache
pub struct CachedEmbeddingProvider {
    inner: Box<dyn EmbeddingProvider>,
    cache: Mutex<LruCache<u64, Vec<f32>>>,
}

impl CachedEmbeddingProvider {
    pub fn new(inner: Box<dyn EmbeddingProvider>, cache_size: usize) -> Self {
        Self {
            inner,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_size.max(1)).expect("cache_size.max(1) is always >= 1"),
            )),
        }
    }

    fn hash_text(text: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

#[async_trait]
impl EmbeddingProvider for CachedEmbeddingProvider {
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError> {
        let hash = Self::hash_text(text);

        // Check cache
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = cache.get(&hash) {
                return Ok(cached.clone());
            }
        }

        // Generate embedding
        let embedding = self.inner.embed(text).await?;

        // Store in cache
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.put(hash, embedding.clone());
        }

        Ok(embedding)
    }

    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
        let mut results = Vec::with_capacity(texts.len());
        let mut uncached_indices = Vec::new();
        let mut uncached_texts = Vec::new();

        // Check cache for each text
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            for (i, text) in texts.iter().enumerate() {
                let hash = Self::hash_text(text);
                if let Some(cached) = cache.get(&hash) {
                    results.push(Some(cached.clone()));
                } else {
                    results.push(None);
                    uncached_indices.push(i);
                    uncached_texts.push(*text);
                }
            }
        }

        // Generate embeddings for uncached texts
        if !uncached_texts.is_empty() {
            let new_embeddings = self.inner.embed_batch(&uncached_texts).await?;

            // Store in cache and fill results
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            for (idx, embedding) in uncached_indices.iter().zip(new_embeddings.iter()) {
                let hash = Self::hash_text(texts[*idx]);
                cache.put(hash, embedding.clone());
                results[*idx] = Some(embedding.clone());
            }
        }

        Ok(results.into_iter().map(|r| r.unwrap_or_default()).collect())
    }

    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEmbeddingProvider;

    #[async_trait]
    impl EmbeddingProvider for MockEmbeddingProvider {
        async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError> {
            // Simple mock: return vector based on text length
            Ok(vec![text.len() as f32; 4])
        }

        fn dimensions(&self) -> usize {
            4
        }

        fn model_id(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn test_cache_hit() {
        let provider = CachedEmbeddingProvider::new(Box::new(MockEmbeddingProvider), 100);

        let emb1 = provider.embed("hello").await.unwrap();
        let emb2 = provider.embed("hello").await.unwrap();
        assert_eq!(emb1, emb2);

        let emb3 = provider.embed("hi").await.unwrap();
        assert_ne!(emb1, emb3);
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let provider = CachedEmbeddingProvider::new(Box::new(MockEmbeddingProvider), 2);

        provider.embed("a").await.unwrap();
        provider.embed("b").await.unwrap();
        provider.embed("c").await.unwrap(); // should evict "a"

        // "a" should be re-computed (different cache state, but still works)
        let emb = provider.embed("a").await.unwrap();
        assert_eq!(emb, vec![1.0; 4]);
    }

    #[tokio::test]
    async fn test_batch_with_cache() {
        let provider = CachedEmbeddingProvider::new(Box::new(MockEmbeddingProvider), 100);

        // First call - all uncached
        let texts = vec!["hello", "world", "foo"];
        let embs1 = provider.embed_batch(&texts).await.unwrap();

        // Second call - all cached
        let embs2 = provider.embed_batch(&texts).await.unwrap();
        assert_eq!(embs1, embs2);
    }
}
