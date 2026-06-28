use async_trait::async_trait;
use futures::Stream;
use std::sync::OnceLock;

use agent_teams_core::provider::*;
use tracing;

/// Shared HTTP client with connection pooling
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build shared HTTP client: {}, using default", e);
                reqwest::Client::new()
            })
    })
}

/// Generic HTTP provider with configurable endpoint
pub struct HttpProvider {
    id: String,
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    default_model: String,
}

impl HttpProvider {
    pub fn new(id: &str, base_url: &str, api_key: &str, default_model: &str) -> Self {
        Self {
            id: id.to_string(),
            client: shared_client().clone(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            default_model: default_model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for HttpProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.id
    }
    fn models(&self) -> Vec<String> {
        vec![self.default_model.clone()]
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, ProviderError> {
        // Default to OpenAI-compatible format
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        for m in &request.messages {
            messages.push(serde_json::json!({"role": m.role, "content": m.content}));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        match response.status().as_u16() {
            401 => return Err(ProviderError::Auth("Invalid API key".to_string())),
            429 => return Err(ProviderError::RateLimited { retry_after: None }),
            400..=499 => {
                return Err(ProviderError::InvalidResponse(format!(
                    "Client error: {}",
                    response.status()
                )))
            }
            500..=599 => {
                return Err(ProviderError::Unavailable(format!(
                    "Server error: {}",
                    response.status()
                )))
            }
            _ => {}
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(CompletionResponse {
            content,
            thinking: None,
            model: model.to_string(),
            usage: TokenUsage::default(),
            stop_reason: None,
            tool_calls: vec![],
        })
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<
        Box<dyn Stream<Item = std::result::Result<CompletionChunk, ProviderError>> + Unpin + Send>,
        ProviderError,
    > {
        Err(ProviderError::Other(
            "Streaming not supported for generic HTTP provider".to_string(),
        ))
    }
}
