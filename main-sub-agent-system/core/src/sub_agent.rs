use serde::{Deserialize, Serialize};

use crate::boxed_agent::AgentCapabilities;
use crate::boxed_agent::{AgentInput, AgentOutput};
use crate::memory::{MemoryKind, MemoryQuery};
use crate::registry::SharedAgent;

/// Descriptor for a registered SubAgent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentDescriptor {
    pub id: String,
    pub capabilities: AgentCapabilities,
    pub expertise: String,
    pub available_tools: Vec<String>,
    pub depends_on: Vec<String>,
    pub priority: i32,
    pub fallback_agent_id: Option<String>,
    pub optional: bool,
    pub default_effects: Vec<serde_json::Value>,
    pub version: Option<AgentVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVersion {
    pub version: String,
    pub changelog: String,
    pub min_framework_version: String,
}

/// SubAgent runner — wraps a BoxedAgent with context injection
pub struct SubAgentRunner {
    inner: SharedAgent,
    descriptor: SubAgentDescriptor,
}

impl SubAgentRunner {
    pub fn new(inner: SharedAgent, descriptor: SubAgentDescriptor) -> Self {
        Self { inner, descriptor }
    }

    pub fn descriptor(&self) -> &SubAgentDescriptor {
        &self.descriptor
    }

    pub fn agent_id(&self) -> &str {
        &self.descriptor.id
    }

    /// Execute the SubAgent with injected context
    /// Automatically loads relevant memories before execution and syncs output after execution
    /// if the agent implements MemoryAwareAgent.
    pub async fn execute(&self, mut input: AgentInput) -> AgentOutput {
        // Inject SubAgent-specific prompt
        input.system_prompt.push_str(&format!(
            "\n\n你是 SubAgent [{}]，擅长领域：{}。请根据你的专长完成分配给你的任务。",
            self.descriptor.id, self.descriptor.expertise
        ));

        // Inject tool info if available
        if !self.descriptor.available_tools.is_empty() {
            let tools_desc = self.descriptor.available_tools.join(", ");
            input
                .system_prompt
                .push_str(&format!("\n可用工具: {}", tools_desc));
        }

        // If agent is MemoryAwareAgent, load relevant memories into prompt
        // Use confirmed_only=true to prevent unverified agent outputs from being treated as facts
        if let Some(memory_aware) = self.inner.as_memory_aware() {
            let cache = memory_aware.memory_cache();
            let query = MemoryQuery {
                text: input.content.clone(),
                kinds: vec![MemoryKind::UserFact, MemoryKind::CrossSessionTopic],
                limit: 5,
                session_id: input.session_id.clone(),
                confirmed_only: true,
                ..Default::default()
            };
            let memories = cache.query(&query).await;
            if !memories.is_empty() {
                let memory_text = memories
                    .iter()
                    .map(|m| {
                        let source_label = match m.kind {
                            MemoryKind::UserFact => "用户陈述",
                            MemoryKind::CrossSessionTopic => "历史主题",
                            _ => "其他",
                        };
                        format!("- [{}] {}", source_label, m.content)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                input
                    .system_prompt
                    .push_str(&format!("\n\n## 已确认的相关记忆\n{}", memory_text));
            }
        }

        // Anti-hallucination grounding instruction
        input.system_prompt.push_str(
            "\n\n**重要约束：** 只使用上下文中明确提供的信息。不要编造用户未提及的具体细节。\
             如果信息不足，坦诚说明而不是虚构。"
        );

        // Execute the agent
        let output = self.inner.run(input).await;

        // If agent is MemoryAwareAgent, auto-sync output to memory cache
        if let Some(memory_aware) = self.inner.as_memory_aware() {
            let cache = memory_aware.memory_cache();
            if !output.content.is_empty() && output.quality > 0.3 {
                let entry = crate::memory::MemoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: None,
                    kind: MemoryKind::AgentOutput,
                    content: output.content.chars().take(500).collect(),
                    data: Some(serde_json::json!({
                        "agent_id": self.descriptor.id,
                        "quality": output.quality,
                    })),
                    embedding: None,
                    weight: output.quality * 0.5,
                    created_at: chrono::Utc::now(),
                    last_accessed_at: chrono::Utc::now(),
                    access_count: 0,
                    tags: vec![self.descriptor.id.clone(), "agent_output".to_string()],
                    source_agent: self.descriptor.id.clone(),
                    confirmed: false,
                    content_hash: Some(crate::memory::compute_content_hash(&output.content)),
                    confidence: output.quality,
                    parent_id: None,
                    version: 1,
                    archived: false,
                    compressed_from: vec![],
                };
                cache.store(entry).await;
            }
        }

        output
    }

    pub async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }
}
