//! SSE buffer for OpenAI Responses API streaming format
//!
//! Handles the specific SSE event types used by /v1/responses endpoint:
//! - response.output_text.delta
//! - response.reasoning_text.delta
//! - response.function_call_arguments.delta / .done
//! - response.completed / response.incomplete
//! - response.output_item.added / .done
//! - response.created / response.in_progress

use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use agent_core::provider::{CompletionChunk, ProviderError, TokenUsage};

const MAX_SSE_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

/// Buffered SSE parser for Responses API format
pub struct SseBufferResponses<S> {
    inner: S,
    buffer: String,
    /// Track function call state by item_id for accumulating partial arguments
    function_calls: HashMap<String, FunctionCallState>,
}

/// State for tracking a function call across multiple delta events
struct FunctionCallState {
    /// Function name (set when function_call_arguments.done is received)
    _name: String,
    /// Accumulated arguments JSON string
    arguments: String,
}

impl<S> SseBufferResponses<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
            function_calls: HashMap::new(),
        }
    }
}

impl<S> Stream for SseBufferResponses<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<CompletionChunk, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Try to extract a complete event from the buffer
            if let Some(event_end) = self.buffer.find("\n\n") {
                let event_str = self.buffer[..event_end].to_string();
                self.buffer.drain(..event_end + 2);

                // Parse the SSE event
                if let Some(chunk) = self.parse_sse_event(&event_str) {
                    return Poll::Ready(Some(Ok(chunk)));
                }
                continue;
            }

            // Not enough data in buffer, try to read more from the inner stream
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.buffer.push_str(&text);
                    if self.buffer.len() > MAX_SSE_BUFFER_SIZE {
                        self.buffer.clear();
                        return Poll::Ready(Some(Err(ProviderError::Other(
                            "SSE buffer exceeded maximum size (1MB)".to_string(),
                        ))));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ProviderError::Unavailable(e.to_string()))));
                }
                Poll::Ready(None) => {
                    if !self.buffer.is_empty() {
                        let remaining = self.buffer.clone();
                        self.buffer.clear();
                        if let Some(chunk) = self.parse_sse_event(&remaining) {
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> SseBufferResponses<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    fn parse_sse_event(&mut self, event: &str) -> Option<CompletionChunk> {
        let mut event_type = None;
        let mut data = None;

        for line in event.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            // Handle both "event: type" and "event:type" (no space)
            if let Some(t) = line.strip_prefix("event:") {
                event_type = Some(t.trim().to_string());
                continue;
            }

            // Handle both "data: json" and "data:json" (no space)
            if let Some(d) = line.strip_prefix("data:") {
                data = Some(d.trim().to_string());
                continue;
            }
        }

        let event_type = event_type?;
        let data_str = data?;

        let json: serde_json::Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to parse SSE data as JSON: {}", e);
                return None;
            }
        };

        match event_type.as_str() {
            // ── Response lifecycle events ──────────────────────────────────

            "response.created" => {
                tracing::debug!("Response created: id={}", json["response"]["id"].as_str().unwrap_or(""));
                None // No chunk to emit
            }

            "response.in_progress" => {
                tracing::debug!("Response in progress");
                None
            }

            // ── Text output delta ──────────────────────────────────────────

            "response.output_text.delta" => {
                let delta = json["delta"].as_str().unwrap_or("");
                if !delta.is_empty() {
                    Some(CompletionChunk {
                        delta: delta.to_string(),
                        thinking_delta: None,
                        done: false,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: None,
                        sub_agent_results: None,
                        companion_state: None,
                        agent_progress: None,
                    
                    annotations: None,})
                } else {
                    None
                }
            }

            "response.output_text.done" => {
                // Text output complete - no chunk needed, handled by response.completed
                tracing::debug!("Output text done: len={}", json["text"].as_str().unwrap_or("").len());
                None
            }

            // ── Reasoning text delta ───────────────────────────────────────

            "response.reasoning_text.delta" => {
                let delta = json["delta"].as_str().unwrap_or("");
                if !delta.is_empty() {
                    Some(CompletionChunk {
                        delta: String::new(),
                        thinking_delta: Some(delta.to_string()),
                        done: false,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: None,
                        sub_agent_results: None,
                        companion_state: None,
                        agent_progress: None,
                    
                    annotations: None,})
                } else {
                    None
                }
            }

            "response.reasoning_text.done" => {
                tracing::debug!("Reasoning text done: len={}", json["text"].as_str().unwrap_or("").len());
                None
            }

            // ── Function call events ───────────────────────────────────────

            "response.function_call_arguments.delta" => {
                let item_id = json["item_id"].as_str().unwrap_or("").to_string();
                let delta = json["delta"].as_str().unwrap_or("").to_string();
                let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;

                if !delta.is_empty() {
                    // Update function call state
                    self.function_calls
                        .entry(item_id.clone())
                        .or_insert_with(|| FunctionCallState {
                            _name: String::new(),
                            arguments: String::new(),
                        })
                        .arguments
                        .push_str(&delta);

                    Some(CompletionChunk {
                        delta: String::new(),
                        thinking_delta: None,
                        done: false,
                        usage: None,
                        tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                            index: output_index,
                            id: Some(item_id),
                            name: None,
                            arguments_delta: Some(delta),
                        }),
                        tool_status: None,
                        sub_agent_results: None,
                        companion_state: None,
                        agent_progress: None,
                    
                    annotations: None,})
                } else {
                    None
                }
            }

            "response.function_call_arguments.done" => {
                let item_id = json["item_id"].as_str().unwrap_or("").to_string();
                let arguments = json["arguments"].as_str().unwrap_or("{}").to_string();
                let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;

                // Look up name from state (set by output_item.added), not from event data
                // The done event may not include the name field
                let name = self
                    .function_calls
                    .get(&item_id)
                    .map(|s| s._name.clone())
                    .or_else(|| json["name"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();

                tracing::debug!(
                    "Function call done: id={}, name={}, args_len={}",
                    item_id,
                    name,
                    arguments.len()
                );

                // Update state with final values
                if let Some(state) = self.function_calls.get_mut(&item_id) {
                    state.arguments = arguments.clone();
                }

                // Emit a final chunk with complete arguments
                Some(CompletionChunk {
                    delta: String::new(),
                    thinking_delta: None,
                    done: false,
                    usage: None,
                    tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                        index: output_index,
                        id: Some(item_id),
                        name: Some(name),
                        arguments_delta: Some(arguments),
                    }),
                    tool_status: None,
                    sub_agent_results: None,
                    companion_state: None,
                    agent_progress: None,
                
                    annotations: None,})
            }

            // ── Output item events ─────────────────────────────────────────

            "response.output_item.added" => {
                let item_type = json["item"]["type"].as_str().unwrap_or("");
                match item_type {
                    "function_call" => {
                        let item_id = json["item"]["call_id"].as_str().unwrap_or("").to_string();
                        let name = json["item"]["name"].as_str().unwrap_or("").to_string();
                        let output_index = json["output_index"].as_u64().unwrap_or(0) as usize;

                        tracing::debug!("Function call started: id={}, name={}", item_id, name);

                        // Initialize function call state
                        self.function_calls.insert(
                            item_id.clone(),
                            FunctionCallState {
                                _name: name.clone(),
                                arguments: String::new(),
                            },
                        );

                        // Emit initial tool call with name
                        Some(CompletionChunk {
                            delta: String::new(),
                            thinking_delta: None,
                            done: false,
                            usage: None,
                            tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                                index: output_index,
                                id: Some(item_id),
                                name: Some(name),
                                arguments_delta: None,
                            }),
                            tool_status: None,
                            sub_agent_results: None,
                            companion_state: None,
                            agent_progress: None,
                        
                    annotations: None,})
                    }
                    "reasoning" => {
                        tracing::debug!("Reasoning output item added");
                        None
                    }
                    "message" => {
                        tracing::debug!("Message output item added");
                        None
                    }
                    _ => {
                        tracing::debug!("Unknown output item type: {}", item_type);
                        None
                    }
                }
            }

            "response.output_item.done" => {
                let item_type = json["item"]["type"].as_str().unwrap_or("");
                if item_type == "function_call" {
                    let item_id = json["item"]["call_id"].as_str().unwrap_or("").to_string();
                    tracing::debug!("Function call item done: id={}", item_id);
                    // Clean up state
                    self.function_calls.remove(&item_id);
                }
                None
            }

            // ── Content part events ────────────────────────────────────────

            "response.content_part.added" => {
                let part_type = json["part"]["type"].as_str().unwrap_or("");
                tracing::debug!("Content part added: type={}", part_type);
                None
            }

            "response.content_part.done" => {
                let part_type = json["part"]["type"].as_str().unwrap_or("");
                tracing::debug!("Content part done: type={}", part_type);
                None
            }

            // ── Response completion events ─────────────────────────────────

            "response.completed" => {
                let response = &json["response"];
                let usage = parse_usage(response);

                // Check if there are any tool calls in the output
                let has_tool_calls = response["output"]
                    .as_array()
                    .map(|output| {
                        output
                            .iter()
                            .any(|item| item["type"].as_str() == Some("function_call"))
                    })
                    .unwrap_or(false);

                if has_tool_calls {
                    tracing::debug!("Response completed with tool calls");
                } else {
                    tracing::debug!("Response completed");
                }

                // Clean up function call state
                self.function_calls.clear();

                Some(CompletionChunk {
                    delta: String::new(),
                    thinking_delta: None,
                    done: true,
                    usage,
                    tool_call_delta: None,
                    tool_status: None,
                    sub_agent_results: None,
                    companion_state: None,
                    agent_progress: None,
                
                    annotations: None,})
            }

            "response.incomplete" => {
                let response = &json["response"];
                let usage = parse_usage(response);
                let reason = response["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("unknown");

                tracing::warn!("Response incomplete: reason={}", reason);

                // Clean up function call state
                self.function_calls.clear();

                Some(CompletionChunk {
                    delta: String::new(),
                    thinking_delta: None,
                    done: true,
                    usage,
                    tool_call_delta: None,
                    tool_status: None,
                    sub_agent_results: None,
                    companion_state: None,
                    agent_progress: None,
                
                    annotations: None,})
            }

            // ── Error events ───────────────────────────────────────────────

            "error" => {
                let error_code = json["code"].as_str().unwrap_or("unknown");
                let error_message = json["message"].as_str().unwrap_or("Unknown error");
                tracing::error!("SSE error event: code={}, message={}", error_code, error_message);

                Some(CompletionChunk {
                    delta: format!("[Error: {}]", error_message),
                    thinking_delta: None,
                    done: true,
                    usage: None,
                    tool_call_delta: None,
                    tool_status: None,
                    sub_agent_results: None,
                    companion_state: None,
                    agent_progress: None,
                
                    annotations: None,})
            }

            // ── Ignore other event types ───────────────────────────────────

            _ => {
                tracing::trace!("Ignoring SSE event: {}", event_type);
                None
            }
        }
    }
}

/// Parse usage from a response object
fn parse_usage(response: &serde_json::Value) -> Option<TokenUsage> {
    response["usage"].as_object().map(|u| TokenUsage {
        input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cached_tokens: u
            .get("input_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        reasoning_tokens: u
            .get("output_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
    })
}

/// Convenience: wrap a byte stream into an SSE-buffered CompletionChunk stream
pub fn buffer_sse<S>(inner: S) -> SseBufferResponses<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    SseBufferResponses::new(inner)
}
