use std::sync::Arc;

use async_trait::async_trait;

use agent_core::boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent};
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider};

/// Response agent: generates the final user-facing response
pub struct ResponseAgent {
    provider: Arc<dyn LlmProvider>,
}

impl ResponseAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl BoxedAgent for ResponseAgent {
    fn id(&self) -> &str {
        "response_agent"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 500,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(16384),
            temperature: Some(0.7),
            system: Some(input.system_prompt),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Response agent received empty content from LLM");
                    "抱歉，我无法生成回复。".to_string()
                } else {
                    resp.content
                };
                AgentOutput {
                    content,
                    quality: 0.9,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Error: {}", e)),
        }
    }
}
