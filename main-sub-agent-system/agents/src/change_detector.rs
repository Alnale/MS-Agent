use std::sync::Arc;

use async_trait::async_trait;

use agent_core::boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent};
use agent_core::effect::AgentEffect;
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider};

/// Change detector agent: detects significant changes in conversation state
pub struct ChangeDetectorAgent {
    provider: Arc<dyn LlmProvider>,
}

impl ChangeDetectorAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl BoxedAgent for ChangeDetectorAgent {
    fn id(&self) -> &str {
        "change_detector"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 30,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        let system = format!(
            "{}\n\n分析对话中是否有重要的状态变化（情绪转变、需求变化、紧急程度变化等）。\
             返回 JSON：{{\"changes\": [{{\"field\": \"\", \"old\": \"\", \"new\": \"\", \"significance\": \"low|medium|high\"}}]}}",
            input.system_prompt
        );

        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", format!(
                "当前消息：{}\n\n上轮effects：{:?}\n\n历史：{:?}",
                input.content, input.prior_effects, input.recent_history
            ))],
            max_tokens: Some(8192),
            temperature: Some(0.2),
            system: Some(system),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Change detector received empty content from LLM");
                    r#"{"changes": []}"#.to_string()
                } else {
                    resp.content
                };
                let effects = vec![AgentEffect::Custom {
                    effect_type: "change_detection".to_string(),
                    data: serde_json::from_str(&content)
                        .unwrap_or(serde_json::json!({"raw": content})),
                    agent_id: "change_detector".to_string(),
                }];
                AgentOutput {
                    content,
                    effects,
                    quality: 0.75,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Change detection error: {}", e)),
        }
    }
}
