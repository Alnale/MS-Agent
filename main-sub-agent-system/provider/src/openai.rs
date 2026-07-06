use async_trait::async_trait;
use futures::Stream;
use secrecy::{ExposeSecret, SecretString};
use std::sync::OnceLock;

use agent_core::provider::*;

use crate::sse_buffer;

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

/// OpenAI-compatible provider
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    default_model: String,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, api_key: &str, default_model: &str) -> Self {
        Self {
            client: shared_client().clone(),
            base_url: base_url.to_string(),
            api_key: SecretString::new(api_key.to_string()),
            default_model: default_model.to_string(),
        }
    }

    fn build_messages(request: &CompletionRequest) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        for m in &request.messages {
            messages.push(serde_json::json!({"role": m.role, "content": m.content}));
        }
        messages
    }

    /// Check response status and map to ProviderError. On 429, read the
    /// `Retry-After` header so the retry layer can honor the server's backoff
    /// rather than guessing with local exponential delay.
    fn check_status(response: &reqwest::Response) -> Option<ProviderError> {
        let status = response.status();
        match status.as_u16() {
            401 => Some(ProviderError::Auth("Invalid API key".to_string())),
            429 => {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());
                Some(ProviderError::RateLimited { retry_after })
            }
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
impl LlmProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }
    fn name(&self) -> &str {
        "OpenAI"
    }
    fn models(&self) -> Vec<String> {
        vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, ProviderError> {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };

        let mut body = serde_json::json!({
            "model": model,
            "messages": Self::build_messages(&request),
        });

        if let Some(max) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        // Serialize tools to OpenAI function calling format
        // Enriches descriptions with data flow hints and prerequisites for better LLM tool chaining
        // Non-function tools (e.g., web_search) are serialized with their parameters at the top level
        if let Some(tools) = &request.tools {
            let openai_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    if t.tool_type != "function" {
                        // Platform tool (e.g., web_search): flatten parameters to top level
                        let mut tool_def = serde_json::json!({ "type": t.tool_type });
                        if let Some(props) = t.parameters.schema.get("properties").and_then(|v| v.as_object()) {
                            for (key, val) in props {
                                // Extract the default value if present, otherwise use the schema value
                                if let Some(default) = val.get("default") {
                                    tool_def[key] = default.clone();
                                }
                            }
                        }
                        tool_def
                    } else {
                        // Standard function tool
                        let mut desc = t.description.clone();
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
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": desc,
                                "parameters": t.parameters.schema,
                            }
                        })
                    }
                })
                .collect();
            body["tools"] = serde_json::json!(openai_tools);
        }

        // Serialize tool_choice
        if let Some(ref choice) = request.tool_choice {
            body["tool_choice"] = match choice {
                agent_core::provider::ToolChoice::Auto => serde_json::json!("auto"),
                agent_core::provider::ToolChoice::None => serde_json::json!("none"),
                agent_core::provider::ToolChoice::Required { name } => {
                    serde_json::json!({"type": "function", "function": {"name": name}})
                }
            };
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(&response) {
            return Err(err);
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let usage = TokenUsage {
            input_tokens: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            cached_tokens: 0,
            reasoning_tokens: json["usage"]["completion_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            total_tokens: json["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
        };

        // Parse tool_calls from OpenAI response format
        let tool_calls = json["choices"][0]["message"]["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|tc| {
                        let id = tc["id"].as_str().unwrap_or("").to_string();
                        let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                        let arguments = tc["function"]["arguments"]
                            .as_str()
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(serde_json::Value::Null);
                        agent_core::tool::ToolCall {
                            id,
                            name,
                            arguments,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let stop_reason = json["choices"][0]["finish_reason"]
            .as_str()
            .map(|s| s.to_string());

        if stop_reason.as_deref() == Some("length") {
            tracing::warn!(
                "LLM response truncated (finish_reason=length, model={}, content_len={}). \
                 The output was cut off — increase max_tokens or simplify the input.",
                model,
                content.len(),
            );
        }

        // Parse annotations (e.g., web search citations)
        let annotations = json["choices"][0]["message"]["annotations"]
            .as_array()
            .map(|arr| arr.to_vec())
            .unwrap_or_default();

        Ok(CompletionResponse {
            content,
            thinking: None,
            model: model.to_string(),
            usage,
            stop_reason,
            tool_calls,
            annotations,
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

        let mut body = serde_json::json!({
            "model": model,
            "messages": Self::build_messages(&request),
            "stream": true,
        });

        if let Some(max) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(&response) {
            return Err(err);
        }

        let byte_stream = response.bytes_stream();
        Ok(Box::new(sse_buffer::buffer_sse(byte_stream)))
    }
}
