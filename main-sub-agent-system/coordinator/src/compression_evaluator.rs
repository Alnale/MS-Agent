use std::sync::Arc;

use agent_teams_core::error::Result;
use agent_teams_core::memory::{MemoryEntry, MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::{EmbeddingProvider, MemoryStore};

/// Evaluates compression quality by measuring information retention
pub struct CompressionEvaluator {
    embedding_provider: Arc<dyn EmbeddingProvider>,
    /// Optional memory store for retrieval effectiveness evaluation
    memory_store: Option<Arc<dyn MemoryStore>>,
}

impl CompressionEvaluator {
    pub fn new(embedding_provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            embedding_provider,
            memory_store: None,
        }
    }

    /// Enable memory-aware evaluation (adds retrieval effectiveness scoring)
    pub fn with_memory_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    /// Evaluate compression quality, returns retention rate (0.0-1.0)
    pub async fn evaluate(
        &self,
        original_turns: &[MemoryEntry],
        extracted_facts: &[MemoryEntry],
        summary: &str,
    ) -> Result<f32> {
        if original_turns.is_empty() {
            return Ok(1.0);
        }

        // 1. Semantic coverage: similarity between original dialogue and summary
        let original_text: String = original_turns
            .iter()
            .map(|t| t.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let semantic_coverage = if !summary.is_empty() {
            match (
                self.embedding_provider.embed(&original_text).await,
                self.embedding_provider.embed(summary).await,
            ) {
                (Ok(orig_emb), Ok(summ_emb)) => cosine_similarity(&orig_emb, &summ_emb),
                _ => 0.5,
            }
        } else {
            0.0
        };

        // 2. Fact extraction rate: key phrases covered by extracted facts
        let fact_coverage = if !extracted_facts.is_empty() {
            let key_phrases = extract_key_phrases(&original_text);
            if key_phrases.is_empty() {
                1.0
            } else {
                let extracted_text: String = extracted_facts
                    .iter()
                    .map(|f| f.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let covered = key_phrases
                    .iter()
                    .filter(|phrase| {
                        extracted_text
                            .to_lowercase()
                            .contains(&phrase.to_lowercase())
                    })
                    .count();
                covered as f32 / key_phrases.len() as f32
            }
        } else {
            0.0
        };

        // 3. Composite score
        let retention_rate = semantic_coverage * 0.6 + fact_coverage * 0.4;
        Ok(retention_rate)
    }

    /// Comprehensive evaluation: semantic coverage + memory retrieval effectiveness
    pub async fn evaluate_with_memory_impact(
        &self,
        session_id: &str,
        original_turns: &[MemoryEntry],
        extracted_facts: &[MemoryEntry],
        summary: &str,
    ) -> Result<f32> {
        // 1. Semantic coverage (existing)
        let semantic_score = self
            .evaluate(original_turns, extracted_facts, summary)
            .await?;

        // 2. Memory retrieval effectiveness (new)
        let retrieval_score = if let Some(ref store) = self.memory_store {
            self.evaluate_retrieval_effectiveness(store, session_id, extracted_facts)
                .await?
        } else {
            0.5
        };

        // 3. Fact consistency (new)
        let consistency_score = self.evaluate_fact_consistency(extracted_facts).await?;

        Ok(semantic_score * 0.5 + retrieval_score * 0.3 + consistency_score * 0.2)
    }

    /// Evaluate whether extracted facts can be effectively retrieved from memory
    async fn evaluate_retrieval_effectiveness(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        facts: &[MemoryEntry],
    ) -> Result<f32> {
        if facts.is_empty() {
            return Ok(1.0);
        }

        let mut total_score = 0.0;
        for fact in facts {
            // Try to retrieve using fact content as query
            let results = store
                .retrieve(MemoryQuery {
                    text: fact.content.clone(),
                    kinds: vec![MemoryKind::UserFact],
                    session_id: Some(session_id.to_string()),
                    limit: 5,
                    min_weight: 0.0,
                    ..Default::default()
                })
                .await?;

            // Check if the fact can be found
            let found = results.entries.iter().any(|e| e.content == fact.content);
            total_score += if found { 1.0 } else { 0.0 };
        }

        Ok(total_score / facts.len() as f32)
    }

    /// Evaluate consistency among extracted facts (detect contradictions)
    async fn evaluate_fact_consistency(&self, facts: &[MemoryEntry]) -> Result<f32> {
        if facts.len() <= 1 {
            return Ok(1.0);
        }

        let mut contradiction_count = 0;
        let total_pairs = facts.len() * (facts.len() - 1) / 2;

        // Check each pair for semantic similarity (potential contradictions)
        for i in 0..facts.len() {
            for j in (i + 1)..facts.len() {
                if let (Some(ref emb_i), Some(ref emb_j)) =
                    (&facts[i].embedding, &facts[j].embedding)
                {
                    let sim = cosine_similarity(emb_i, emb_j);
                    // High similarity but different content = potential contradiction
                    if sim > 0.8 && facts[i].content != facts[j].content {
                        contradiction_count += 1;
                    }
                }
            }
        }

        if total_pairs == 0 {
            return Ok(1.0);
        }

        Ok(1.0 - (contradiction_count as f32 / total_pairs as f32))
    }
}

use agent_teams_core::cosine_similarity;

/// Extract key phrases from text (numbers, proper nouns, important terms)
fn extract_key_phrases(text: &str) -> Vec<String> {
    let mut phrases = Vec::new();

    // Extract numbers
    for word in text.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        if clean.chars().any(|c| c.is_numeric()) && clean.len() > 1 {
            phrases.push(clean);
        }
    }

    // Extract Chinese phrases > 2 chars that appear to be important
    for segment in text.split(|c: char| c.is_ascii_punctuation() || c == ' ' || c == '\n') {
        let trimmed = segment.trim();
        if trimmed.chars().count() >= 2 && trimmed.chars().count() <= 10 {
            // Check if it contains Chinese characters
            if trimmed.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
                phrases.push(trimmed.to_string());
            }
        }
    }

    phrases.dedup();
    phrases.truncate(20);
    phrases
}
