use std::sync::Arc;

use async_trait::async_trait;

use agent_teams_core::agent_memory_cache::AgentMemoryCache;
use agent_teams_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_teams_core::memory::{MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::MemoryStore;
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};

/// Knowledge SubAgent: dedicated knowledge base query agent.
///
/// Responsibilities (ONLY):
/// - Answer user questions based on existing knowledge and historical memory
/// - Retrieve relevant context from memory store
/// - Provide factual, well-sourced answers
///
/// Does NOT do:
/// - Sentiment analysis (delegated to sentiment agent)
/// - Task complexity assessment (delegated to task_planner agent)
/// - Tool execution (delegated to tool_agent)
pub struct KnowledgeSubAgent {
    provider: Arc<dyn LlmProvider>,
    agent_memory_cache: AgentMemoryCache,
    thinking_config: Option<ThinkingConfig>,
    max_tokens: u32,
}

impl KnowledgeSubAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            agent_memory_cache: AgentMemoryCache::new("knowledge".to_string(), 100),
            thinking_config: None,
            max_tokens: 16384,
        }
    }

    pub fn with_thinking_config(mut self, config: Option<ThinkingConfig>) -> Self {
        self.thinking_config = config;
        self
    }

    pub fn with_agent_memory_cache(mut self, cache: AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[async_trait]
impl BoxedAgent for KnowledgeSubAgent {
    fn id(&self) -> &str {
        "knowledge"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["knowledge_query".to_string(), "user_input".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 80,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_memory_aware(&self) -> Option<&dyn MemoryAwareAgent> {
        Some(self)
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        // Query memory for relevant historical context
        let knowledge_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![
                MemoryKind::UserFact,
                MemoryKind::InferredPreference,
                MemoryKind::CrossSessionTopic,
            ],
            limit: 8,
            min_weight: 0.3,
            session_id: input.session_id.clone(),
            user_id: input.user_id.clone(),
            confirmed_only: true,
            ..Default::default()
        };
        let knowledge_memories = self.agent_memory_cache.query(&knowledge_query).await;

        // Also query for past knowledge results
        let result_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::AgentOutput],
            tags: vec!["knowledge_result".to_string()],
            limit: 3,
            session_id: input.session_id.clone(),
            confirmed_only: false,
            ..Default::default()
        };
        let past_results = self.agent_memory_cache.query(&result_query).await;

        // Build memory context
        let mut memory_sections = Vec::new();

        if !knowledge_memories.is_empty() {
            let items: Vec<String> = knowledge_memories
                .iter()
                .map(|m| {
                    let label = match m.kind {
                        MemoryKind::UserFact => "用户陈述",
                        MemoryKind::InferredPreference => "推断偏好",
                        MemoryKind::CrossSessionTopic => "历史主题",
                        _ => "其他",
                    };
                    format!("- [{}] {}", label, m.content)
                })
                .collect();
            memory_sections.push(format!("## 已确认的历史信息\n{}", items.join("\n")));
        }

        if !past_results.is_empty() {
            let items: Vec<String> = past_results
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect();
            memory_sections.push(format!("## 相关历史回答\n{}", items.join("\n")));
        }

        let memory_context = if memory_sections.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", memory_sections.join("\n\n"))
        };

        // Memory, system instructions, and domain state are already in system_prompt
        // from build_system_prompt() — no need to inject them again.

        let system = format!(
            r#"{system_prompt}

你是知识库专家。你的唯一职责：基于已有知识和历史记忆回答用户问题。

## 工作范围（严格限制）
- 回答用户的知识性问题
- 基于历史记忆提供上下文相关信息
- 解释概念、原理、事实

## 不属于你的工作（交给其他 Agent）
- 情感分析 → 交给 sentiment agent
- 任务规划和路由 → 交给 task_planner agent
- 工具调用和外部操作 → 交给 tool_agent
- 复杂度评估 → 交给 task_planner agent

## 回答原则
1. **基于事实**：只使用上下文中明确提供的信息
2. **诚实回答**：不确定就说不确定，不要装懂
3. **禁止编造**：不要捏造用户未提及的具体细节
4. **引用来源**：如果信息来自历史记忆，说明来源
5. **简洁明了**：直接回答问题，不要冗长的铺垫

## 回答格式
直接用自然语言回答，不要输出 JSON。如果需要引用历史信息，用括号标注来源。{memory_context}"#,
            system_prompt = input.system_prompt,
            memory_context = memory_context,
        );

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: input.content.clone(),
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.3),
            system: Some(system),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: self.thinking_config.clone(),
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Knowledge agent received empty content from LLM");
                    "抱歉，我无法回答这个问题。".to_string()
                } else {
                    resp.content
                };
                AgentOutput {
                    content,
                    thinking: resp.thinking,
                    quality: 0.85,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Knowledge query error: {}", e)),
        }
    }
}

#[async_trait]
impl MemoryAwareAgent for KnowledgeSubAgent {
    fn memory_cache(&self) -> &AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> agent_teams_core::error::Result<()> {
        // Cache knowledge query results for future reference
        if !output.content.is_empty() && output.quality > 0.5 {
            let entry = agent_teams_core::memory::MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: MemoryKind::AgentOutput,
                content: output.content.chars().take(500).collect(),
                data: None,
                embedding: None,
                weight: 0.5,
                created_at: chrono::Utc::now(),
                last_accessed_at: chrono::Utc::now(),
                access_count: 0,
                tags: vec!["knowledge_result".to_string()],
                source_agent: "knowledge".to_string(),
                confirmed: false,
                content_hash: Some(agent_teams_core::memory::compute_content_hash(
                    &output.content,
                )),
                confidence: 0.7,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            store.store(entry).await?;
        }

        self.agent_memory_cache.flush_all().await?;
        Ok(())
    }
}
