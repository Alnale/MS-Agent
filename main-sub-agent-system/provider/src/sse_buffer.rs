use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

use agent_teams_core::provider::{CompletionChunk, ProviderError};

const MAX_SSE_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

/// Buffered SSE parser that handles TCP chunk boundary misalignment.
/// Accumulates bytes until a complete SSE event (`\n\n` boundary) is found,
/// then parses and yields the event.
pub struct SseBuffer<S> {
    inner: S,
    buffer: String,
}

impl<S> SseBuffer<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
        }
    }
}

impl<S> Stream for SseBuffer<S>
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
                if let Some(chunk) = parse_sse_event(&event_str) {
                    return Poll::Ready(Some(Ok(chunk)));
                }
                // If parsing yielded nothing (comment, empty event), continue loop
                continue;
            }

            // Not enough data in buffer, try to read more from the inner stream
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.buffer.push_str(&text);
                    // Prevent unbounded buffer growth on malformed streams
                    if self.buffer.len() > MAX_SSE_BUFFER_SIZE {
                        self.buffer.clear();
                        return Poll::Ready(Some(Err(ProviderError::Other(
                            "SSE buffer exceeded maximum size (1MB), stream may be malformed"
                                .to_string(),
                        ))));
                    }
                    // Continue loop to try parsing
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(ProviderError::Unavailable(e.to_string()))));
                }
                Poll::Ready(None) => {
                    // Stream ended — flush any remaining buffer
                    if !self.buffer.is_empty() {
                        let remaining = self.buffer.clone();
                        self.buffer.clear();
                        if let Some(chunk) = parse_sse_event(&remaining) {
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

/// Parse a single SSE event string into a CompletionChunk.
/// Handles Anthropic (including thinking blocks and tool_use), OpenAI (including tool_calls), and Ollama SSE formats.
fn parse_sse_event(event: &str) -> Option<CompletionChunk> {
    for line in event.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        // Handle "data: [DONE]" (OpenAI)
        if line == "data: [DONE]" {
            return Some(CompletionChunk {
                delta: String::new(),
                thinking_delta: None,
                done: true,
                usage: None,
                tool_call_delta: None,
                tool_status: None,
                sub_agent_results: None,
                companion_state: None,
                agent_progress: None,
            });
        }

        // Handle "data: {json}" lines
        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                // --- Anthropic tool_use: content_block_start with tool_use type ---
                if json["type"].as_str() == Some("content_block_start")
                    && json["content_block"]["type"].as_str() == Some("tool_use")
                {
                    let id = json["content_block"]["id"].as_str().unwrap_or("").to_string();
                    let name = json["content_block"]["name"].as_str().unwrap_or("").to_string();
                    let index = json["index"].as_u64().unwrap_or(0) as usize;
                    if !name.is_empty() {
                        return Some(CompletionChunk {
                            delta: String::new(),
                            thinking_delta: None,
                            done: false,
                            usage: None,
                            tool_call_delta: Some(agent_teams_core::tool::ToolCallDelta {
                                index,
                                id: Some(id),
                                name: Some(name),
                                arguments_delta: None,
                            }),
                            tool_status: None,
                            sub_agent_results: None,
                companion_state: None,
                            agent_progress: None,
                        });
                    }
                }

                // --- Anthropic tool_use: content_block_delta with input_json_delta ---
                if json["type"].as_str() == Some("content_block_delta")
                    && json["delta"]["type"].as_str() == Some("input_json_delta")
                {
                    let partial = json["delta"]["partial_json"].as_str().unwrap_or("").to_string();
                    let index = json["index"].as_u64().unwrap_or(0) as usize;
                    if !partial.is_empty() {
                        return Some(CompletionChunk {
                            delta: String::new(),
                            thinking_delta: None,
                            done: false,
                            usage: None,
                            tool_call_delta: Some(agent_teams_core::tool::ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments_delta: Some(partial),
                            }),
                            tool_status: None,
                            sub_agent_results: None,
                companion_state: None,
                            agent_progress: None,
                        });
                    }
                }

                // Anthropic thinking block delta
                if json["type"].as_str() == Some("content_block_delta") {
                    if let Some(thinking) = json["delta"]["thinking"].as_str() {
                        if !thinking.is_empty() {
                            return Some(CompletionChunk {
                                delta: String::new(),
                                thinking_delta: Some(thinking.to_string()),
                                done: false,
                                usage: None,
                                tool_call_delta: None,
                                tool_status: None,
                                sub_agent_results: None,
                companion_state: None,
                                agent_progress: None,
                            });
                        }
                    }
                    // Anthropic text delta (content_block_delta with text)
                    if let Some(text) = json["delta"]["text"].as_str() {
                        if !text.is_empty() {
                            return Some(CompletionChunk {
                                delta: text.to_string(),
                                thinking_delta: None,
                                done: false,
                                usage: None,
                                tool_call_delta: None,
                                tool_status: None,
                                sub_agent_results: None,
                companion_state: None,
                                agent_progress: None,
                            });
                        }
                    }
                }

                // Anthropic format (legacy): {"delta": {"text": "..."}}
                if let Some(text) = json["delta"]["text"].as_str() {
                    if !text.is_empty() {
                        return Some(CompletionChunk {
                            delta: text.to_string(),
                            thinking_delta: None,
                            done: false,
                            usage: None,
                            tool_call_delta: None,
                            tool_status: None,
                            sub_agent_results: None,
                companion_state: None,
                            agent_progress: None,
                        });
                    }
                }

                // Anthropic message_stop
                if json["type"].as_str() == Some("message_stop") {
                    return Some(CompletionChunk {
                        delta: String::new(),
                        thinking_delta: None,
                        done: true,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: None,
                        sub_agent_results: None,
                companion_state: None,
                        agent_progress: None,
                    });
                }

                // OpenAI format: {"choices": [{"delta": {"content": "..."}}]}
                if let Some(choices) = json["choices"].as_array() {
                    for choice in choices {
                        // --- OpenAI tool_calls streaming ---
                        if let Some(tool_calls) = choice["delta"]["tool_calls"].as_array() {
                            for tc in tool_calls {
                                let index = tc["index"].as_u64().unwrap_or(0) as usize;
                                let id = tc["id"].as_str().map(|s| s.to_string());
                                let name = tc["function"]["name"].as_str().map(|s| s.to_string());
                                let arguments_delta = tc["function"]["arguments"]
                                    .as_str()
                                    .map(|s| s.to_string());
                                if id.is_some() || name.is_some() || arguments_delta.is_some() {
                                    return Some(CompletionChunk {
                                        delta: String::new(),
                                        thinking_delta: None,
                                        done: false,
                                        usage: None,
                                        tool_call_delta: Some(agent_teams_core::tool::ToolCallDelta {
                                            index,
                                            id,
                                            name,
                                            arguments_delta,
                                        }),
                                        tool_status: None,
                                        sub_agent_results: None,
                companion_state: None,
                                        agent_progress: None,
                                    });
                                }
                            }
                        }

                        if let Some(content) = choice["delta"]["content"].as_str() {
                            if !content.is_empty() {
                                return Some(CompletionChunk {
                                    delta: content.to_string(),
                                    thinking_delta: None,
                                    done: false,
                                    usage: None,
                                    tool_call_delta: None,
                                    tool_status: None,
                                    sub_agent_results: None,
                companion_state: None,
                                    agent_progress: None,
                                });
                            }
                        }
                        let finish = choice["finish_reason"].as_str();
                        if finish.is_some() {
                            return Some(CompletionChunk {
                                delta: String::new(),
                                thinking_delta: None,
                                done: true,
                                usage: None,
                                tool_call_delta: None,
                                tool_status: None,
                                sub_agent_results: None,
                companion_state: None,
                                agent_progress: None,
                            });
                        }
                    }
                }

                // Ollama format: {"message": {"content": "..."}, "done": false}
                if let Some(content) = json["message"]["content"].as_str() {
                    let done = json["done"].as_bool().unwrap_or(false);
                    return Some(CompletionChunk {
                        delta: content.to_string(),
                        thinking_delta: None,
                        done,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: None,
                        sub_agent_results: None,
                companion_state: None,
                        agent_progress: None,
                    });
                }
            }
        }
    }

    None
}

/// Convenience: wrap a byte stream into an SSE-buffered CompletionChunk stream
pub fn buffer_sse<S>(inner: S) -> SseBuffer<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    SseBuffer::new(inner)
}
