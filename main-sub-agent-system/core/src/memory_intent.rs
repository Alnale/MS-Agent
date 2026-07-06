use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;

use crate::memory::MemoryKind;
use crate::provider::LlmProvider;

/// Query intent classification for memory retrieval
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryIntent {
    /// Asking about historical facts ("what did I say yesterday")
    HistoricalRecall,
    /// Asking about user preferences ("what do I like")
    PreferenceQuery,
    /// Task continuation ("continue from where we left off")
    TaskContinuation,
    /// Fact verification ("didn't I say that...")
    FactVerification,
    /// General query
    General,
}

/// Intent recognizer: classifies query intent using rules + optional LLM
pub struct IntentRecognizer {
    llm_provider: Option<Arc<dyn LlmProvider>>,
    rules: Vec<(Regex, QueryIntent)>,
}

impl IntentRecognizer {
    pub fn new(llm_provider: Option<Arc<dyn LlmProvider>>) -> Self {
        let rules = vec![
            // Historical recall patterns
            (
                Regex::new(
                    r"(?i)(昨天|上次|之前|以前|earlier|yesterday|last time|before|历史|说了什么)",
                )
                .expect("invalid historical recall regex"),
                QueryIntent::HistoricalRecall,
            ),
            // Preference query patterns
            (
                Regex::new(r"(?i)(喜欢|偏好|prefer|favorite|爱好|习惯|口味|风格)")
                    .expect("invalid preference query regex"),
                QueryIntent::PreferenceQuery,
            ),
            // Task continuation patterns
            (
                Regex::new(r"(?i)(继续|接着|刚才|continue|resume|go on|下一步)")
                    .expect("invalid task continuation regex"),
                QueryIntent::TaskContinuation,
            ),
            // Fact verification patterns
            (
                Regex::new(r"(?i)(是不是|对不对|确认|verify|confirm|不是说|应该)")
                    .expect("invalid fact verification regex"),
                QueryIntent::FactVerification,
            ),
        ];

        Self {
            llm_provider,
            rules,
        }
    }

    /// Recognize query intent
    pub async fn recognize(&self, query: &str) -> QueryIntent {
        // 1. Rule matching (fast path)
        for (regex, intent) in &self.rules {
            if regex.is_match(query) {
                return intent.clone();
            }
        }

        // 2. LLM classification (if available)
        if let Some(llm) = &self.llm_provider {
            match self.llm_classify(query, llm).await {
                Ok(intent) => return intent,
                Err(e) => tracing::warn!("Intent classification failed: {}", e),
            }
        }

        QueryIntent::General
    }

    async fn llm_classify(
        &self,
        query: &str,
        llm: &Arc<dyn LlmProvider>,
    ) -> Result<QueryIntent, String> {
        let system = "你是一个查询意图分类器。根据用户消息判断查询意图类型。\n\
            返回以下之一：\n\
            - HistoricalRecall: 询问历史事实\n\
            - PreferenceQuery: 询问用户偏好\n\
            - TaskContinuation: 任务续作\n\
            - FactVerification: 事实确认\n\
            - General: 一般性查询\n\n\
            只返回意图类型名称，不要其他内容。"
            .to_string();

        let request = crate::provider::CompletionRequest {
            messages: vec![crate::provider::ChatMessage::simple("user", query)],
            max_tokens: Some(1024),
            temperature: Some(0.0),
            system: Some(system),
            ..Default::default()
        };

        let resp = llm.complete(request).await.map_err(|e| e.to_string())?;
        let content = resp.content.trim().to_string();

        match content.as_str() {
            "HistoricalRecall" => Ok(QueryIntent::HistoricalRecall),
            "PreferenceQuery" => Ok(QueryIntent::PreferenceQuery),
            "TaskContinuation" => Ok(QueryIntent::TaskContinuation),
            "FactVerification" => Ok(QueryIntent::FactVerification),
            _ => Ok(QueryIntent::General),
        }
    }
}

/// Dynamic ranking configuration per intent
pub struct RankingConfig {
    pub semantic_weight: f32,
    pub recency_weight: f32,
    pub access_weight: f32,
    pub weight_factor: f32,
    pub confirmed_bonus: f32,
    pub intent_boosts: HashMap<MemoryKind, f32>,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            semantic_weight: 0.40,
            recency_weight: 0.20,
            access_weight: 0.15,
            weight_factor: 0.15,
            confirmed_bonus: 0.10,
            intent_boosts: HashMap::new(),
        }
    }
}

impl RankingConfig {
    /// Get ranking config for a specific query intent
    pub fn for_intent(intent: &QueryIntent) -> Self {
        match intent {
            QueryIntent::HistoricalRecall => Self {
                semantic_weight: 0.10,
                recency_weight: 0.60,
                access_weight: 0.10,
                weight_factor: 0.10,
                confirmed_bonus: 0.10,
                intent_boosts: [(MemoryKind::DialogueTurn, 2.0)].into(),
            },
            QueryIntent::PreferenceQuery => Self {
                semantic_weight: 0.30,
                recency_weight: 0.10,
                access_weight: 0.10,
                weight_factor: 0.30,
                confirmed_bonus: 0.20,
                intent_boosts: [
                    (MemoryKind::UserFact, 1.5),
                    (MemoryKind::UserProfile, 2.0),
                    (MemoryKind::InferredPreference, 1.3),
                ]
                .into(),
            },
            QueryIntent::TaskContinuation => Self {
                semantic_weight: 0.15,
                recency_weight: 0.55,
                access_weight: 0.10,
                weight_factor: 0.10,
                confirmed_bonus: 0.10,
                intent_boosts: [(MemoryKind::Summary, 2.0), (MemoryKind::DialogueTurn, 1.5)].into(),
            },
            QueryIntent::FactVerification => Self {
                semantic_weight: 0.50,
                recency_weight: 0.10,
                access_weight: 0.10,
                weight_factor: 0.20,
                confirmed_bonus: 0.10,
                intent_boosts: [(MemoryKind::UserFact, 2.0)].into(),
            },
            QueryIntent::General => Self::default(),
        }
    }
}
