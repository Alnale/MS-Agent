use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

use agent_core::provider::{CompletionChunk, ProviderError};

const MAX_SSE_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

/// Buffered SSE parser that handles TCP chunk boundary misalignment.
/// Accumulates raw bytes until a complete SSE event (`\n\n` boundary) is found,
/// then decodes and parses the event. Decoding at event boundaries (which are
/// ASCII `\n\n`) avoids corrupting multi-byte UTF-8 characters that span chunk
/// boundaries — `from_utf8_lossy` on a partial chunk would replace the trailing
/// incomplete bytes with U+FFFD, losing data permanently.
pub struct SseBuffer<S> {
    inner: S,
    buffer: Vec<u8>,
}

impl<S> SseBuffer<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
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
            // Try to extract a complete event from the buffer.
            // `\n\n` is ASCII, so byte-level search is safe in UTF-8 streams
            // (multi-byte sequences never contain ASCII bytes as sub-bytes).
            if let Some(event_end) = find_subslice(&self.buffer, b"\n\n") {
                let event_bytes = self.buffer.split_off(event_end + 2);
                let event_bytes = std::mem::replace(&mut self.buffer, event_bytes);
                let event_bytes = &event_bytes[..event_end];

                // Decode the complete event as UTF-8 — at this point the bytes
                // form a full event, so any multi-byte char is complete.
                let event_str = String::from_utf8_lossy(event_bytes);
                if let Some(chunk) = parse_sse_event(&event_str) {
                    return Poll::Ready(Some(Ok(chunk)));
                }
                // If parsing yielded nothing (comment, empty event), continue loop
                continue;
            }

            // Not enough data in buffer, try to read more from the inner stream
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buffer.extend_from_slice(&bytes);
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
                        let buf = std::mem::take(&mut self.buffer);
                        let remaining = String::from_utf8_lossy(&buf);
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

/// Find the starting index of `needle` in `haystack`, or None.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
            
                    annotations: None,});
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
                            tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                                index,
                                id: Some(id),
                                name: Some(name),
                                arguments_delta: None,
                            }),
                            tool_status: None,
                            sub_agent_results: None,
                companion_state: None,
                            agent_progress: None,
                        
                    annotations: None,});
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
                            tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments_delta: Some(partial),
                            }),
                            tool_status: None,
                            sub_agent_results: None,
                companion_state: None,
                            agent_progress: None,
                        
                    annotations: None,});
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
                            
                    annotations: None,});
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
                            
                    annotations: None,});
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
                        
                    annotations: None,});
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
                    
                    annotations: None,});
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
                                        tool_call_delta: Some(agent_core::tool::ToolCallDelta {
                                            index,
                                            id,
                                            name,
                                            arguments_delta,
                                        }),
                                        tool_status: None,
                                        sub_agent_results: None,
                companion_state: None,
                                        agent_progress: None,
                                    
                    annotations: None,});
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
                                
                    annotations: None,});
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
                            
                    annotations: None,});
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
                    
                    annotations: None,});
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
