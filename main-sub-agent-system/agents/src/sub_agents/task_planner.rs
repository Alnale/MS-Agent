use std::sync::Arc;

use async_trait::async_trait;

use agent_core::agent_memory_cache::AgentMemoryCache;
use crate::task_planner_prompt::TASK_PLANNER_SYSTEM_PROMPT;
use agent_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_core::effect::AgentEffect;
use agent_core::memory::{MemoryKind, MemoryQuery};
use agent_core::memory_store::MemoryStore;
use agent_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};
use agent_core::sub_agent::SubAgentDescriptor;
use agent_core::tool::{ToolExecutor, UnifiedToolRegistry};

use crate::main_agent::MainAgent;
use crate::tool_param_infer::{ParameterInferrer, build_parameter_hints, detect_preparatory_steps};
use agent_core::tool_param_infer::ConversationContext;

/// TaskPlannerAgent: unified task planning, tool execution, and routing agent.
///
/// Responsibilities:
/// - Analyze user request to understand what needs to be done
/// - Decide whether tools are needed and plan tool execution
/// - Execute tools directly (absorbed from tool_agent)
/// - Handle file pre-writing when tools need local file input
/// - Determine which other Sub Agents (sentiment, summary) should be called
/// - Assess task complexity and risk level
///
/// Does NOT do:
/// - Answer knowledge questions (handled by Main Agent directly)
/// - Analyze sentiment (delegated to sentiment agent)
/// - Generate final response (delegated to main agent)
pub struct TaskPlannerAgent {
    provider: Arc<dyn LlmProvider>,
    agent_memory_cache: AgentMemoryCache,
    available_agents: Vec<SubAgentDescriptor>,
    thinking_config: Option<ThinkingConfig>,
    max_tokens: u32,
    // Tool infrastructure (absorbed from tool_agent)
    unified_registry: Option<Arc<UnifiedToolRegistry>>,
    tool_executor: Option<Arc<dyn ToolExecutor>>,
    tool_engine: Option<Arc<crate::tool_engine::ToolExecutionEngine>>,
    resource_pool: Arc<agent_core::tool::ResourcePool>,
    param_inferrer: Option<Arc<ParameterInferrer>>,
}

impl TaskPlannerAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            agent_memory_cache: AgentMemoryCache::new("task_planner".to_string(), 100),
            available_agents: Vec::new(),
            thinking_config: None,
            max_tokens: 16384,
            unified_registry: None,
            tool_executor: None,
            tool_engine: None,
            resource_pool: Arc::new(agent_core::tool::ResourcePool::new()),
            param_inferrer: None,
        }
    }

    pub fn with_thinking_config(mut self, config: Option<ThinkingConfig>) -> Self {
        self.thinking_config = config;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_agent_memory_cache(mut self, cache: AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
        self
    }

    pub fn with_available_agents(mut self, agents: Vec<SubAgentDescriptor>) -> Self {
        self.available_agents = agents;
        self
    }

    pub fn with_unified_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.unified_registry = Some(registry);
        self
    }

    pub fn with_tool_executor(mut self, executor: Arc<dyn ToolExecutor>) -> Self {
        self.tool_executor = Some(executor);
        self
    }

    pub fn with_tool_engine(
        mut self,
        engine: Arc<crate::tool_engine::ToolExecutionEngine>,
    ) -> Self {
        self.tool_engine = Some(engine);
        self
    }

    pub fn with_param_inferrer(mut self, inferrer: Arc<ParameterInferrer>) -> Self {
        self.param_inferrer = Some(inferrer);
        self
    }

    /// Validate input to prevent hallucination
    fn validate_input(&self, input: &AgentInput) -> bool {
        if input.content.is_empty() || input.content.len() < 2 {
            tracing::warn!("Input too short: '{}'", input.content);
            return false;
        }
        if input.content.len() > 10000 {
            tracing::warn!("Input too long ({} chars)", input.content.len());
            return false;
        }
        true
    }

    /// Build SubAgent descriptions for the planning prompt (excluding task_planner itself)
    fn build_agent_descriptions(&self) -> String {
        if self.available_agents.is_empty() {
            return "（无可用 SubAgent）".to_string();
        }
        self.available_agents
            .iter()
            .filter(|a| a.id != "task_planner" && a.id != "main_agent")
            .map(|a| {
                format!(
                    "- **{}** (优先级: {}): {}\n  擅长处理: {}",
                    a.id,
                    a.priority,
                    a.expertise,
                    a.capabilities.message_types.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Build tool descriptions for the planning prompt, including data flow hints
    fn build_tool_descriptions(&self) -> String {
        match &self.unified_registry {
            Some(registry) => {
                let tools = registry.list_tools();
                if tools.is_empty() {
                    return "（无已注册工具）".to_string();
                }
                tools
                    .iter()
                    .map(|t| {
                        let required_params: Vec<String> = t.parameters.required.iter()
                            .map(|r| format!("{}*", r))
                            .collect();
                        let optional_params: Vec<String> = t.parameters.schema
                            .get("properties")
                            .and_then(|p| p.as_object())
                            .map(|props| {
                                props.keys()
                                    .filter(|k| !t.parameters.required.contains(k))
                                    .cloned()
                                    .collect()
                            })
                            .unwrap_or_default();

                        let mut all_params = required_params;
                        all_params.extend(optional_params);

                        let params_str = if all_params.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", all_params.join(", "))
                        };

                        let mut desc = format!("- {}{}: {}", t.name, params_str, t.description);

                        // Append data flow hints
                        if !t.data_flow_hints.is_empty() {
                            desc.push_str(&format!("\n  数据流: {}", t.data_flow_hints.join("；")));
                        }

                        // Append prerequisites
                        if !t.prerequisites.is_empty() {
                            desc.push_str(&format!("\n  ⚠ 前置依赖: 通常需要先调用 {}", t.prerequisites.join(", ")));
                        }

                        // Append output fields
                        if !t.output_fields.is_empty() {
                            desc.push_str(&format!("\n  输出字段: {}", t.output_fields.join(", ")));
                        }

                        desc
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            None => "（工具注册表未配置）".to_string(),
        }
    }

    /// Get available tools list
    fn available_tools(&self) -> Vec<agent_core::tool::Tool> {
        self.unified_registry
            .as_ref()
            .map(|r| r.list_tools())
            .unwrap_or_default()
    }

    /// Maximum output size before compression kicks in (8KB)
    const MAX_OUTPUT_SIZE: usize = 32768;
    /// Target compressed size
    const COMPRESSED_SIZE: usize = 16000;
    /// Overall timeout for the entire task_planner run (120s)
    const RUN_TIMEOUT_MS: u64 = 120_000;
    /// Fast-path result size thresholds
    const FAST_PATH_SMALL: usize = 200;
    const FAST_PATH_MEDIUM: usize = 2000;

    /// Compress large tool outputs to prevent context bloat.
    /// For JSON data, uses structure-aware compression that preserves key fields.
    fn compress_output(content: &str) -> String {
        if content.len() <= Self::MAX_OUTPUT_SIZE {
            return content.to_string();
        }

        tracing::info!(
            "Compressing tool output: {} chars -> target {} chars",
            content.len(),
            Self::COMPRESSED_SIZE
        );

        // Try structure-aware JSON compression first
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(compressed) = Self::compress_json_value(&json_val, Self::COMPRESSED_SIZE) {
                if compressed.len() < content.len() {
                    return compressed;
                }
            }
        }

        // Fallback: head/tail truncation for non-JSON content
        let chars: Vec<char> = content.chars().collect();
        let total = chars.len();

        let head_end = (Self::COMPRESSED_SIZE * 6 / 10).min(total);
        let tail_start = (total - Self::COMPRESSED_SIZE / 5).max(head_end);

        let head: String = chars[..head_end].iter().collect();
        let tail: String = chars[tail_start..].iter().collect();
        let skipped = tail_start - head_end;

        format!(
            "{}\n\n... [省略 {} 字符] ...\n\n{}",
            head, skipped, tail
        )
    }

    /// Structure-aware JSON compression: keeps important fields, truncates arrays, preserves structure.
    fn compress_json_value(val: &serde_json::Value, max_size: usize) -> Option<String> {
        match val {
            serde_json::Value::Object(map) => {
                let mut compressed = serde_json::Map::new();
                // Priority fields to always keep
                let priority_keys = ["success", "status", "error", "path", "url", "count",
                    "total", "match_count", "ok", "tool", "content", "message", "title", "name"];
                let mut size = 0;

                // First pass: add priority fields
                for key in &priority_keys {
                    if let Some(v) = map.get(*key) {
                        let val_str = v.to_string();
                        if size + val_str.len() < max_size {
                            compressed.insert(key.to_string(), v.clone());
                            size += val_str.len();
                        }
                    }
                }

                // Second pass: add remaining fields, truncating large values
                for (k, v) in map {
                    if compressed.contains_key(k) {
                        continue;
                    }
                    let val_str = v.to_string();
                    if val_str.len() > 2000 {
                        // Truncate large nested values
                        let truncated = if val_str.len() > 4000 {
                            format!("{}...[{} chars]", &val_str[..2000], val_str.len())
                        } else {
                            val_str.clone()
                        };
                        let truncated_len = truncated.len();
                        if size + truncated_len < max_size {
                            compressed.insert(k.clone(), serde_json::Value::String(truncated));
                            size += truncated_len;
                        }
                    } else if size + val_str.len() < max_size {
                        compressed.insert(k.clone(), v.clone());
                        size += val_str.len();
                    }
                }

                serde_json::to_string_pretty(&compressed).ok()
            }
            serde_json::Value::Array(arr) => {
                // For arrays, keep first few and last few elements
                if arr.len() <= 5 {
                    return serde_json::to_string_pretty(arr).ok();
                }
                let keep_head = 3;
                let keep_tail = 1;
                let mut compressed_arr: Vec<serde_json::Value> = Vec::new();
                for item in arr.iter().take(keep_head) {
                    compressed_arr.push(item.clone());
                }
                compressed_arr.push(serde_json::Value::String(
                    format!("...[{} more items]...", arr.len() - keep_head - keep_tail)
                ));
                for item in arr.iter().skip(arr.len() - keep_tail) {
                    compressed_arr.push(item.clone());
                }
                serde_json::to_string_pretty(&compressed_arr).ok()
            }
            _ => None,
        }
    }

    /// Emit a tool status event if the context has an event sender
    fn emit_tool_event(ctx: &AgentInput, event: agent_core::tool::ToolStatusEvent) {
        if let Some(ref agent_ctx) = ctx.agent_context {
            if let Some(ref tx) = agent_ctx.tool_event_tx {
                tracing::debug!("Emitting tool event: {:?}", event);
                let _ = tx.send(event);
            } else {
                tracing::debug!("No tool_event_tx set on agent_context");
            }
        } else {
            tracing::debug!("No agent_context on input");
        }
    }

    /// Execute a single tool call
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        input: &AgentInput,
    ) -> Result<agent_core::tool::ToolResult, String> {
        let call_id = uuid::Uuid::new_v4().to_string();
        let tool_call = agent_core::tool::ToolCall {
            id: call_id.clone(),
            name: tool_name.to_string(),
            arguments: arguments.clone(),
        };

        // Emit Executing event
        Self::emit_tool_event(input, agent_core::tool::ToolStatusEvent::Executing {
            call_id: call_id.clone(),
            tool_name: tool_name.to_string(),
        });

        let tool_ctx = agent_core::tool::ToolExecutionContext {
            session_id: input.session_id.clone().unwrap_or_default(),
            user_id: input.user_id.clone(),
            agent_id: "task_planner".to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            tool_history: vec![],
            resources: self.resource_pool.clone(),
            agent_context: input.agent_context.clone(),
        };

        let start = std::time::Instant::now();
        let result = if let Some(ref engine) = self.tool_engine {
            engine.execute_with_resilience(&tool_call, &tool_ctx).await
                .map_err(|e| e.to_string())
        } else if let Some(ref executor) = self.tool_executor {
            executor.execute(&tool_call, &tool_ctx).await
                .map_err(|e| e.to_string())
        } else {
            Err("No tool engine or executor configured".to_string())
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        // Emit Completed/Error event
        match &result {
            Ok(tool_result) => {
                Self::emit_tool_event(input, agent_core::tool::ToolStatusEvent::Completed {
                    call_id: call_id.clone(),
                    tool_name: tool_name.to_string(),
                    success: tool_result.success,
                    output: tool_result.output.clone(),
                    error: tool_result.error.clone(),
                    duration_ms,
                });
            }
            Err(e) => {
                Self::emit_tool_event(input, agent_core::tool::ToolStatusEvent::Completed {
                    call_id: call_id.clone(),
                    tool_name: tool_name.to_string(),
                    success: false,
                    output: serde_json::Value::Null,
                    error: Some(e.clone()),
                    duration_ms,
                });
            }
        }

        result
    }

    /// Execute tool chain via AgentToolLoop (ReAct pattern)
    async fn execute_tools_react(
        &self,
        input: &AgentInput,
        system: String,
        tools: Vec<agent_core::tool::Tool>,
    ) -> AgentOutput {
        if let Some(ref engine) = self.tool_engine {
            let mut tool_loop = crate::tool_engine::AgentToolLoop::new(
                self.provider.clone(),
                engine.clone(),
            )
            .with_max_iterations(10)
            .with_system_prompt(system.clone());

            if let Some(ref inferrer) = self.param_inferrer {
                tool_loop = tool_loop.with_param_inferrer(inferrer.clone());
            }

            let messages = vec![ChatMessage::simple("user", &input.content)];
            let tool_ctx = agent_core::tool::ToolExecutionContext {
                session_id: input.session_id.clone().unwrap_or_default(),
                user_id: input.user_id.clone(),
                agent_id: "task_planner".to_string(),
                request_id: uuid::Uuid::new_v4().to_string(),
                tool_history: vec![],
                resources: self.resource_pool.clone(),
                agent_context: input.agent_context.clone(),
            };

            match tool_loop.run(messages, tools, &tool_ctx).await {
                Ok((output, tool_history)) => {
                    let content = Self::compress_output(&output.content);

                    let effects: Vec<AgentEffect> = tool_history
                        .iter()
                        .map(|(call, _result)| AgentEffect::ToolTrigger {
                            tool_name: call.name.clone(),
                            input: call.arguments.clone(),
                            agent_id: "task_planner".to_string(),
                        })
                        .collect();

                    AgentOutput {
                        content,
                        thinking: output.thinking,
                        effects,
                        quality: 0.9,
                        annotations: output.annotations,
                        ..Default::default()
                    }
                }
                Err(e) => AgentOutput::error(format!("Tool execution error: {}", e)),
            }
        } else {
            // Fallback: single-shot LLM call with native tool parsing
            self.run_single_shot(input, system, tools).await
        }
    }

    /// Single-shot fallback: LLM calls tools natively, then continues with tool results.
    async fn run_single_shot(
        &self,
        input: &AgentInput,
        system: String,
        available_tools: Vec<agent_core::tool::Tool>,
    ) -> AgentOutput {
        let request = CompletionRequest {
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.5),
            system: Some(system.clone()),
            tools: if available_tools.is_empty() { None } else { Some(available_tools) },
            thinking: self.thinking_config.clone(),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => {
                let mut effects: Vec<AgentEffect> = Vec::new();

                if resp.tool_calls.is_empty() {
                    return AgentOutput {
                        content: Self::compress_output(&resp.content),
                        thinking: resp.thinking,
                        quality: 0.85,
                        ..Default::default()
                    };
                }

                let messages_for_inference = vec![ChatMessage::simple("user", &input.content)];

                // Enrich all tool calls first, then execute in parallel
                let mut enriched_calls: Vec<agent_core::tool::ToolCall> = Vec::new();
                for tc in &resp.tool_calls {
                    let enriched_tc = if let Some(ref inferrer) = self.param_inferrer {
                        if let Some(tool) = self.unified_registry.as_ref().and_then(|r| r.get_tool(&tc.name)) {
                            inferrer.enrich_tool_call(tc, &tool, &messages_for_inference).await
                        } else {
                            tc.clone()
                        }
                    } else {
                        tc.clone()
                    };
                    effects.push(AgentEffect::ToolTrigger {
                        tool_name: enriched_tc.name.clone(),
                        input: enriched_tc.arguments.clone(),
                        agent_id: "task_planner".to_string(),
                    });
                    enriched_calls.push(enriched_tc);
                }

                // Execute all tool calls in parallel
                let tool_futures: Vec<_> = enriched_calls.iter().map(|enriched_tc| {
                    let tool_ctx = agent_core::tool::ToolExecutionContext {
                        session_id: input.session_id.clone().unwrap_or_default(),
                        user_id: input.user_id.clone(),
                        agent_id: "task_planner".to_string(),
                        request_id: uuid::Uuid::new_v4().to_string(),
                        tool_history: vec![],
                        resources: self.resource_pool.clone(),
                        agent_context: input.agent_context.clone(),
                    };
                    let engine = self.tool_engine.clone();
                    let executor = self.tool_executor.clone();
                    let tc = enriched_tc.clone();
                    async move {
                        let result = if let Some(ref engine) = engine {
                            engine.execute_with_resilience(&tc, &tool_ctx).await
                        } else if let Some(ref executor) = executor {
                            executor.execute(&tc, &tool_ctx).await
                        } else {
                            Err(agent_core::error::AgentTeamsError::ToolNotFound(
                                "No tool engine or executor configured".to_string(),
                            ))
                        };
                        (tc.id.clone(), tc.name.clone(), result)
                    }
                }).collect();

                let parallel_results = futures::future::join_all(tool_futures).await;
                let tool_results: Vec<(String, agent_core::tool::ToolResult)> = parallel_results
                    .into_iter()
                    .map(|(id, name, result)| {
                        let tool_result = match result {
                            Ok(r) => r,
                            Err(e) => agent_core::tool::ToolResult {
                                call_id: id.clone(),
                                name: name.clone(),
                                success: false,
                                output: serde_json::Value::Null,
                                error: Some(e.to_string()),
                                execution_duration_ms: 0,
                            },
                        };
                        (id, tool_result)
                    })
                    .collect();

                let mut messages = vec![
                    ChatMessage::simple("user", &input.content),
                    ChatMessage {
                        role: "assistant".to_string(),
                        content: resp.content.clone(),
                        cache_control: None,
                        images: None,
                        tool_call_id: None,
                        tool_calls: Some(resp.tool_calls.clone()),
                    },
                ];

                for (call_id, result) in &tool_results {
                    let result_str = serde_json::to_string(&result.compact()).unwrap_or_default();
                    let compressed = Self::compress_output(&result_str);
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: compressed,
                        cache_control: None,
                        images: None,
                        tool_call_id: Some(call_id.clone()),
                        tool_calls: None,
                    });
                }

                let continue_request = CompletionRequest {
                    messages,
                    max_tokens: Some(self.max_tokens),
                    temperature: Some(0.5),
                    system: Some(system),
                    thinking: self.thinking_config.clone(),
                    ..Default::default()
                };

                let final_content = match self.provider.complete(continue_request).await {
                    Ok(continue_resp) => continue_resp.content,
                    Err(_) => {
                        let raw: Vec<String> = tool_results.iter().map(|(_, r)| {
                            format!("[{}] {}: {}", if r.success { "OK" } else { "FAIL" }, r.name, r.output)
                        }).collect();
                        format!("{}\n\n## 工具执行结果\n{}", resp.content, raw.join("\n"))
                    }
                };

                AgentOutput {
                    content: Self::compress_output(&final_content),
                    thinking: resp.thinking,
                    effects,
                    quality: 0.85,
                    ..Default::default()
                }
            }
            Err(e) => AgentOutput::error(format!("Tool execution error: {}", e)),
        }
    }
}

#[async_trait]
impl BoxedAgent for TaskPlannerAgent {
    fn id(&self) -> &str {
        "task_planner"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec![
                "routing_decision".to_string(),
                "task_planning".to_string(),
                "user_input".to_string(),
                "tool_request".to_string(),
                "tool_planning".to_string(),
                "tool_execution".to_string(),
                "file_operation".to_string(),
                "web_request".to_string(),
                "browser_automation".to_string(),
            ],
            requires_llm: true,
            supports_streaming: false,
            priority: 110,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_memory_aware(&self) -> Option<&dyn MemoryAwareAgent> {
        Some(self)
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        // Fast path: detect direct tool invocation from orchestrator
        if let Some(tool_call_json) = Self::extract_tool_call_meta(&input.content) {
            let fallback_input = AgentInput {
                content: Self::strip_tool_call_meta(&input.content),
                system_prompt: input.system_prompt.clone(),
                session_id: input.session_id.clone(),
                user_id: input.user_id.clone(),
                recent_history: input.recent_history.clone(),
                prior_effects: input.prior_effects.clone(),
                available_tools: input.available_tools.clone(),
                agent_context: input.agent_context.clone(),
            };
            let result = self.execute_direct_tool_call(input, tool_call_json).await;
            // If the fast path tool call failed, fall back to LLM-based decision
            // so the LLM can choose the correct tool based on context
            if matches!(result.status, agent_core::AgentStatus::Error(_)) {
                tracing::info!(
                    "Fast path tool call failed, falling back to LLM-based task planning"
                );
                if !fallback_input.content.trim().is_empty() {
                    return match tokio::time::timeout(
                        std::time::Duration::from_millis(Self::RUN_TIMEOUT_MS),
                        self.run_core(&fallback_input),
                    ).await {
                        Ok(output) => output,
                        Err(_) => {
                            tracing::error!("Task planner fallback run timed out after {}ms", Self::RUN_TIMEOUT_MS);
                            AgentOutput::error("任务规划超时，请稍后重试".to_string())
                        }
                    };
                }
                return result;
            }
            return result;
        }

        // Validate input
        if !self.validate_input(&input) {
            tracing::warn!("Task planner input validation failed, returning safe default");
            return AgentOutput {
                content: r#"{"tools":[],"other_agents":[],"mode":"Parallel","complexity":"simple","skip_others":true,"reasoning":"输入验证失败","needs_tools":false}"#.to_string(),
                quality: 0.5,
                ..Default::default()
            };
        }

        // Wrap entire run in timeout
        let timeout = std::time::Duration::from_millis(Self::RUN_TIMEOUT_MS);
        let input_ref = &input;
        match tokio::time::timeout(timeout, async { self.run_core(input_ref).await }).await {
            Ok(output) => output,
            Err(_) => {
                tracing::error!("Task planner run timed out after {}ms", Self::RUN_TIMEOUT_MS);
                AgentOutput::error("任务规划超时，请稍后重试".to_string())
            }
        }
    }
}

impl TaskPlannerAgent {
    /// Build memory context string from recent routing decisions and tool usage.
    async fn build_memory_context(&self, input: &AgentInput) -> String {
        let routing_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::AgentOutput],
            tags: vec!["routing_decision".to_string()],
            limit: 3,
            session_id: input.session_id.clone(),
            ..Default::default()
        };
        let routing_memories = self.agent_memory_cache.query(&routing_query).await;

        let tool_query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::AgentOutput],
            tags: vec!["tool_execution".to_string()],
            limit: 3,
            session_id: input.session_id.clone(),
            ..Default::default()
        };
        let tool_memories = self.agent_memory_cache.query(&tool_query).await;

        let mut memory_sections = Vec::new();
        if !routing_memories.is_empty() {
            let history: Vec<String> = routing_memories.iter().map(|m| format!("- {}", m.content)).collect();
            memory_sections.push(format!("## 最近路由决策记录\n{}", history.join("\n")));
        }
        if !tool_memories.is_empty() {
            let history: Vec<String> = tool_memories.iter()
                .filter(|m| m.tags.contains(&"tool_execution".to_string()))
                .map(|m| format!("- {}", m.content))
                .collect();
            if !history.is_empty() {
                memory_sections.push(format!("## 最近工具调用记录（避免重复调用）\n{}", history.join("\n")));
            }
        }
        if memory_sections.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", memory_sections.join("\n\n"))
        }
    }

    /// Build parameter hints string from conversation context.
    fn build_param_hints(&self, input: &AgentInput) -> String {
        let mut context_messages: Vec<ChatMessage> = input.recent_history.iter().filter_map(|entry| {
            let role = entry.get("role")?.as_str()?;
            let content = entry.get("content")?.as_str()?;
            if content.trim().is_empty() { return None; }
            Some(ChatMessage::simple(role, content))
        }).collect();
        context_messages.push(ChatMessage::simple("user", &input.content));
        let context = if let Some(ref inferrer) = self.param_inferrer {
            inferrer.extract_context(&context_messages)
        } else {
            ConversationContext::default()
        };
        let tools = self.available_tools();
        let param_hints: Vec<String> = tools.iter()
            .map(|t| build_parameter_hints(t, &context))
            .filter(|h| !h.is_empty())
            .collect();
        if param_hints.is_empty() {
            String::new()
        } else {
            format!("\n\n## 参数推断提示\n{}", param_hints.join("\n"))
        }
    }

    /// Core task planning logic (separate impl block to avoid async_trait issues)
    async fn run_core(&self, input: &AgentInput) -> AgentOutput {
        let memory_context = self.build_memory_context(input).await;
        let agent_list = self.build_agent_descriptions();
        let tool_list = self.build_tool_descriptions();
        let param_hints_str = self.build_param_hints(input);

        // Build conversation context for tool execution (needed later for preparatory steps)
        let mut context_messages: Vec<ChatMessage> = input.recent_history.iter().filter_map(|entry| {
            let role = entry.get("role")?.as_str()?;
            let content = entry.get("content")?.as_str()?;
            if content.trim().is_empty() { return None; }
            Some(ChatMessage::simple(role, content))
        }).collect();
        context_messages.push(ChatMessage::simple("user", &input.content));
        let context = if let Some(ref inferrer) = self.param_inferrer {
            inferrer.extract_context(&context_messages)
        } else {
            ConversationContext::default()
        };

        let system = TASK_PLANNER_SYSTEM_PROMPT
            .replace("{system_prompt}", &input.system_prompt)
            .replace("{agent_list}", &agent_list)
            .replace("{tool_list}", &tool_list)
            .replace("{memory_context}", &memory_context)
            .replace("{param_hints_str}", &param_hints_str);

        // Phase 1: LLM analyzes the request
        // Build messages with conversation history for context
        let mut messages: Vec<ChatMessage> = input.recent_history.iter().filter_map(|entry| {
            let role = entry.get("role")?.as_str()?;
            let content = entry.get("content")?.as_str()?;
            if content.trim().is_empty() { return None; }
            Some(ChatMessage::simple(role, content))
        }).collect();
        messages.push(ChatMessage::simple("user", &input.content));
        let request = CompletionRequest {
            messages,
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.2),
            system: Some(system.clone()),
            thinking: self.thinking_config.clone(),
            ..Default::default()
        };

        let resp = match self.provider.complete(request).await {
            Ok(resp) => resp,
            Err(e) => return AgentOutput::error(format!("Task planning error: {}", e)),
        };

        let content = resp.content;
        tracing::info!(
            "Task planner LLM response: content_len={}, thinking_len={}",
            content.len(),
            resp.thinking.as_ref().map(|t| t.len()).unwrap_or(0)
        );
        tracing::debug!("Task planner LLM raw response: {}", &content[..content.len().min(2000)]);
        let mut effects: Vec<AgentEffect> = Vec::new();

        // Parse the planning decision — extract JSON from possible markdown code blocks
        let json_str = MainAgent::extract_json_from_response(&content);
        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Task planner JSON parse failed: {} (raw: {})", e, &content[..content.len().min(200)]);
                // If JSON parsing fails, treat as a simple response without tools
                return AgentOutput {
                    content,
                    thinking: resp.thinking,
                    quality: 0.7,
                    ..Default::default()
                };
            }
        };

        let needs_tools = parsed.get("needs_tools").and_then(|v| v.as_bool()).unwrap_or(false);
        let skip_others = parsed.get("skip_others").and_then(|v| v.as_bool()).unwrap_or(false);
        let tools_count = parsed.get("tools").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        tracing::info!(
            "Task planner decision: needs_tools={}, skip_others={}, tools_count={}",
            needs_tools, skip_others, tools_count
        );
        if tools_count > 0 {
            if let Some(tools_arr) = parsed.get("tools").and_then(|v| v.as_array()) {
                for (i, t) in tools_arr.iter().enumerate() {
                    tracing::info!(
                        "Task planner tool[{}]: name={}, args={}",
                        i,
                        t.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                        t.get("arguments").map(|a| a.to_string()).unwrap_or_default()
                    );
                }
            }
        }

        // Phase 2: Execute tools if needed
        let tool_response = if needs_tools {
            // Check for pre-write file requirement
            if let Some(pre_write) = parsed.get("pre_write_file") {
                if !pre_write.is_null() {
                    if let (Some(path), Some(content_val)) = (
                        pre_write.get("path").and_then(|v| v.as_str()),
                        pre_write.get("content").and_then(|v| v.as_str()),
                    ) {
                        tracing::info!("Pre-writing file: {}", path);
                        let write_args = serde_json::json!({
                            "action": "write",
                            "path": path,
                            "content": content_val,
                        });
                        match self.execute_tool_call("file", write_args, input).await {
                            Ok(result) => {
                                if !result.success {
                                    tracing::warn!("Pre-write file failed: {:?}", result.error);
                                } else {
                                    effects.push(AgentEffect::ToolTrigger {
                                        tool_name: "file".to_string(),
                                        input: serde_json::json!({"action": "write", "path": path}),
                                        agent_id: "task_planner".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Pre-write file error: {}", e);
                            }
                        }
                    }
                }
            }

            // Execute pre_steps (LLM-planned preparatory tool calls)
            if let Some(pre_steps) = parsed.get("pre_steps").and_then(|v| v.as_array()) {
                for step in pre_steps {
                    if let (Some(step_tool), Some(step_args)) = (
                        step.get("tool").and_then(|v| v.as_str()),
                        step.get("arguments"),
                    ) {
                        tracing::info!("Executing pre_step: {} with {:?}", step_tool, step_args);
                        match self.execute_tool_call(step_tool, step_args.clone(), input).await {
                            Ok(result) => {
                                if result.success {
                                    tracing::info!("Pre_step {} succeeded", step_tool);
                                    effects.push(AgentEffect::ToolTrigger {
                                        tool_name: step_tool.to_string(),
                                        input: step_args.clone(),
                                        agent_id: "task_planner".to_string(),
                                    });
                                } else {
                                    tracing::warn!("Pre_step {} failed: {:?}", step_tool, result.error);
                                }
                            }
                            Err(e) => tracing::warn!("Pre_step {} error: {}", step_tool, e),
                        }
                    }
                }
            }

            // Execute main tools
            if let Some(tools_array) = parsed.get("tools").and_then(|v| v.as_array()) {
                if !tools_array.is_empty() {
                    // Auto-detect preparatory steps: if a tool needs file path input
                    // but data is in context, automatically write it to a temp file first
                    if self.param_inferrer.is_some() {
                        for tool_entry in tools_array {
                            let tool_name = tool_entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let arguments = tool_entry.get("arguments").cloned().unwrap_or(serde_json::json!({}));
                            if let Some(tool_def) = self.unified_registry.as_ref().and_then(|r| r.get_tool(tool_name)) {
                                let prep_steps = detect_preparatory_steps(&tool_def, &arguments, &context);
                                for step in prep_steps {
                                    tracing::info!("Auto-detected preparatory step: {} -> {}", step.tool_name, step.reason);
                                    // Extract actual data from context to write
                                    let content_to_write = if let Some((_, ref result_val)) = context.recent_results.last() {
                                        serde_json::to_string_pretty(result_val).unwrap_or_else(|_| result_val.to_string())
                                    } else if let Some(last_msg) = context.conversation_history.last() {
                                        last_msg.clone()
                                    } else {
                                        continue;
                                    };
                                    let tmp_path = std::env::temp_dir()
                                        .join(format!("tool_input_{}.json", tool_name))
                                        .to_string_lossy()
                                        .to_string();
                                    let write_args = serde_json::json!({
                                        "action": "write",
                                        "path": tmp_path,
                                        "content": content_to_write,
                                    });
                                    match self.execute_tool_call("file", write_args, input).await {
                                        Ok(result) => {
                                            if result.success {
                                                tracing::info!("Preparatory file write succeeded: {}", tmp_path);
                                                effects.push(AgentEffect::ToolTrigger {
                                                    tool_name: "file".to_string(),
                                                    input: serde_json::json!({"action": "write", "path": tmp_path}),
                                                    agent_id: "task_planner".to_string(),
                                                });
                                            } else {
                                                tracing::warn!("Preparatory file write failed: {:?}", result.error);
                                            }
                                        }
                                        Err(e) => tracing::warn!("Preparatory file write error: {}", e),
                                    }
                                }
                            }
                        }
                    }

                    // For single tool or simple cases, use direct execution
                    // For complex multi-tool cases, use AgentToolLoop
                    if tools_array.len() == 1 {
                        let tool_entry = &tools_array[0];
                        let tool_name = tool_entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let arguments = tool_entry.get("arguments").cloned().unwrap_or(serde_json::json!({}));

                        match self.execute_tool_call(tool_name, arguments.clone(), input).await {
                            Ok(result) => {
                                effects.push(AgentEffect::ToolTrigger {
                                    tool_name: tool_name.to_string(),
                                    input: arguments,
                                    agent_id: "task_planner".to_string(),
                                });

                                if result.success {
                                    let output_str = result.output.to_string();
                                    Some(Self::compress_output(&output_str))
                                } else {
                                    Some(format!("工具 `{}` 执行失败：{}", tool_name,
                                        result.error.unwrap_or_else(|| "未知错误".to_string())))
                                }
                            }
                            Err(e) => Some(format!("工具 `{}` 执行失败：{}", tool_name, e)),
                        }
                    } else {
                        // Multiple tools: execute pre-generated tool calls directly
                        // (skip the AgentToolLoop's extra LLM call to avoid 400 errors)
                        let mut outputs: Vec<String> = Vec::new();
                        for tool_entry in tools_array {
                            let tool_name = tool_entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let arguments = tool_entry.get("arguments").cloned().unwrap_or(serde_json::json!({}));
                            match self.execute_tool_call(tool_name, arguments.clone(), input).await {
                                Ok(result) => {
                                    effects.push(AgentEffect::ToolTrigger {
                                        tool_name: tool_name.to_string(),
                                        input: arguments,
                                        agent_id: "task_planner".to_string(),
                                    });
                                    if result.success {
                                        outputs.push(result.output.to_string());
                                    } else {
                                        outputs.push(format!("工具 `{}` 执行失败：{}", tool_name,
                                            result.error.unwrap_or_else(|| "未知错误".to_string())));
                                    }
                                }
                                Err(e) => outputs.push(format!("工具 `{}` 执行失败：{}", tool_name, e)),
                            }
                        }
                        Some(Self::compress_output(&outputs.join("\n---\n")))
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Build the routing_decision effect
        let other_agents: Vec<String> = parsed
            .get("other_agents")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        effects.push(AgentEffect::Custom {
            effect_type: "routing_decision".to_string(),
            data: serde_json::json!({
                "selected_agents": other_agents,
                "mode": parsed.get("mode").and_then(|v| v.as_str()).unwrap_or("Parallel"),
                "complexity": parsed.get("complexity").and_then(|v| v.as_str()).unwrap_or("simple"),
                "skip_others": skip_others,
                "reasoning": parsed.get("reasoning").and_then(|v| v.as_str()).unwrap_or(""),
                "needs_tools": needs_tools,
                "tool_response": tool_response,
            }),
            agent_id: "task_planner".to_string(),
        });

        // Emit complexity status change for complex/risky tasks
        let complexity = parsed
            .get("complexity")
            .and_then(|v| v.as_str())
            .unwrap_or("simple");
        if matches!(complexity, "complex" | "risky") {
            effects.push(AgentEffect::StatusChange {
                field: "task_complexity".to_string(),
                old_value: "normal".to_string(),
                new_value: complexity.to_string(),
                agent_id: "task_planner".to_string(),
            });
        }

        // Build final content: if tools were executed, include the tool response
        let final_content = if let Some(ref tool_resp) = tool_response {
            // Update the parsed JSON with the tool response
            let mut updated = parsed.clone();
            // Truncate tool_response in the JSON to prevent output bloat
            let truncated_resp = if tool_resp.len() > 16000 {
                format!("{}... [truncated, {} chars total]", &tool_resp[..16000], tool_resp.len())
            } else {
                tool_resp.clone()
            };
            updated["tool_response"] = serde_json::Value::String(truncated_resp);
            serde_json::to_string_pretty(&updated).unwrap_or(content)
        } else {
            content
        };

        AgentOutput {
            content: final_content,
            thinking: resp.thinking,
            effects,
            quality: 0.9,
            ..Default::default()
        }
    }

    /// Extract `[TOOL_CALL]{json}[/TOOL_CALL]` metadata from input content.
    fn extract_tool_call_meta(content: &str) -> Option<serde_json::Value> {
        let start_tag = "[TOOL_CALL]";
        let end_tag = "[/TOOL_CALL]";
        let start = content.find(start_tag)?;
        let json_start = start + start_tag.len();
        let json_end = content.find(end_tag)?;
        let json_str = content[json_start..json_end].trim();
        serde_json::from_str(json_str).ok()
    }

    /// Strip `[TOOL_CALL]...[/TOOL_CALL]` block from content, keeping only the user message.
    fn strip_tool_call_meta(content: &str) -> String {
        let start_tag = "[TOOL_CALL]";
        let end_tag = "[/TOOL_CALL]";
        if let (Some(_start), Some(end)) = (content.find(start_tag), content.find(end_tag)) {
            let after = &content[end + end_tag.len()..];
            after.trim().to_string()
        } else {
            content.to_string()
        }
    }

    /// Execute a direct tool call from the orchestrator (fast path).
    async fn execute_direct_tool_call(
        &self,
        input: AgentInput,
        tool_meta: serde_json::Value,
    ) -> AgentOutput {
        let tool_name = tool_meta["tool_name"].as_str().unwrap_or("");
        let arguments = tool_meta["arguments"].clone();

        // Platform tools (e.g., web_search) are handled by the LLM, not executed locally.
        // Route them through the tool loop with the tool included in the LLM request.
        if tool_name == "web_search" {
            tracing::info!("task_planner: web_search is a platform tool, routing through tool loop");
            let mut tools = self.available_tools();
            // Ensure web_search tool is in the list
            if !tools.iter().any(|t| t.name == "web_search") {
                if let Some(ref registry) = self.unified_registry {
                    if let Some(tool_def) = registry.get_tool("web_search") {
                        tools.push(tool_def);
                    }
                }
            }
            let system = input.system_prompt.clone();
            return self.execute_tools_react(&input, system, tools).await;
        }
        let call_id = tool_meta["call_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        tracing::info!(
            "task_planner: direct tool invocation for '{}' (call_id={})",
            tool_name,
            call_id
        );

        let tool_call = agent_core::tool::ToolCall {
            id: call_id,
            name: tool_name.to_string(),
            arguments,
        };

        let tool_ctx = agent_core::tool::ToolExecutionContext {
            session_id: input.session_id.clone().unwrap_or_default(),
            user_id: input.user_id.clone(),
            agent_id: "task_planner".to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            tool_history: vec![],
            resources: self.resource_pool.clone(),
            agent_context: input.agent_context.clone(),
        };

        let result = if let Some(ref engine) = self.tool_engine {
            engine.execute_with_resilience(&tool_call, &tool_ctx).await
        } else if let Some(ref executor) = self.tool_executor {
            executor.execute(&tool_call, &tool_ctx).await
        } else {
            Err(agent_core::error::AgentTeamsError::ToolNotFound(
                "No tool engine or executor configured".to_string(),
            ))
        };

        match result {
            Ok(tool_result) => {
                if !tool_result.success {
                    let err_msg = format!(
                        "工具 `{}` 执行失败：{}",
                        tool_name,
                        tool_result.error.unwrap_or_else(|| "未知错误".to_string())
                    );
                    return AgentOutput {
                        content: err_msg.clone(),
                        quality: 0.3,
                        status: agent_core::AgentStatus::Error(err_msg),
                        ..Default::default()
                    };
                }

                let tool_output = tool_result.output.to_string();

                // Fast path: skip LLM analysis for small/simple results
                let is_simple = tool_output.len() < Self::FAST_PATH_SMALL
                    || tool_result.output.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let is_medium = tool_output.len() < Self::FAST_PATH_MEDIUM && !tool_output.contains("error");

                if is_simple || is_medium {
                    return AgentOutput {
                        content: format!(
                            "工具 `{}` 执行成功（{}ms）：{}",
                            tool_name, tool_result.execution_duration_ms, tool_output
                        ),
                        quality: 0.85,
                        effects: vec![AgentEffect::ToolTrigger {
                            tool_name: tool_name.to_string(),
                            input: tool_call.arguments,
                            agent_id: "task_planner".to_string(),
                        }],
                        metadata: Some(serde_json::json!({
                            "tool_name": tool_name,
                            "execution_ms": tool_result.execution_duration_ms,
                            "output_size": tool_output.len(),
                            "skipped_analysis": true,
                        })),
                        ..Default::default()
                    };
                }

                // Slow path: use LLM with thinking to analyze complex tool results
                let user_question = input
                    .content
                    .find("[/TOOL_CALL]")
                    .map(|pos| input.content[pos + "[/TOOL_CALL]".len()..].trim())
                    .unwrap_or("");

                let compressed_output = Self::compress_output(&tool_output);

                let analysis_prompt = format!(
                    "工具 `{}` 执行成功（{}ms），返回数据：\n{}\n\n用户问题：{}\n\n分析数据并回答用户问题。如果数据量大，提取关键信息即可。",
                    tool_name, tool_result.execution_duration_ms, compressed_output, user_question
                );

                let analysis_request = CompletionRequest {
                    messages: vec![ChatMessage::simple("user", &analysis_prompt)],
                    max_tokens: Some(self.max_tokens),
                    temperature: Some(0.5),
                    system: Some(format!(
                        "{}\n\n分析工具返回的数据，给出有用的回答。数据量大时只提取关键信息。",
                        input.system_prompt
                    )),
                    thinking: self.thinking_config.clone(),
                    ..Default::default()
                };

                match self.provider.complete(analysis_request).await {
                    Ok(resp) => AgentOutput {
                        content: Self::compress_output(&resp.content),
                        thinking: resp.thinking,
                        quality: 0.9,
                        effects: vec![AgentEffect::ToolTrigger {
                            tool_name: tool_name.to_string(),
                            input: tool_call.arguments,
                            agent_id: "task_planner".to_string(),
                        }],
                        metadata: Some(serde_json::json!({
                            "tool_name": tool_name,
                            "execution_ms": tool_result.execution_duration_ms,
                            "output_size": tool_output.len(),
                            "compressed": tool_output.len() > Self::MAX_OUTPUT_SIZE,
                            "llm_analyzed": true,
                        })),
                        ..Default::default()
                    },
                    Err(e) => {
                        tracing::warn!("LLM analysis failed: {}, returning compressed result", e);
                        AgentOutput {
                            content: format!(
                                "工具 `{}` 执行成功（{}ms）：\n{}",
                                tool_name, tool_result.execution_duration_ms,
                                Self::compress_output(&tool_output)
                            ),
                            quality: 0.7,
                            ..Default::default()
                        }
                    }
                }
            }
            Err(e) => AgentOutput::error(format!("工具 `{}` 执行失败：{}", tool_name, e)),
        }
    }
}

#[async_trait]
impl MemoryAwareAgent for TaskPlannerAgent {
    fn memory_cache(&self) -> &AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> agent_core::error::Result<()> {
        // Cache routing decisions
        if !output.effects.is_empty() {
            for effect in &output.effects {
                if let AgentEffect::Custom {
                    effect_type, data, ..
                } = effect
                {
                    if effect_type == "routing_decision" {
                        let agents = data
                            .get("selected_agents")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();

                        let complexity = data
                            .get("complexity")
                            .and_then(|v| v.as_str())
                            .unwrap_or("simple");

                        let needs_tools = data
                            .get("needs_tools")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        let entry = agent_core::memory::MemoryEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id: Some(session_id.to_string()),
                            kind: MemoryKind::AgentOutput,
                            content: format!(
                                "Routing: agents=[{}], complexity={}, needs_tools={}",
                                agents, complexity, needs_tools
                            ),
                            data: None,
                            embedding: None,
                            weight: 0.3,
                            created_at: chrono::Utc::now(),
                            last_accessed_at: chrono::Utc::now(),
                            access_count: 0,
                            tags: vec!["routing_decision".to_string()],
                            source_agent: "task_planner".to_string(),
                            confirmed: false,
                            content_hash: None,
                            confidence: 0.8,
                            parent_id: None,
                            version: 1,
                            archived: false,
                            compressed_from: vec![],
                        };
                        store.store(entry).await?;
                    }
                }
            }

            // Cache tool execution records
            let tool_names: Vec<String> = output.effects.iter()
                .filter_map(|e| match e {
                    AgentEffect::ToolTrigger { tool_name, .. } => Some(tool_name.clone()),
                    _ => None,
                })
                .collect();

            if !tool_names.is_empty() {
                let entry = agent_core::memory::MemoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: Some(session_id.to_string()),
                    kind: MemoryKind::AgentOutput,
                    content: format!("Tools used: {}", tool_names.join(", ")),
                    data: None,
                    embedding: None,
                    weight: 0.3,
                    created_at: chrono::Utc::now(),
                    last_accessed_at: chrono::Utc::now(),
                    access_count: 0,
                    tags: vec!["tool_execution".to_string()],
                    source_agent: "task_planner".to_string(),
                    confirmed: false,
                    content_hash: None,
                    confidence: 0.8,
                    parent_id: None,
                    version: 1,
                    archived: false,
                    compressed_from: vec![],
                };
                store.store(entry).await?;
            }
        }

        self.agent_memory_cache.flush_all().await?;
        Ok(())
    }
}
