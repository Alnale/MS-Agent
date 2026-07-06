//! Agent Tool Loop — ReAct pattern (Reasoning + Acting)
//!
//! Enables agents to iteratively call tools based on LLM decisions.
//! The loop continues until the LLM produces a response without tool calls
//! or max_iterations is reached.

use std::sync::Arc;

use agent_core::boxed_agent::AgentOutput;
use agent_core::error::{AgentTeamsError, Result};
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ToolChoice};
use agent_core::tool::{
    Tool, ToolCall, ToolExecutionContext, ToolResult,
};
use agent_core::tool_engine::ToolExecutionEngine;
use agent_core::tool_param_infer::ParamInferrer;

/// Agent Tool Loop — ReAct pattern (Reasoning + Acting)
///
/// Enables agents to iteratively call tools based on LLM decisions.
/// The loop continues until the LLM produces a response without tool calls
/// or max_iterations is reached.
pub struct AgentToolLoop {
    pub provider: Arc<dyn LlmProvider>,
    pub tool_engine: Arc<ToolExecutionEngine>,
    pub max_iterations: usize,
    pub system_prompt: Option<String>,
    /// Parameter inferrer for automatic parameter completion
    pub param_inferrer: Option<Arc<dyn ParamInferrer>>,
}

impl AgentToolLoop {
    pub fn new(provider: Arc<dyn LlmProvider>, tool_engine: Arc<ToolExecutionEngine>) -> Self {
        Self {
            provider,
            tool_engine,
            max_iterations: 5,
            system_prompt: None,
            param_inferrer: None,
        }
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    pub fn with_param_inferrer(mut self, inferrer: Arc<dyn ParamInferrer>) -> Self {
        self.param_inferrer = Some(inferrer);
        self
    }

    /// Truncate tool output for SSE events to avoid oversized payloads.
    fn truncate_tool_output_for_event(output: &serde_json::Value) -> serde_json::Value {
        const MAX_FIELD_LEN: usize = 500;

        let mut truncated = output.clone();

        if let Some(obj) = truncated.as_object_mut() {
            for key in ["text", "body"] {
                if let Some(val) = obj.get_mut(key) {
                    if let Some(s) = val.as_str() {
                        if s.len() > MAX_FIELD_LEN {
                            *val = serde_json::Value::String(format!(
                                "{}...(truncated, {} chars total)",
                                &s[..MAX_FIELD_LEN.min(s.len())],
                                s.len()
                            ));
                        }
                    }
                }
            }

            if let Some(results) = obj.get_mut("results").and_then(|v| v.as_array_mut()) {
                for result in results.iter_mut() {
                    if let Some(r_obj) = result.as_object_mut() {
                        for key in ["text", "body"] {
                            if let Some(val) = r_obj.get_mut(key) {
                                if let Some(s) = val.as_str() {
                                    if s.len() > MAX_FIELD_LEN {
                                        *val = serde_json::Value::String(format!(
                                            "{}...(truncated, {} chars total)",
                                            &s[..MAX_FIELD_LEN.min(s.len())],
                                            s.len()
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        truncated
    }

    /// Build a data flow context string from available tools' data_flow_hints.
    fn build_data_flow_context(&self, tools: &[Tool]) -> String {
        let hints: Vec<String> = tools
            .iter()
            .filter(|t| !t.data_flow_hints.is_empty() || !t.prerequisites.is_empty())
            .map(|t| {
                let mut parts = Vec::new();
                if !t.data_flow_hints.is_empty() {
                    parts.push(format!("  {}: {}", t.name, t.data_flow_hints.join("；")));
                }
                if !t.prerequisites.is_empty() {
                    parts.push(format!("  {} 需要先调用: {}", t.name, t.prerequisites.join(", ")));
                }
                parts.join("\n")
            })
            .filter(|s| !s.is_empty())
            .collect();

        if hints.is_empty() {
            String::new()
        } else {
            hints.join("\n")
        }
    }

    /// Run the ReAct loop: LLM reasons, optionally calls tools, repeats until done
    pub async fn run(
        &self,
        mut messages: Vec<ChatMessage>,
        available_tools: Vec<Tool>,
        ctx: &ToolExecutionContext,
    ) -> Result<(AgentOutput, Vec<(ToolCall, ToolResult)>)> {
        let mut iteration = 0;
        let mut tool_history: Vec<(ToolCall, ToolResult)> = Vec::new();
        let mut executed_tool_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut executed_tool_names: std::collections::HashSet<String> = std::collections::HashSet::new();

        let data_flow_context = self.build_data_flow_context(&available_tools);
        let enhanced_system_prompt = match (&self.system_prompt, data_flow_context.as_str()) {
            (Some(base), df) if !df.is_empty() => {
                Some(format!("{}\n\n## 工具数据流提示\n{}\n\n当一个工具的参数需要文件路径但数据在上下文中时，先用 file(action=\"write\") 将数据写入临时文件，再调用目标工具。", base, df))
            }
            (Some(base), _) => Some(base.clone()),
            (None, df) if !df.is_empty() => {
                Some(format!("## 工具数据流提示\n{}\n\n当一个工具的参数需要文件路径但数据在上下文中时，先用 file(action=\"write\") 将数据写入临时文件，再调用目标工具。", df))
            }
            _ => None,
        };

        let tools = if available_tools.is_empty() {
            None
        } else {
            Some(available_tools)
        };

        loop {
            if iteration >= self.max_iterations {
                tracing::warn!(
                    "AgentToolLoop reached max iterations ({})",
                    self.max_iterations
                );
                break;
            }
            iteration += 1;

            let request = CompletionRequest {
                messages: messages.clone(),
                tools: tools.clone(),
                tool_choice: Some(ToolChoice::Auto),
                max_tokens: Some(65536),
                temperature: Some(0.5),
                system: enhanced_system_prompt.clone(),
                ..Default::default()
            };

            let response = self.provider.complete(request).await.map_err(|e| {
                AgentTeamsError::Provider(format!("LLM call failed in tool loop: {}", e))
            })?;

            if response.tool_calls.is_empty() {
                return Ok((AgentOutput {
                    content: response.content,
                    thinking: response.thinking,
                    quality: 0.9,
                    annotations: response.annotations,
                    ..Default::default()
                }, tool_history));
            }

            let all_already_executed = response.tool_calls.iter().all(|call| {
                let key = format!("{}:{}", call.name, call.arguments);
                executed_tool_keys.contains(&key)
            });
            if all_already_executed && iteration > 1 {
                tracing::warn!(
                    "AgentToolLoop: all requested tools already executed (exact match), breaking loop (iteration {})",
                    iteration
                );
                let last_content = tool_history
                    .last()
                    .map(|(_, r)| r.output.to_string())
                    .unwrap_or_default();
                return Ok((AgentOutput {
                    content: if response.content.is_empty() {
                        format!("工具已执行完成。{}", last_content)
                    } else {
                        response.content
                    },
                    thinking: response.thinking,
                    quality: 0.7,
                    ..Default::default()
                }, tool_history));
            }

            let thinking_only = response.content.is_empty() && response.thinking.is_some();
            if thinking_only && iteration > 1 {
                let all_names_already_called = response.tool_calls.iter().all(|call| {
                    executed_tool_names.contains(&call.name)
                });
                if all_names_already_called {
                    tracing::warn!(
                        "AgentToolLoop: thinking-only response with already-called tool names, breaking loop (iteration {})",
                        iteration
                    );
                    let last_content = tool_history
                        .last()
                        .map(|(_, r)| r.output.to_string())
                        .unwrap_or_default();
                    return Ok((AgentOutput {
                        content: format!("工具已执行完成。{}", last_content),
                        thinking: response.thinking,
                        quality: 0.6,
                        ..Default::default()
                    }, tool_history));
                }
            }

            let mut tool_results = Vec::new();
            let mut parallel_futures = Vec::new();
            let mut sequential_calls = Vec::new();

            for call in &response.tool_calls {
                let enriched_call = if let Some(ref inferrer) = self.param_inferrer {
                    if let Some(tool) = self.tool_engine.registry().get_tool(&call.name) {
                        let context = inferrer.extract_context(&messages);
                        let enriched_args = inferrer.infer_parameters_with_history(
                            &tool, &call.arguments, &context, &messages, &tool_history
                        ).await;
                        ToolCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            arguments: enriched_args,
                        }
                    } else {
                        call.clone()
                    }
                } else {
                    call.clone()
                };

                if let Some(tool) = self.tool_engine.registry().get_tool(&enriched_call.name) {
                    if tool.allow_parallel {
                        let mut tool_ctx = ctx.clone();
                        tool_ctx.tool_history = tool_history.clone();
                        let engine = self.tool_engine.clone();
                        let call_clone = enriched_call.clone();
                        parallel_futures.push(async move {
                            let result = engine
                                .execute_with_resilience(&call_clone, &tool_ctx)
                                .await
                                .unwrap_or_else(|e| ToolResult {
                                    call_id: call_clone.id.clone(),
                                    name: call_clone.name.clone(),
                                    success: false,
                                    output: serde_json::Value::Null,
                                    error: Some(e.to_string()),
                                    execution_duration_ms: 0,
                                });
                            (call_clone, result)
                        });
                        continue;
                    }
                }
                sequential_calls.push(enriched_call);
            }

            let emit_event = |event: agent_core::tool::ToolStatusEvent| {
                if let Some(ref agent_ctx) = ctx.agent_context {
                    if let Some(ref tx) = agent_ctx.tool_event_tx {
                        tracing::debug!("Emitting tool event: {:?}", &event);
                        let _ = tx.send(event);
                    }
                }
            };

            if !parallel_futures.is_empty() {
                for call in &response.tool_calls {
                    if let Some(tool) = self.tool_engine.registry().get_tool(&call.name) {
                        if tool.allow_parallel {
                            emit_event(agent_core::tool::ToolStatusEvent::Executing {
                                call_id: call.id.clone(),
                                tool_name: call.name.clone(),
                            });
                        }
                    }
                }

                let results = futures::future::join_all(parallel_futures).await;
                for (call, result) in results {
                    let key = format!("{}:{}", call.name, call.arguments);
                    executed_tool_keys.insert(key);
                    executed_tool_names.insert(call.name.clone());

                    let truncated_output = Self::truncate_tool_output_for_event(&result.output);
                    emit_event(agent_core::tool::ToolStatusEvent::Completed {
                        call_id: result.call_id.clone(),
                        tool_name: result.name.clone(),
                        success: result.success,
                        output: truncated_output,
                        error: result.error.clone(),
                        duration_ms: result.execution_duration_ms,
                    });

                    tool_results.push(result);
                    if let Some(r) = tool_results.last() {
                        tool_history.push((call, r.clone()));
                    }
                }
            }

            for call in sequential_calls {
                let key = format!("{}:{}", call.name, call.arguments);
                executed_tool_keys.insert(key);
                executed_tool_names.insert(call.name.clone());

                emit_event(agent_core::tool::ToolStatusEvent::Executing {
                    call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                });

                let mut tool_ctx = ctx.clone();
                tool_ctx.tool_history = tool_history.clone();
                let result = self
                    .tool_engine
                    .execute_with_resilience(&call, &tool_ctx)
                    .await
                    .unwrap_or_else(|e| ToolResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        success: false,
                        output: serde_json::Value::Null,
                        error: Some(e.to_string()),
                        execution_duration_ms: 0,
                    });

                let truncated_output = Self::truncate_tool_output_for_event(&result.output);
                emit_event(agent_core::tool::ToolStatusEvent::Completed {
                    call_id: result.call_id.clone(),
                    tool_name: result.name.clone(),
                    success: result.success,
                    output: truncated_output,
                    error: result.error.clone(),
                    duration_ms: result.execution_duration_ms,
                });
                tool_history.push((call.clone(), result.clone()));
                tool_results.push(result);
            }

            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: response.content.clone(),
                cache_control: None,
                images: None,
                tool_call_id: None,
                tool_calls: Some(response.tool_calls.clone()),
            });

            for result in &tool_results {
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: serde_json::to_string(&result.compact()).unwrap_or_default(),
                    cache_control: None,
                    images: None,
                    tool_call_id: Some(result.call_id.clone()),
                    tool_calls: None,
                });
            }

            let failed_results: Vec<&ToolResult> = tool_results.iter().filter(|r| !r.success).collect();
            if !failed_results.is_empty() {
                let recovery_hints: Vec<String> = failed_results.iter().map(|r| {
                    let error = r.error.as_deref().unwrap_or("unknown");
                    if error.contains("No such file") || error.contains("找不到") || error.contains("not found") {
                        format!("工具 `{}` 失败: 文件不存在。如果此文件应由前一步创建，请先调用 file(action=\"write\") 写入文件。", r.name)
                    } else if error.contains("permission") || error.contains("权限") {
                        format!("工具 `{}` 失败: 权限不足。请检查路径或尝试使用 /tmp/ 目录。", r.name)
                    } else if error.contains("timeout") || error.contains("超时") {
                        format!("工具 `{}` 失败: 超时。可以尝试重试一次，或简化请求。", r.name)
                    } else if error.contains("403") || error.contains("429") {
                        format!("工具 `{}` 失败: 被限制访问。建议换一种方式或告知用户。", r.name)
                    } else if error.contains("参数") || error.contains("parameter") || error.contains("argument") {
                        format!("工具 `{}` 失败: 参数错误。请检查必需参数是否完整，可参考工具描述。", r.name)
                    } else {
                        format!("工具 `{}` 失败: {}。请分析错误原因并调整后重试。", r.name, error)
                    }
                }).collect();

                if !recovery_hints.is_empty() {
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: format!("[系统提示] 工具执行错误分析:\n{}", recovery_hints.join("\n")),
                        cache_control: None,
                        images: None,
                        tool_call_id: None,
                        tool_calls: None,
                    });
                }
            }

            tracing::debug!(
                "AgentToolLoop iteration {}: executed {} tool calls",
                iteration,
                tool_results.len()
            );
        }

        Ok((AgentOutput {
            content: "Tool execution loop reached maximum iterations.".to_string(),
            quality: 0.5,
            ..Default::default()
        }, tool_history))
    }
}
