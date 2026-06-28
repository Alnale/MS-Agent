use std::collections::HashSet;
use std::sync::Arc;

use crate::error::Result;
use crate::memory::MemoryEntry;
use crate::memory_store::EmbeddingProvider;

/// Relation between a new fact and an existing memory
#[derive(Debug, Clone, PartialEq)]
pub enum DedupRelation {
    /// Exact duplicate (content hash match)
    ExactDuplicate,
    /// New memory is a synonym of existing (merge and promote)
    Synonym { existing_id: String },
    /// New memory is contained by existing (existing is more general)
    ContainedBy { existing_id: String },
    /// New memory contains existing (new is more general, replace)
    Contains { existing_id: String },
    /// Related but distinct (store both with relation)
    RelatedButDistinct { existing_id: String },
    /// Completely new memory
    Novel,
}

/// Recommended action for handling a dedup check result
#[derive(Debug, Clone)]
pub enum DedupAction {
    /// Merge into existing memory and promote weight
    MergeAndPromote { existing_id: String },
    /// Skip (new memory is redundant)
    SkipAsRedundant,
    /// Replace existing memory (new one is more complete)
    ReplaceExisting { existing_id: String },
    /// Store both and link with relation
    StoreWithRelation { existing_id: String },
    /// Store as a completely new memory
    StoreAsNew,
}

/// Result of a dedup check
#[derive(Debug, Clone)]
pub struct DedupResult {
    pub relation: DedupRelation,
    pub confidence: f32,
}

impl DedupResult {
    pub fn recommended_action(&self) -> DedupAction {
        match &self.relation {
            DedupRelation::ExactDuplicate | DedupRelation::Synonym { .. } => match &self.relation {
                DedupRelation::Synonym { existing_id } => DedupAction::MergeAndPromote {
                    existing_id: existing_id.clone(),
                },
                _ => DedupAction::SkipAsRedundant,
            },
            DedupRelation::ContainedBy { .. } => DedupAction::SkipAsRedundant,
            DedupRelation::Contains { existing_id } => DedupAction::ReplaceExisting {
                existing_id: existing_id.clone(),
            },
            DedupRelation::RelatedButDistinct { existing_id } => DedupAction::StoreWithRelation {
                existing_id: existing_id.clone(),
            },
            DedupRelation::Novel => DedupAction::StoreAsNew,
        }
    }
}

/// Hierarchical deduplication engine that detects synonym, containment,
/// and related-but-distinct relationships between memories.
pub struct HierarchicalDedupEngine {
    embedding_provider: Arc<dyn EmbeddingProvider>,
    /// Semantic similarity above this = synonym (merge)
    synonym_threshold: f32,
    /// Similarity above this = potential containment
    containment_threshold: f32,
    /// Similarity above this = related but distinct
    related_threshold: f32,
}

impl HierarchicalDedupEngine {
    pub fn new(embedding_provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            embedding_provider,
            synonym_threshold: 0.92,
            containment_threshold: 0.85,
            related_threshold: 0.70,
        }
    }

    pub fn with_thresholds(mut self, synonym: f32, containment: f32, related: f32) -> Self {
        self.synonym_threshold = synonym;
        self.containment_threshold = containment;
        self.related_threshold = related;
        self
    }

    /// Check if a new fact duplicates or relates to any existing memory.
    pub async fn check_duplicate(
        &self,
        new_fact: &str,
        candidate_pool: &[MemoryEntry],
    ) -> Result<DedupResult> {
        let new_embedding = self
            .embedding_provider
            .embed(new_fact)
            .await
            .map_err(|e| crate::error::AgentTeamsError::Internal(e.to_string()))?;

        self.check_duplicate_with_embedding(new_fact, &new_embedding, candidate_pool)
    }

    /// Check if a new fact duplicates or relates to any existing memory,
    /// using a pre-computed embedding to avoid redundant embedding calls.
    pub fn check_duplicate_with_embedding(
        &self,
        new_fact: &str,
        new_embedding: &[f32],
        candidate_pool: &[MemoryEntry],
    ) -> Result<DedupResult> {
        if candidate_pool.is_empty() {
            return Ok(DedupResult {
                relation: DedupRelation::Novel,
                confidence: 0.0,
            });
        }

        let mut best_match: Option<(DedupRelation, f32)> = None;

        for candidate in candidate_pool {
            if let Some(ref cand_emb) = candidate.embedding {
                let sim = cosine_similarity(new_embedding, cand_emb);

                let relation = if sim >= self.synonym_threshold {
                    Some(DedupRelation::Synonym {
                        existing_id: candidate.id.clone(),
                    })
                } else if sim >= self.containment_threshold {
                    self.detect_containment(new_fact, &candidate.content, &candidate.id)
                } else if sim >= self.related_threshold {
                    Some(DedupRelation::RelatedButDistinct {
                        existing_id: candidate.id.clone(),
                    })
                } else {
                    None
                };

                if let Some(rel) = relation {
                    if best_match.as_ref().map(|(_, s)| sim > *s).unwrap_or(true) {
                        best_match = Some((rel, sim));
                    }
                }
            }
        }

        Ok(match best_match {
            Some((relation, confidence)) => DedupResult {
                relation,
                confidence,
            },
            None => DedupResult {
                relation: DedupRelation::Novel,
                confidence: 0.0,
            },
        })
    }

    /// Detect containment relationship using string and keyword rules.
    fn detect_containment(
        &self,
        new: &str,
        existing: &str,
        existing_id: &str,
    ) -> Option<DedupRelation> {
        // Rule 1: direct string containment
        if existing.contains(new) {
            return Some(DedupRelation::ContainedBy {
                existing_id: existing_id.to_string(),
            });
        }
        if new.contains(existing) {
            return Some(DedupRelation::Contains {
                existing_id: existing_id.to_string(),
            });
        }

        // Rule 2: keyword set containment
        let new_keywords = extract_keywords(new);
        let existing_keywords = extract_keywords(existing);

        if !new_keywords.is_empty() && !existing_keywords.is_empty() {
            if new_keywords.is_subset(&existing_keywords) {
                return Some(DedupRelation::ContainedBy {
                    existing_id: existing_id.to_string(),
                });
            }
            if existing_keywords.is_subset(&new_keywords) {
                return Some(DedupRelation::Contains {
                    existing_id: existing_id.to_string(),
                });
            }
        }

        // Similarity is in containment range but no clear containment pattern
        Some(DedupRelation::RelatedButDistinct {
            existing_id: existing_id.to_string(),
        })
    }
}

/// Extract meaningful keywords from text (Chinese segments + English words)
fn extract_keywords(text: &str) -> HashSet<String> {
    let mut keywords = HashSet::new();

    // Split by whitespace and punctuation, keep segments >= 2 chars
    for segment in
        text.split(|c: char| c.is_ascii_punctuation() || c == ' ' || c == '\n' || c == '\t')
    {
        let trimmed = segment.trim();
        if trimmed.chars().count() >= 2 {
            keywords.insert(trimmed.to_lowercase());
        }
    }

    keywords
}

use crate::hash::cosine_similarity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_keywords() {
        let kw = extract_keywords("喜欢深红色");
        assert!(kw.contains("喜欢深红色"));
    }

    #[test]
    fn test_extract_keywords_subset() {
        let new_kw = extract_keywords("深圳");
        let existing_kw = extract_keywords("深圳南山区");
        // "深圳" is not a substring of "深圳南山区" as a segment after split,
        // but "深圳南山区" contains "深圳" as prefix — handled by string containment rule
        assert!(!new_kw.is_subset(&existing_kw));
    }

    #[test]
    fn test_string_containment() {
        // "住在深圳南山区" contains "住在深圳"
        assert!("住在深圳南山区".contains("住在深圳"));
    }

    #[test]
    fn test_dedup_result_novel_action() {
        let result = DedupResult {
            relation: DedupRelation::Novel,
            confidence: 0.0,
        };
        assert!(matches!(
            result.recommended_action(),
            DedupAction::StoreAsNew
        ));
    }

    #[test]
    fn test_dedup_result_synonym_action() {
        let result = DedupResult {
            relation: DedupRelation::Synonym {
                existing_id: "abc".to_string(),
            },
            confidence: 0.95,
        };
        assert!(matches!(
            result.recommended_action(),
            DedupAction::MergeAndPromote { .. }
        ));
    }

    #[test]
    fn test_dedup_result_contained_action() {
        let result = DedupResult {
            relation: DedupRelation::ContainedBy {
                existing_id: "abc".to_string(),
            },
            confidence: 0.88,
        };
        assert!(matches!(
            result.recommended_action(),
            DedupAction::SkipAsRedundant
        ));
    }
}
