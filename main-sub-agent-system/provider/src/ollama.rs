use async_trait::async_trait;
use futures::Stream;

use agent_teams_core::provider::*;

/// Ollama local provider
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    default_model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, default_model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            default_model: default_model.to_string(),
        }
    }

    fn check_status(status: reqwest::StatusCode) -> Option<ProviderError> {
        match status.as_u16() {
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
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }
    fn name(&self) -> &str {
        "Ollama"
    }
    fn models(&self) -> Vec<String> {
        vec![self.default_model.clone()]
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
            "stream": false,
        });

        // Pass options to Ollama
        let mut options = serde_json::Map::new();
        if let Some(max_tokens) = request.max_tokens {
            options.insert("num_predict".to_string(), serde_json::json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            options.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if !options.is_empty() {
            body["options"] = serde_json::Value::Object(options);
        }

        // Serialize tools to OpenAI-compatible format (Ollama supports this)
        if let Some(tools) = &request.tools {
            let ollama_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters.schema,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(ollama_tools);
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
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

        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Parse tool_calls from Ollama response (OpenAI-compatible format)
        let tool_calls = json["message"]["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|tc| {
                        let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                        let arguments = tc["function"]["arguments"].clone();
                        agent_teams_core::tool::ToolCall {
                            id: format!("ollama_{}", std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos()),
                            name,
                            arguments,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(CompletionResponse {
            content,
            thinking: None,
            model: model.to_string(),
            usage: TokenUsage {
                input_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
                output_tokens: json["eval_count"].as_u64().unwrap_or(0) as u32,
                cached_tokens: 0,
            },
            stop_reason: None,
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

        let mut body = serde_json::json!({
            "model": model,
            "messages": Self::build_messages(&request),
            "stream": true,
        });

        // Pass options to Ollama (same as non-streaming)
        let mut options = serde_json::Map::new();
        if let Some(max_tokens) = request.max_tokens {
            options.insert("num_predict".to_string(), serde_json::json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            options.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if !options.is_empty() {
            body["options"] = serde_json::Value::Object(options);
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?;

        if let Some(err) = Self::check_status(response.status()) {
            return Err(err);
        }

        // Ollama uses NDJSON (one JSON per line), buffer and split on newlines
        use futures::StreamExt;
        let byte_stream = response.bytes_stream();

        let stream = futures::stream::unfold(
            (byte_stream, String::new()),
            |(mut byte_stream, mut buffer)| async move {
                loop {
                    // Try to extract a complete line from buffer
                    if let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].to_string();
                        buffer.drain(..line_end + 1);

                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let result = match serde_json::from_str::<serde_json::Value>(trimmed) {
                                Ok(json) => {
                                    let delta = json["message"]["content"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    let done = json["done"].as_bool().unwrap_or(false);
                                    Ok(CompletionChunk {
                                        delta,
                                        thinking_delta: None,
                                        done,
                                        usage: None,
                                        tool_call_delta: None,
                                        tool_status: None,
                                        sub_agent_results: None,
                companion_state: None,
                                        agent_progress: None,
                                    })
                                }
                                Err(e) => Err(ProviderError::InvalidResponse(e.to_string())),
                            };
                            return Some((result, (byte_stream, buffer)));
                        }
                        continue;
                    }

                    // Need more data from the byte stream
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(ProviderError::Unavailable(e.to_string())),
                                (byte_stream, buffer),
                            ));
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::new(Box::pin(stream)))
    }
}
