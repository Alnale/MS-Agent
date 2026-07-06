//! OpenAI Responses API provider (MIMO-compatible)
//!
//! Implements the /v1/responses endpoint which has a different request/response
//! format from the traditional /v1/chat/completions endpoint.
//!
//! Key differences from Chat Completions API:
//! - Uses `input` array instead of `messages`
//! - Uses `instructions` instead of `system`
//! - Uses `max_output_tokens` instead of `max_tokens`
//! - Uses `reasoning.effort` instead of `thinking.type`
//! - Response has `output` array with mixed types (message/reasoning/function_call)
//! - Tool calls use `call_id` instead of `id`

use async_trait::async_trait;
use futures::Stream;
use secrecy::{ExposeSecret, SecretString};
use std::sync::OnceLock;

use agent_core::provider::*;
use agent_core::tool::Tool;

use crate::sse_buffer_responses;

/// MIMO model IDs supported by Responses API
const MIMO_MODELS: &[&str] = &["mimo-v2.5-pro", "mimo-v2.5"];

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

/// OpenAI Responses API provider (MIMO-compatible)
pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    default_model: String,
}

impl OpenAiResponsesProvider {
    pub fn new(base_url: &str, api_key: &str, default_model: &str) -> Self {
        Self {
            client: shared_client().clone(),
            base_url: base_url.to_string(),
            api_key: SecretString::new(api_key.to_string()),
            default_model: default_model.to_string(),
        }
    }

    /// Build the input array for Responses API
    ///
    /// Converts internal ChatMessage format to Responses API InputItem format.
    /// Also supports plain string input (from CompletionRequest.input) as a user message.
    fn build_input(request: &CompletionRequest) -> serde_json::Value {
        let mut input_items = Vec::new();

        // If input is a plain string, wrap as user message
        if let Some(ref input_str) = request.input {
            if !input_str.is_empty() {
                input_items.push(serde_json::json!({
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": input_str
                    }]
                }));
            }
        }

        for m in &request.messages {
            match m.role.as_str() {
                // System messages are handled via instructions field
                "system" => continue,

                "user" => {
                    // Check if message contains image attachments
                    let mut content_parts = Vec::new();
                    if let Some(ref images) = m.images {
                        for img_url in images {
                            content_parts.push(serde_json::json!({
                                "type": "input_image",
                                "image_url": img_url
                            }));
                        }
                    }
                    content_parts.push(serde_json::json!({
                        "type": "input_text",
                        "text": m.content
                    }));
                    input_items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": content_parts
                    }));
                }

                "assistant" => {
                    // If this message has tool_calls, emit FunctionCall items
                    if let Some(ref tool_calls) = m.tool_calls {
                        for tc in tool_calls {
                            input_items.push(serde_json::json!({
                                "type": "function_call",
                                "call_id": tc.id,
                                "name": tc.name,
                                "arguments": serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".to_string())
                            }));
                        }
                    }

                    // If there's text content, also emit as assistant message
                    if !m.content.is_empty() {
                        input_items.push(serde_json::json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": m.content
                            }],
                            "status": "completed"
                        }));
                    }
                }

                "tool" => {
                    // Tool result messages → FunctionCallOutput
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        input_items.push(serde_json::json!({
                            "type": "function_call_output",
                            "call_id": tool_call_id,
                            "output": m.content
                        }));
                    } else {
                        tracing::warn!("Tool message without tool_call_id, skipping");
                    }
                }

                "reasoning" => {
                    // Preserve reasoning content for multi-turn tool calls
                    // This is critical for MIMO's thinking mode
                    input_items.push(serde_json::json!({
                        "type": "reasoning",
                        "id": format!("rs_{}", m.content.chars().take(8).collect::<String>()),
                        "content": [{
                            "type": "reasoning_text",
                            "text": m.content
                        }],
                        "status": "completed"
                    }));
                }

                _ => {
                    // Unknown role, treat as user message
                    tracing::warn!("Unknown message role '{}', treating as user", m.role);
                    input_items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": m.content
                        }]
                    }));
                }
            }
        }

        serde_json::json!(input_items)
    }

    /// Build tools array for Responses API
    fn build_tools(tools: &[Tool]) -> serde_json::Value {
        let response_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let mut desc = t.description.clone();
                if !t.data_flow_hints.is_empty() {
                    desc.push_str(&format!("\n[数据流] {}", t.data_flow_hints.join("；")));
                }
                if !t.prerequisites.is_empty() {
                    desc.push_str(&format!(
                        "\n[前置依赖] 通常需要先调用: {}",
                        t.prerequisites.join(", ")
                    ));
                }
                if !t.output_fields.is_empty() {
                    desc.push_str(&format!("\n[输出字段] {}", t.output_fields.join(", ")));
                }
                serde_json::json!({
                    "type": "function",
                    "name": t.name,
                    "description": desc,
                    "parameters": t.parameters.schema,
                    "strict": false
                })
            })
            .collect();
        serde_json::json!(response_tools)
    }

    fn check_status(status: reqwest::StatusCode) -> Option<ProviderError> {
        match status.as_u16() {
            401 => Some(ProviderError::Auth("Invalid API key".to_string())),
            421 => Some(ProviderError::Other("Content filter triggered (421)".to_string())),
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

    /// Parse a non-streaming Responses API response
    fn parse_response(
        json: serde_json::Value,
        model: &str,
    ) -> std::result::Result<CompletionResponse, ProviderError> {
        // Check for top-level error
        if let Some(error) = json["error"].as_object() {
            let code = error.get("code").and_then(|v| v.as_str()).unwrap_or("unknown");
            let message = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(ProviderError::Other(format!(
                "API error: {} - {}",
                code, message
            )));
        }

        let mut content = String::new();
        let mut thinking_content = String::new();
        let mut tool_calls = Vec::new();

        // Parse output array
        if let Some(output) = json["output"].as_array() {
            for item in output {
                match item["type"].as_str() {
                    Some("message") => {
                        if let Some(content_blocks) = item["content"].as_array() {
                            for block in content_blocks {
                                if block["type"].as_str() == Some("output_text") {
                                    if let Some(text) = block["text"].as_str() {
                                        content.push_str(text);
                                    }
                                }
                            }
                        }
                    }
                    Some("reasoning") => {
                        if let Some(content_blocks) = item["content"].as_array() {
                            for block in content_blocks {
                                if block["type"].as_str() == Some("reasoning_text") {
                                    if let Some(text) = block["text"].as_str() {
                                        thinking_content.push_str(text);
                                    }
                                }
                            }
                        }
                    }
                    Some("function_call") => {
                        let id = item["call_id"].as_str().unwrap_or("").to_string();
                        let name = item["name"].as_str().unwrap_or("").to_string();
                        let arguments = match item["arguments"].as_str() {
                            Some(s) => serde_json::from_str(s).unwrap_or_else(|e| {
                                tracing::warn!(
                                    "Failed to parse function_call arguments as JSON: {}. Raw: {}",
                                    e,
                                    &s[..s.len().min(200)]
                                );
                                serde_json::Value::Null
                            }),
                            None => {
                                tracing::warn!("function_call.arguments is not a string");
                                serde_json::Value::Null
                            }
                        };
                        tool_calls.push(agent_core::tool::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {
                        tracing::debug!("Unknown output type: {:?}", item["type"].as_str());
                    }
                }
            }
        }

        // Parse usage with full detail
        let usage = TokenUsage {
            input_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cached_tokens: json["usage"]["input_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            reasoning_tokens: json["usage"]["output_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            total_tokens: json["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
        };

        // Determine stop reason
        let status = json["status"].as_str().unwrap_or("completed");
        let stop_reason = match status {
            "completed" => {
                if !tool_calls.is_empty() {
                    Some("tool_use".to_string())
                } else {
                    Some("end_turn".to_string())
                }
            }
            "incomplete" => {
                let reason = json["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("unknown");
                match reason {
                    "max_output_tokens" => Some("max_tokens".to_string()),
                    "content_filter" => Some("content_filter".to_string()),
                    _ => {
                        tracing::warn!("Unknown incomplete reason: {}", reason);
                        Some("max_tokens".to_string())
                    }
                }
            }
            _ => {
                tracing::warn!("Unknown response status: {}", status);
                None
            }
        };

        if stop_reason.as_deref() == Some("max_tokens") {
            tracing::warn!(
                "LLM response truncated (stop_reason=max_tokens, model={}, content_len={}). \
                 The output was cut off — increase max_output_tokens or simplify the input.",
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
            annotations: vec![],
        })
    }

    /// Build common request body fields
    fn build_body(
        &self,
        request: &CompletionRequest,
        model: &str,
        stream: bool,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": model,
            "input": Self::build_input(request),
        });

        if stream {
            body["stream"] = serde_json::json!(true);
        }

        // Add instructions from system message
        if let Some(system) = &request.system {
            body["instructions"] = serde_json::json!(system);
        }

        // max_output_tokens (Responses API uses this instead of max_tokens)
        if let Some(max) = request.max_tokens {
            body["max_output_tokens"] = serde_json::json!(max);
        }

        // reasoning configuration
        // MIMO supports: "none" (off), "low"/"medium"/"high" (all enable reasoning, same effect)
        let thinking_enabled = request
            .thinking
            .as_ref()
            .is_some_and(|t| t.enabled);
        if thinking_enabled {
            body["reasoning"] = serde_json::json!({ "effort": "high" });
            // MIMO forces temperature=1.0 and top_p=0.95 in thinking mode
            body["temperature"] = serde_json::json!(1.0);
            body["top_p"] = serde_json::json!(0.95);
        } else {
            body["reasoning"] = serde_json::json!({ "effort": "none" });
            // Only set temperature/top_p when thinking is disabled
            if let Some(temp) = request.temperature {
                body["temperature"] = serde_json::json!(temp);
            }
        }

        // text format (structured output support)
        if let Some(ref response_format) = request.response_format {
            match response_format.format_type.as_str() {
                "json_object" => {
                    body["text"] = serde_json::json!({
                        "format": { "type": "json_object" }
                    });
                }
                "json_schema" => {
                    body["text"] = serde_json::json!({
                        "format": {
                            "type": "json_schema",
                            "name": response_format.name,
                            "schema": response_format.schema,
                            "strict": true
                        }
                    });
                }
                _ => {
                    // "text" is the default, no need to set
                }
            }
        }

        // tools
        if let Some(tools) = &request.tools {
            body["tools"] = Self::build_tools(tools);
        }

        // tool_choice (MIMO only supports "auto")
        if let Some(ref choice) = request.tool_choice {
            match choice {
                ToolChoice::Auto => {
                    body["tool_choice"] = serde_json::json!("auto");
                }
                ToolChoice::None => {
                    // Explicitly disable tools
                    body["tool_choice"] = serde_json::json!("none");
                }
                ToolChoice::Required { .. } => {
                    // MIMO only supports auto, so we use auto as fallback
                    tracing::warn!(
                        "MIMO Responses API only supports tool_choice='auto', falling back to auto"
                    );
                    body["tool_choice"] = serde_json::json!("auto");
                }
            }
        }

        body
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn id(&self) -> &str {
        "openai_responses"
    }

    fn name(&self) -> &str {
        "OpenAI Responses API"
    }

    fn models(&self) -> Vec<String> {
        MIMO_MODELS.iter().map(|s| s.to_string()).collect()
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

        let body = self.build_body(&request, model, false);

        tracing::debug!(
            "Responses API request: model={}, input_items={}, thinking={}",
            model,
            body["input"].as_array().map(|a| a.len()).unwrap_or(0),
            body["reasoning"]["effort"].as_str().unwrap_or("none")
        );

        let response = self
            .client
            .post(format!("{}/responses", self.base_url))
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
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

        Self::parse_response(json, model)
    }

    #[tracing::instrument(skip(self, request), fields(model = %request.model))]
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

        let body = self.build_body(&request, model, true);

        tracing::debug!(
            "Responses API streaming request: model={}, input_items={}, thinking={}",
            model,
            body["input"].as_array().map(|a| a.len()).unwrap_or(0),
            body["reasoning"]["effort"].as_str().unwrap_or("none")
        );

        let response = self
            .client
            .post(format!("{}/responses", self.base_url))
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(response.status()) {
            return Err(err);
        }

        let byte_stream = response.bytes_stream();
        Ok(Box::new(sse_buffer_responses::buffer_sse(byte_stream)))
    }
}
