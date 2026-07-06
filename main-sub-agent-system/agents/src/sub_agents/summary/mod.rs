pub mod chain_optimizer;
pub mod global_summary;
pub mod quality_inspector;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

use agent_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_core::effect::AgentEffect;
use agent_core::memory::{
    compute_content_hash, MemoryEntry, MemoryKind, MemoryQuery, MemoryRelation, MemoryRelationType,
};
use agent_core::memory_store::MemoryStore;
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider};

const MAX_HISTORY_CHARS: usize = 4000;
const MAX_TURNS_FOR_SUMMARY: usize = 20;

/// Operating mode for the Summary Sub Agent
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SummaryAgentMode {
    /// Standalone mode (for testing/debug)
    Standalone,
    /// Embedded in background service (default production mode)
    #[default]
    BackgroundService,
}

/// Structured output from Summary Sub Agent
struct StructuredSummaryOutput {
    summary: String,
    facts: Vec<String>,
    quality_score: f32,
}

/// Summary SubAgent: generates conversation summaries with sliding window
/// Supports memory-aware incremental mode and bidirectional memory sync
pub struct SummarySubAgent {
    provider: Arc<dyn LlmProvider>,
    /// Optional memory store for reading existing summaries (incremental mode)
    memory_store: Option<Arc<dyn MemoryStore>>,
    /// Embedding provider for generating vectors
    embedding_provider: Option<Arc<dyn agent_core::memory_store::EmbeddingProvider>>,
    /// Operating mode
    mode: SummaryAgentMode,
    /// Agent-local memory cache (always available)
    agent_memory_cache: agent_core::AgentMemoryCache,
    thinking_config: Option<agent_core::provider::ThinkingConfig>,
}

impl SummarySubAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            memory_store: None,
            embedding_provider: None,
            mode: SummaryAgentMode::default(),
            agent_memory_cache: agent_core::AgentMemoryCache::new("summary".to_string(), 100),
            thinking_config: None,
        }
    }

    pub fn with_thinking_config(mut self, config: Option<agent_core::provider::ThinkingConfig>) -> Self {
        self.thinking_config = config;
        self
    }

    /// Set the operating mode
    pub fn with_mode(mut self, mode: SummaryAgentMode) -> Self {
        self.mode = mode;
        self
    }

    /// Enable memory-aware mode: the agent can read existing summaries
    pub fn with_memory_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    /// Set the agent-local memory cache with custom configuration
    pub fn with_agent_memory_cache(mut self, cache: agent_core::AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
        self
    }

    /// Set embedding provider for vector generation
    pub fn with_embedding_provider(
        mut self,
        provider: Arc<dyn agent_core::memory_store::EmbeddingProvider>,
    ) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Build history text with sliding window — truncate older turns if too long
    fn build_history_text(history: &[serde_json::Value], _current_msg: &str) -> String {
        if history.is_empty() {
            return String::new();
        }

        let recent: Vec<String> = history
            .iter()
            .rev()
            .take(MAX_TURNS_FOR_SUMMARY)
            .map(|h| {
                let text = h.to_string();
                // Truncate individual turns if very long
                if text.len() > 500 {
                    let mut boundary = 500;
                    while boundary > 0 && !text.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    format!("{}...", &text[..boundary])
                } else {
                    text
                }
            })
            .collect();

        let mut result = recent.join("\n");

        // If still too long, keep only recent turns that fit
        if result.len() > MAX_HISTORY_CHARS {
            let mut trimmed = Vec::new();
            let mut total = 0;
            for turn in recent {
                if total + turn.len() + 1 > MAX_HISTORY_CHARS && !trimmed.is_empty() {
                    break;
                }
                total += turn.len() + 1;
                trimmed.push(turn);
            }
            trimmed.reverse();
            result = trimmed.join("\n");
        }

        result
    }

    /// Load memory context from memory store for incremental mode
    async fn load_memory_context(&self, session_id: &str, _query: &str) -> String {
        let store = match self.memory_store.as_ref() {
            Some(s) => s,
            None => return String::new(),
        };

        // Load existing summaries
        let summaries = store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                session_id: Some(session_id.to_string()),
                limit: 3,
                min_weight: 0.0,
                ..Default::default()
            })
            .await
            .unwrap_or_else(|_| agent_core::memory::MemoryRetrievalResult {
                entries: Vec::new(),
                total_available: 0,
            });

        if summaries.entries.is_empty() {
            return String::new();
        }

        let summaries_text: String = summaries
            .entries
            .iter()
            .map(|e| format!("- {}", e.content))
            .collect::<Vec<_>>()
            .join("\n");

        format!("已有摘要：\n{}", summaries_text)
    }

    /// Build enhanced prompt with memory context
    fn build_enhanced_prompt(
        input: &AgentInput,
        memory_context: &str,
        history_text: &str,
    ) -> String {
        let mut parts = Vec::new();

        if !memory_context.is_empty() {
            parts.push(memory_context.to_string());
        }

        if !history_text.is_empty() {
            parts.push(format!("对话历史：\n{}", history_text));
        }

        parts.push(format!("当前消息：{}", input.content));
        parts.join("\n\n")
    }

    /// Parse structured output from LLM response
    fn parse_structured_output(content: &str) -> StructuredSummaryOutput {
        let mut facts = Vec::new();
        let mut summary = String::new();
        let mut quality_score = 0.8;
        let mut in_facts = false;
        let mut in_summary = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Try to parse JSON first
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if let Some(s) = json["summary"].as_str() {
                        summary = s.to_string();
                    }
                    if let Some(arr) = json["facts"].as_array() {
                        facts = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                    }
                    if let Some(q) = json["quality"].as_f64() {
                        quality_score = q as f32;
                    }
                    if !summary.is_empty() {
                        return StructuredSummaryOutput {
                            summary,
                            facts,
                            quality_score,
                        };
                    }
                }
            }

            // Fallback: parse text format
            if trimmed.starts_with("FACTS:") {
                in_facts = true;
                in_summary = false;
                continue;
            }
            if trimmed.starts_with("SUMMARY:") {
                in_facts = false;
                in_summary = true;
                continue;
            }
            if trimmed.starts_with("QUALITY:") {
                if let Some(q) = trimmed
                    .strip_prefix("QUALITY:")
                    .and_then(|s| s.trim().parse::<f32>().ok())
                {
                    quality_score = q;
                }
                continue;
            }

            if in_facts {
                if let Some(fact) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("• "))
                {
                    facts.push(fact.to_string());
                }
            }
            if in_summary && !trimmed.is_empty() {
                if !summary.is_empty() {
                    summary.push(' ');
                }
                summary.push_str(trimmed);
            }
        }

        StructuredSummaryOutput {
            summary,
            facts,
            quality_score,
        }
    }

    /// Sync summary and facts to memory system
    async fn sync_to_memory(
        &self,
        session_id: &str,
        output: &StructuredSummaryOutput,
    ) -> Vec<AgentEffect> {
        let store = match self.memory_store.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut effects = Vec::new();

        // Store facts with relations
        for fact in &output.facts {
            let embedding = if let Some(ref emb) = self.embedding_provider {
                emb.embed(fact).await.ok()
            } else {
                None
            };

            let fact_entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::UserFact,
                content: fact.clone(),
                data: None,
                embedding,
                weight: 0.8,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["fact".to_string(), "from_summary".to_string()],
                source_agent: "summary".to_string(),
                confirmed: true,
                content_hash: Some(compute_content_hash(fact)),
                confidence: output.quality_score,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };

            if let Err(e) = store.store(fact_entry).await {
                tracing::warn!("Failed to store fact from summary: {}", e);
            }

            effects.push(AgentEffect::MemoryUpdate {
                key: "user_fact".to_string(),
                value: json!({ "fact": fact, "quality": output.quality_score }),
                agent_id: "summary".to_string(),
            });
        }

        // Store summary with relation to previous summaries
        if !output.summary.is_empty() {
            let embedding = if let Some(ref emb) = self.embedding_provider {
                emb.embed(&output.summary).await.ok()
            } else {
                None
            };

            let summary_id = uuid::Uuid::new_v4().to_string();
            let summary_entry = MemoryEntry {
                id: summary_id.clone(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::Summary,
                content: output.summary.clone(),
                data: Some(json!({
                    "facts_count": output.facts.len(),
                    "quality": output.quality_score,
                })),
                embedding,
                weight: 0.6,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["compressed".to_string()],
                source_agent: "summary".to_string(),
                confirmed: false,
                content_hash: Some(compute_content_hash(&output.summary)),
                confidence: output.quality_score,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };

            // Find existing summaries to create relations
            let existing_summaries = store
                .retrieve(MemoryQuery {
                    kinds: vec![MemoryKind::Summary],
                    session_id: Some(session_id.to_string()),
                    limit: 3,
                    min_weight: 0.0,
                    ..Default::default()
                })
                .await
                .unwrap_or_else(|_| agent_core::memory::MemoryRetrievalResult {
                    entries: Vec::new(),
                    total_available: 0,
                });

            // Create relations to existing summaries
            let relations: Vec<MemoryRelation> = existing_summaries
                .entries
                .iter()
                .map(|prev| MemoryRelation {
                    source_id: summary_id.clone(),
                    target_id: prev.id.clone(),
                    relation_type: MemoryRelationType::Related,
                    strength: 0.7,
                    created_at: Utc::now(),
                })
                .collect();

            // Store summary with relations (transactional)
            if let Err(e) = store.store_with_relations(summary_entry, relations).await {
                tracing::warn!("Failed to store summary with relations: {}", e);
            }

            // Update quality of existing summaries
            for prev in &existing_summaries.entries {
                if prev.confidence < output.quality_score {
                    let _ = store
                        .update_quality(&prev.id, output.quality_score, "summary")
                        .await;
                }
            }

            effects.push(AgentEffect::MemoryUpdate {
                key: "session_summary".to_string(),
                value: json!({ "summary": output.summary, "facts": output.facts, "quality": output.quality_score }),
                agent_id: "summary".to_string(),
            });
        }

        effects
    }

    /// Extract session_id from text (look for "session_id: xxx" pattern)
    fn extract_session_id(text: &str) -> Option<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("session_id:") {
                return Some(rest.trim().to_string());
            }
            if let Some(rest) = trimmed.strip_prefix("session_id: ") {
                return Some(rest.trim().to_string());
            }
        }
        None
    }
}

#[async_trait]
impl BoxedAgent for SummarySubAgent {
    fn id(&self) -> &str {
        "summary"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["conversation_summary".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 50,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_memory_aware(&self) -> Option<&dyn agent_core::boxed_agent::MemoryAwareAgent> {
        Some(self)
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        // Use session_id from AgentInput directly, fall back to text extraction
        let session_id = input.session_id.clone()
            .or_else(|| Self::extract_session_id(&input.system_prompt))
            .or_else(|| Self::extract_session_id(&input.content));

        // In background service mode, session_id is required
        if self.mode == SummaryAgentMode::BackgroundService && session_id.is_none() {
            return AgentOutput::error(
                "BackgroundService mode requires session_id in input".to_string(),
            );
        }

        let session_id = session_id.unwrap_or_else(|| "default".to_string());

        // 1. Load memory context for incremental mode
        let memory_context = self.load_memory_context(&session_id, &input.content).await;

        // 2. Build history text
        let history_text = Self::build_history_text(&input.recent_history, &input.content);

        // 3. Build enhanced prompt with memory context
        let enhanced_content = Self::build_enhanced_prompt(&input, &memory_context, &history_text);

        let system = format!(
            r#"{system_prompt}

你是对话压缩专家。你的任务是从对话历史中提取最关键的信息，压缩成结构化摘要。

## 提取规则

### 摘要（summary）
- 不超过 200 字
- 保留：重要结论、关键决策、未完成的任务、用户的核心诉求
- 丢弃：寒暄、重复内容、过程性讨论、已解决的细节

### 事实提取（facts）
- 用户明确表达的偏好（"我喜欢..."、"我不想要..."）
- 用户提供的具体信息（地点、时间、人名、数字）
- 做出的决定和承诺（"我们决定..."、"我会..."）
- 未解决的问题（"还需要确认..."）
- **不要提取**：通用知识、显而易见的事实、agent 自己的输出

### 质量评估（quality）
- 0.9-1.0：对话内容丰富，提取到 3+ 有价值的事实
- 0.7-0.9：正常对话，提取到 1-2 个事实
- 0.5-0.7：对话较少或主要是闲聊
- 0.0-0.5：几乎没有有价值的信息

## 输出格式

```json
{{
  "summary": "压缩后的摘要",
  "facts": ["事实1", "事实2"],
  "quality": 0.8
}}
```"#,
            system_prompt = input.system_prompt
        );

        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", enhanced_content)],
            max_tokens: Some(32768),
            temperature: Some(0.2),
            system: Some(system),
            thinking: self.thinking_config.clone(),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Summary agent received empty content from LLM");
                    return AgentOutput {
                        content: "无法生成摘要。".to_string(),
                        quality: 0.0,
                        ..Default::default()
                    };
                } else {
                    resp.content
                };

                // 4. Parse structured output
                let structured = Self::parse_structured_output(&content);

                // 5. Sync to memory system (bidirectional data flow)
                let effects = self.sync_to_memory(&session_id, &structured).await;

                // Use summary as the output content
                let output_content = if structured.summary.is_empty() {
                    content.clone()
                } else {
                    structured.summary.clone()
                };

                AgentOutput {
                    content: output_content,
                    thinking: resp.thinking,
                    quality: structured.quality_score,
                    effects,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Summary error: {}", e)),
        }
    }
}

#[async_trait]
impl MemoryAwareAgent for SummarySubAgent {
    fn memory_cache(&self) -> &agent_core::AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> agent_core::error::Result<()> {
        // Parse the output content as structured summary
        let structured = Self::parse_structured_output(&output.content);

        // Use the provided store for sync
        // Build facts and summary entries
        for fact in &structured.facts {
            let embedding = if let Some(ref emb) = self.embedding_provider {
                emb.embed(fact).await.ok()
            } else {
                None
            };

            let fact_entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::UserFact,
                content: fact.clone(),
                data: None,
                embedding,
                weight: 0.8,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["fact".to_string(), "from_summary".to_string()],
                source_agent: "summary".to_string(),
                confirmed: true,
                content_hash: Some(compute_content_hash(fact)),
                confidence: structured.quality_score,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            store.store(fact_entry).await?;
        }

        if !structured.summary.is_empty() {
            let embedding = if let Some(ref emb) = self.embedding_provider {
                emb.embed(&structured.summary).await.ok()
            } else {
                None
            };

            let summary_entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::Summary,
                content: structured.summary.clone(),
                data: Some(json!({
                    "facts_count": structured.facts.len(),
                    "quality": structured.quality_score,
                })),
                embedding,
                weight: 0.6,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
                tags: vec!["compressed".to_string()],
                source_agent: "summary".to_string(),
                confirmed: false,
                content_hash: Some(compute_content_hash(&structured.summary)),
                confidence: structured.quality_score,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            store.store(summary_entry).await?;
        }

        Ok(())
    }

    async fn load_memory_context(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        _query: &str,
    ) -> agent_core::error::Result<String> {
        let summaries = store
            .retrieve(MemoryQuery {
                kinds: vec![MemoryKind::Summary],
                session_id: Some(session_id.to_string()),
                limit: 3,
                min_weight: 0.0,
                ..Default::default()
            })
            .await?;

        if summaries.entries.is_empty() {
            return Ok(String::new());
        }

        let summaries_text: String = summaries
            .entries
            .iter()
            .map(|e| format!("- {}", e.content))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!("已有摘要：\n{}", summaries_text))
    }
}
