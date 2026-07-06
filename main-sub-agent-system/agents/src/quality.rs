use std::sync::Arc;

use async_trait::async_trait;

use agent_core::boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent};
use agent_core::effect::AgentEffect;
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider};

/// Quality agent: checks response quality and flags issues
pub struct QualityAgent {
    provider: Arc<dyn LlmProvider>,
}

impl QualityAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl BoxedAgent for QualityAgent {
    fn id(&self) -> &str {
        "quality"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 50,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        let system = format!(
            "{}\n\n评估以下响应的质量。返回 JSON：\n\
             {{\"quality_score\": 0.0-1.0, \"issues\": [], \"suggestions\": []}}",
            input.system_prompt
        );

        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(8192),
            temperature: Some(0.1),
            system: Some(system),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Quality agent received empty content from LLM");
                    r#"{"quality_score": 0.5, "issues": ["无法评估"], "suggestions": []}"#
                        .to_string()
                } else {
                    resp.content
                };
                let parsed: serde_json::Value = serde_json::from_str(&content)
                    .unwrap_or(serde_json::json!({"quality_score": 0.5}));
                let score = parsed["quality_score"].as_f64().unwrap_or(0.5) as f32;

                let effects = vec![AgentEffect::Custom {
                    effect_type: "quality_check".to_string(),
                    data: parsed,
                    agent_id: "quality".to_string(),
                }];

                AgentOutput {
                    content,
                    effects,
                    quality: score,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Quality check error: {}", e)),
        }
    }
}
