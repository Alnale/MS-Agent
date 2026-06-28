use std::sync::OnceLock;

use agent_teams_core::provider::*;
use async_trait::async_trait;
use futures::Stream;
use secrecy::{ExposeSecret, SecretString};

use crate::sse_buffer;

/// Shared HTTP client with connection pooling
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build shared HTTP client: {}, using default", e);
                reqwest::Client::new()
            })
    })
}

/// Anthropic Claude provider
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    default_model: String,
}

impl AnthropicProvider {
    pub fn new(base_url: &str, api_key: &str, default_model: &str) -> Self {
        Self {
            client: shared_client().clone(),
            base_url: base_url.to_string(),
            api_key: SecretString::new(api_key.to_string()),
            default_model: default_model.to_string(),
        }
    }

    fn build_messages_body(request: &CompletionRequest) -> serde_json::Value {
        serde_json::json!({
            "messages": request.messages.iter().map(|m| {
                let mut msg = serde_json::json!({"role": m.role, "content": m.content});
                if let Some(ref cc) = m.cache_control {
                    msg["cache_control"] = cc.clone();
                }
                if let Some(ref tcid) = m.tool_call_id {
                    msg["tool_call_id"] = serde_json::Value::String(tcid.clone());
                }
                if let Some(ref calls) = m.tool_calls {
                    msg["tool_calls"] = serde_json::to_value(calls).unwrap_or_default();
                }
                msg
            }).collect::<Vec<_>>(),
        })
    }

    fn check_status(status: reqwest::StatusCode) -> Option<ProviderError> {
        match status.as_u16() {
            401 => Some(ProviderError::Auth("Invalid API key".to_string())),
            429 => Some(ProviderError::RateLimited { retry_after: None }),
            400..=499 => Some(ProviderError::InvalidResponse(format!(
                "Client error: {}",
                status
            ))),
            500..=599 => Some(ProviderError::Unavailable(format!(
                "Server error: {}",
                status
            ))),
            _ => None,
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }
    fn name(&self) -> &str {
        "Anthropic"
    }
    fn models(&self) -> Vec<String> {
        vec![
            "claude-sonnet-4-20250514".to_string(),
            "claude-haiku-4-5-20251001".to_string(),
            "claude-opus-4-7".to_string(),
        ]
    }

    #[tracing::instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, ProviderError> {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let mut body = Self::build_messages_body(&request);
        body["model"] = serde_json::json!(model);
        body["max_tokens"] = serde_json::json!(request.max_tokens.unwrap_or(65536));

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!(system);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        // Serialize tools to Anthropic tool use format
        // Enriches descriptions with data flow hints and prerequisites for better LLM tool chaining
        if let Some(tools) = &request.tools {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    let mut desc = t.description.clone();
                    // Append data flow hints to tool description so LLM sees them natively
                    if !t.data_flow_hints.is_empty() {
                        desc.push_str(&format!("\n[数据流] {}", t.data_flow_hints.join("；")));
                    }
                    if !t.prerequisites.is_empty() {
                        desc.push_str(&format!("\n[前置依赖] 通常需要先调用: {}", t.prerequisites.join(", ")));
                    }
                    if !t.output_fields.is_empty() {
                        desc.push_str(&format!("\n[输出字段] {}", t.output_fields.join(", ")));
                    }
                    serde_json::json!({
                        "name": t.name,
                        "description": desc,
                        "input_schema": t.parameters.schema,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(anthropic_tools);
        }

        // Configure extended thinking: explicitly enable or disable to prevent
        // proxies that default to thinking-on from wasting tokens.
        let thinking_enabled = request.thinking.as_ref().is_some_and(|t| t.enabled);
        if thinking_enabled {
            let budget = request.thinking.as_ref().map_or(8192, |t| t.budget_tokens);
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget
            });
            // Anthropic requires temperature=1 when thinking is enabled
            body["temperature"] = serde_json::json!(1);
        } else {
            body["thinking"] = serde_json::json!({ "type": "disabled" });
        }

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(response.status()) {
            return Err(err);
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;

        // Parse content blocks: extract text, thinking, and tool_use separately.
        // Only collect thinking content if thinking was explicitly requested,
        // to prevent proxies/models that return thinking by default from leaking.
        let mut content = String::new();
        let mut thinking_content = String::new();
        let mut tool_calls = Vec::new();
        if let Some(blocks) = json["content"].as_array() {
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        content.push_str(block["text"].as_str().unwrap_or(""));
                    }
                    Some("thinking") if thinking_enabled => {
                        thinking_content.push_str(block["thinking"].as_str().unwrap_or(""));
                    }
                    Some("tool_use") => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let arguments = block["input"].clone();
                        tool_calls.push(agent_teams_core::tool::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = TokenUsage {
            input_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cached_tokens: json["usage"]["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        // If content is empty but thinking exists, log warning
        if content.is_empty() && !thinking_content.is_empty() {
            tracing::warn!(
                "LLM returned thinking but no text content. This may indicate an API issue."
            );
        }

        let stop_reason = json["stop_reason"].as_str().map(|s| s.to_string());

        // Warn if the model's response was truncated due to max_tokens
        if stop_reason.as_deref() == Some("max_tokens") {
            tracing::warn!(
                "LLM response truncated (stop_reason=max_tokens, model={}, content_len={}). \
                 The output was cut off — increase max_tokens or simplify the input.",
                model,
                content.len(),
            );
        }

        Ok(CompletionResponse {
            content,
            thinking: if thinking_content.is_empty() {
                None
            } else {
                Some(thinking_content)
            },
            model: model.to_string(),
            usage,
            stop_reason,
            tool_calls,
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<
        Box<dyn Stream<Item = std::result::Result<CompletionChunk, ProviderError>> + Unpin + Send>,
        ProviderError,
    > {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let mut body = Self::build_messages_body(&request);
        body["model"] = serde_json::json!(model);
        body["max_tokens"] = serde_json::json!(request.max_tokens.unwrap_or(65536));
        body["stream"] = serde_json::json!(true);

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!(system);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        // Configure extended thinking: explicitly enable or disable
        let thinking_enabled = request.thinking.as_ref().is_some_and(|t| t.enabled);
        if thinking_enabled {
            let budget = request.thinking.as_ref().map_or(8192, |t| t.budget_tokens);
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget
            });
            body["temperature"] = serde_json::json!(1);
        } else {
            body["thinking"] = serde_json::json!({ "type": "disabled" });
        }

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(response.status()) {
            return Err(err);
        }

        let byte_stream = response.bytes_stream();
        Ok(Box::new(sse_buffer::buffer_sse(byte_stream)))
    }

    fn supports_structured(&self) -> bool {
        true
    }
}
