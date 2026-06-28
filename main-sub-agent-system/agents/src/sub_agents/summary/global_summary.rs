use std::collections::HashMap;
use std::sync::Arc;

use agent_teams_core::error::Result;
use agent_teams_core::memory::{MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::{EmbeddingProvider, MemoryStore};

/// A domain-level summary of user knowledge/preferences
#[derive(Debug, Clone)]
pub struct DomainSummary {
    pub domain_name: String,
    pub summary: String,
    pub fact_count: usize,
}

/// User's cognitive map: domain-organized view of all knowledge
#[derive(Debug, Clone)]
pub struct CognitiveMap {
    pub user_id: String,
    pub domains: Vec<DomainSummary>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// Builds a global cognitive map for a user by clustering cross-session
/// facts and topics into domain-organized summaries.
pub struct GlobalSummaryBuilder {
    memory_store: Arc<dyn MemoryStore>,
}

impl GlobalSummaryBuilder {
    pub fn new(
        memory_store: Arc<dyn MemoryStore>,
        _embedding_provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self {
            memory_store,
        }
    }

    /// Build a user-level cognitive map from all cross-session knowledge
    pub async fn build_user_cognitive_map(&self, user_id: &str) -> Result<CognitiveMap> {
        // 1. Get cross-session topics
        let topics = self
            .memory_store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::CrossSessionTopic],
                limit: 50,
                min_weight: 0.3,
                user_id: Some(user_id.to_string()),
                ..Default::default()
            })
            .await?;

        // 2. Get all user facts
        let facts = self
            .memory_store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::UserFact, MemoryKind::InferredPreference],
                limit: 100,
                min_weight: 0.3,
                user_id: Some(user_id.to_string()),
                ..Default::default()
            })
            .await?;

        // 3. Simple clustering by tag similarity
        let mut domain_map: HashMap<String, Vec<String>> = HashMap::new();

        for fact in &facts.entries {
            let domain = self.infer_domain(fact);
            domain_map
                .entry(domain)
                .or_default()
                .push(fact.content.clone());
        }

        for topic in &topics.entries {
            let domain = "跨会话主题".to_string();
            domain_map
                .entry(domain)
                .or_default()
                .push(topic.content.clone());
        }

        // 4. Build domain summaries
        let domains: Vec<DomainSummary> = domain_map
            .into_iter()
            .map(|(name, contents)| {
                let summary = if contents.len() <= 3 {
                    contents.join("; ")
                } else {
                    format!("{} (共{}项)", contents[..3].join("; "), contents.len())
                };
                DomainSummary {
                    domain_name: name,
                    summary,
                    fact_count: contents.len(),
                }
            })
            .collect();

        Ok(CognitiveMap {
            user_id: user_id.to_string(),
            domains,
            last_updated: chrono::Utc::now(),
        })
    }

    /// Build a prompt string from the cognitive map for injection into working memory
    pub fn build_global_context_prompt(&self, map: &CognitiveMap) -> String {
        if map.domains.is_empty() {
            return String::new();
        }

        let mut sections = vec!["## 用户全局认知图谱".to_string()];

        for domain in &map.domains {
            sections.push(format!(
                "### {} ({}项)\n{}",
                domain.domain_name, domain.fact_count, domain.summary
            ));
        }

        sections.join("\n\n")
    }

    /// Infer domain from a memory entry based on tags and content
    fn infer_domain(&self, entry: &agent_teams_core::memory::MemoryEntry) -> String {
        // Use tags if available
        for tag in &entry.tags {
            if tag != "fact" && tag != "from_summary" && tag != "compressed" {
                return tag.clone();
            }
        }

        // Fallback: use kind-based domain
        match entry.kind {
            MemoryKind::UserFact => "用户事实".to_string(),
            MemoryKind::InferredPreference => "偏好推断".to_string(),
            MemoryKind::CrossSessionTopic => "跨会话主题".to_string(),
            _ => "其他".to_string(),
        }
    }
}
