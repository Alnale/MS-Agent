use std::sync::Arc;

use async_trait::async_trait;

use agent_teams_core::boxed_agent::{AgentCapabilities, AgentInput, AgentOutput, BoxedAgent};
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider};

/// Extractor agent: extracts structured data from responses
pub struct ExtractorAgent {
    provider: Arc<dyn LlmProvider>,
}

impl ExtractorAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl BoxedAgent for ExtractorAgent {
    fn id(&self) -> &str {
        "extractor"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 100,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        let system = format!(
            "{}\n\n从以下文本中提取结构化信息（实体、日期、数字、关键事实）。返回 JSON 数组。",
            input.system_prompt
        );

        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: input.content,
                cache_control: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(8192),
            temperature: Some(0.1),
            system: Some(system),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let content = if resp.content.is_empty() {
                    tracing::warn!("Extractor agent received empty content from LLM");
                    "[]".to_string()
                } else {
                    resp.content
                };
                let effects = vec![AgentEffect::InfoFragment {
                    content: content.clone(),
                    agent_id: "extractor".to_string(),
                    priority: 30,
                    hash_key: None,
                    category: Some("extracted_data".to_string()),
                }];
                AgentOutput {
                    content,
                    effects,
                    quality: 0.8,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Extraction error: {}", e)),
        }
    }
}
