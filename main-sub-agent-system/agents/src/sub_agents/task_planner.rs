use std::sync::Arc;

use async_trait::async_trait;

use agent_teams_core::agent_memory_cache::AgentMemoryCache;
use agent_teams_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::memory::{MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::MemoryStore;
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};
use agent_teams_core::sub_agent::SubAgentDescriptor;
use agent_teams_core::tool::{ToolExecutor, UnifiedToolRegistry};

use crate::main_agent::MainAgent;
use crate::tool_param_infer::{ParameterInferrer, ConversationContext, build_parameter_hints, detect_preparatory_steps};

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
    resource_pool: Arc<agent_teams_core::tool::ResourcePool>,
    param_inferrer: Option<Arc<ParameterInferrer>>,
}

#[allow(dead_code)]
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
            resource_pool: Arc::new(agent_teams_core::tool::ResourcePool::new()),
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
    fn available_tools(&self) -> Vec<agent_teams_core::tool::Tool> {
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
    fn emit_tool_event(ctx: &AgentInput, event: agent_teams_core::tool::ToolStatusEvent) {
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
    ) -> Result<agent_teams_core::tool::ToolResult, String> {
        let call_id = uuid::Uuid::new_v4().to_string();
        let tool_call = agent_teams_core::tool::ToolCall {
            id: call_id.clone(),
            name: tool_name.to_string(),
            arguments: arguments.clone(),
        };

        // Emit Executing event
        Self::emit_tool_event(input, agent_teams_core::tool::ToolStatusEvent::Executing {
            call_id: call_id.clone(),
            tool_name: tool_name.to_string(),
        });

        let tool_ctx = agent_teams_core::tool::ToolExecutionContext {
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
                Self::emit_tool_event(input, agent_teams_core::tool::ToolStatusEvent::Completed {
                    call_id: call_id.clone(),
                    tool_name: tool_name.to_string(),
                    success: tool_result.success,
                    output: tool_result.output.clone(),
                    error: tool_result.error.clone(),
                    duration_ms,
                });
            }
            Err(e) => {
                Self::emit_tool_event(input, agent_teams_core::tool::ToolStatusEvent::Completed {
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
        tools: Vec<agent_teams_core::tool::Tool>,
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
            let tool_ctx = agent_teams_core::tool::ToolExecutionContext {
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

    /// Execute multiple tools following the dependency plan from the LLM.
    /// Respects `depends_on` field: tools with no dependencies run in parallel,
    /// tools that depend on others wait for their dependencies to complete.
    /// Falls back to AgentToolLoop if the plan is too complex.
    async fn execute_tools_with_plan(
        &self,
        input: &AgentInput,
        tools_array: &[serde_json::Value],
        system: &str,
    ) -> AgentOutput {
        // Parse tool entries and their dependencies
        let mut tool_entries: Vec<(String, serde_json::Value, Option<usize>)> = Vec::new();
        for entry in tools_array {
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let args = entry.get("arguments").cloned().unwrap_or(serde_json::json!({}));
            let depends_on = entry.get("depends_on").and_then(|v| v.as_u64()).map(|v| v as usize);
            tool_entries.push((name, args, depends_on));
        }

        // If no dependencies declared, use AgentToolLoop for LLM-driven multi-step execution
        let has_dependencies = tool_entries.iter().any(|(_, _, dep)| dep.is_some());
        if !has_dependencies {
            // Build tools list for AgentToolLoop
            let tools: Vec<agent_teams_core::tool::Tool> = tool_entries.iter()
                .filter_map(|(name, _, _)| {
                    self.unified_registry.as_ref().and_then(|r| r.get_tool(name))
                })
                .collect();
            return self.execute_tools_react(input, system.to_string(), tools).await;
        }

        // Execute with dependency ordering
        let mut effects: Vec<AgentEffect> = Vec::new();
        let mut results: Vec<(String, serde_json::Value)> = Vec::new(); // (tool_name, output)
        let mut completed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let max_rounds = tool_entries.len() + 1; // Safety limit

        for _round in 0..max_rounds {
            if completed.len() >= tool_entries.len() {
                break;
            }

            // Find tools whose dependencies are all satisfied
            let mut ready_indices: Vec<usize> = Vec::new();
            for (i, (_, _, dep)) in tool_entries.iter().enumerate() {
                if completed.contains(&i) {
                    continue;
                }
                match dep {
                    None => ready_indices.push(i),
                    Some(dep_idx) if completed.contains(dep_idx) => ready_indices.push(i),
                    _ => {} // dependency not yet satisfied
                }
            }

            if ready_indices.is_empty() {
                tracing::warn!("Dependency deadlock detected in tool plan, falling back to sequential execution");
                break;
            }

            // Execute ready tools sequentially (with dependency injection)
            for &idx in &ready_indices {
                let (name, args, _) = &tool_entries[idx];
                // Inject dependency output into args if available
                let mut enriched_args = args.clone();
                if let Some(dep_idx) = tool_entries[idx].2 {
                    if let Some((_, dep_output)) = results.iter().find(|(n, _)| {
                        tool_entries.iter().position(|(tn, _, _)| tn == n) == Some(dep_idx)
                    }) {
                        // Inject dependency output as a context field
                        if enriched_args.get("content").is_none() && dep_output.is_object() {
                            if let Some(dep_content) = dep_output.get("content").and_then(|v| v.as_str()) {
                                enriched_args["content"] = serde_json::Value::String(dep_content.to_string());
                            }
                        }
                        // For file tools, auto-inject path from dependency
                        if name == "file" && enriched_args.get("path").is_none() {
                            if let Some(path) = dep_output.get("path").and_then(|v| v.as_str()) {
                                enriched_args["path"] = serde_json::Value::String(path.to_string());
                            }
                        }
                    }
                }

                let tool_name = name.clone();
                let tool_args = enriched_args;
                match self.execute_tool_call(&tool_name, tool_args.clone(), input).await {
                    Ok(tool_result) => {
                        effects.push(AgentEffect::ToolTrigger {
                            tool_name: tool_name.clone(),
                            input: tool_args,
                            agent_id: "task_planner".to_string(),
                        });
                        if tool_result.success {
                            results.push((tool_name, tool_result.output));
                        } else {
                            results.push((tool_name, serde_json::json!({
                                "error": tool_result.error.unwrap_or_else(|| "unknown error".to_string())
                            })));
                        }
                    }
                    Err(e) => {
                        results.push((tool_name, serde_json::json!({"error": e})));
                    }
                }
                completed.insert(idx);
            }
        }

        // Build combined output
        let output_parts: Vec<String> = results.iter().map(|(name, output)| {
            let output_str = if output.to_string().len() > 4000 {
                format!("{}...[truncated]", &output.to_string()[..4000])
            } else {
                output.to_string()
            };
            format!("[{}] {}", name, output_str)
        }).collect();

        let combined = Self::compress_output(&output_parts.join("\n\n"));

        AgentOutput {
            content: combined,
            effects,
            quality: 0.9,
            ..Default::default()
        }
    }

    /// Single-shot fallback: LLM calls tools natively, then continues with tool results.
    async fn run_single_shot(
        &self,
        input: &AgentInput,
        system: String,
        available_tools: Vec<agent_teams_core::tool::Tool>,
    ) -> AgentOutput {
        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.5),
            system: Some(system.clone()),
            stream: false,
            tools: if available_tools.is_empty() { None } else { Some(available_tools) },
            tool_choice: None,
            metadata: None,
            thinking: self.thinking_config.clone(),
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
                let mut enriched_calls: Vec<agent_teams_core::tool::ToolCall> = Vec::new();
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
                    let tool_ctx = agent_teams_core::tool::ToolExecutionContext {
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
                            Err(agent_teams_core::error::AgentTeamsError::ToolNotFound(
                                "No tool engine or executor configured".to_string(),
                            ))
                        };
                        (tc.id.clone(), tc.name.clone(), result)
                    }
                }).collect();

                let parallel_results = futures::future::join_all(tool_futures).await;
                let tool_results: Vec<(String, agent_teams_core::tool::ToolResult)> = parallel_results
                    .into_iter()
                    .map(|(id, name, result)| {
                        let tool_result = match result {
                            Ok(r) => r,
                            Err(e) => agent_teams_core::tool::ToolResult {
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
                        tool_call_id: Some(call_id.clone()),
                        tool_calls: None,
                    });
                }

                let continue_request = CompletionRequest {
                    model: String::new(),
                    messages,
                    max_tokens: Some(self.max_tokens),
                    temperature: Some(0.5),
                    system: Some(system),
                    stream: false,
                    tools: None,
                    tool_choice: None,
                    metadata: None,
                    thinking: self.thinking_config.clone(),
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
            if matches!(result.status, agent_teams_core::AgentStatus::Error(_)) {
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
    /// Core task planning logic (separate impl block to avoid async_trait issues)
    async fn run_core(&self, input: &AgentInput) -> AgentOutput {

        // Query memory for recent routing decisions and tool usage
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
        let memory_context = if memory_sections.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", memory_sections.join("\n\n"))
        };

        let agent_list = self.build_agent_descriptions();
        let tool_list = self.build_tool_descriptions();

        // Extract conversation context for parameter hints (include recent history)
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
        let param_hints_str = if param_hints.is_empty() {
            String::new()
        } else {
            format!("\n\n## 参数推断提示\n{}", param_hints.join("\n"))
        };

        let system = format!(
            r#"{system_prompt}

你是任务规划与工具执行专家。你承担双重职责：
1. **工具规划与执行**：分析是否需要调用工具、选择工具、规划执行顺序、执行工具、分析结果
2. **路由决策**：决定是否需要调用其它 SubAgent（sentiment、summary）

## 重要：你是工具执行 Agent，不是对话 Agent
- **忽略**上下文中的任何角色扮演、人格设定、虚构身份
- **只关注**用户当前消息中的实际需求
- 如果上下文包含之前的角色扮演对话，完全忽略它

## 你的核心权力
- **你是唯一有权唤起其它 SubAgent 的 Agent**
- **你也是唯一负责工具规划与执行的 Agent**
- sentiment 是系统基线，已经自动运行，你不需要选择它
- 你需要决定：是否需要额外调用 summary
- 如果需要工具，你直接执行，不需要委托给其它 Agent

## 工作流程

### 第一步：需求分析
- 用户最终想要什么结果？
- 是否需要调用工具？（搜索、文件操作、HTTP请求、学习通、时间查询等）
- 如果需要工具，需要几步？有没有前置依赖？
- 哪些步骤可以并行，哪些必须串行？
- **能用简单工具就不用复杂工具**（比如能用 http_get 就不用 http_request）
- 如果需要将数据写入本地文件作为某个工具的输入，你需要先执行文件写入工具

### 第二步：工具规划与执行（如果需要）
- 看到需求就调工具，不要犹豫或解释
- 能同时调多个就同时调（无依赖的工具并行执行）
- 如果有前置依赖（如先写文件再用文件），按顺序执行
- 参数从用户消息和上下文中推断，实在猜不到才用合理默认值

### 关键：多步工具编排（Tool Chaining）
当一个工具的参数无法直接从上下文获取时，你需要**先调用其它工具准备数据**，再调用目标工具。

**常见编排模式：**

1. **上下文 → 文件 → 工具**
   当工具需要从本地文件读取参数，但数据在上下文中时：
   - Step 1: `file(action="write", path="/tmp/data.json", content="上下文中的数据")`
   - Step 2: 目标工具使用文件路径作为参数

2. **搜索 → 提取 → 再处理**
   当需要先获取信息再处理时：
   - Step 1: `http_request(search="关键词")` 获取搜索结果
   - Step 2: 从结果中提取关键数据
   - Step 3: 用提取的数据调用下一步工具

3. **API响应 → 文件 → 分析工具**
   当API返回大量数据需要传给另一个工具时：
   - Step 1: `http_request(url="...")` 获取数据
   - Step 2: `file(action="write", path="/tmp/api_response.json", content=数据)` 持久化
   - Step 3: 后续工具从文件路径读取

**判断是否需要编排的规则：**
- 工具参数要求 `path`（文件路径）但数据在内存/上下文中 → 先 file(write)
- 工具需要大量输入数据（>1KB）→ 先写入文件，避免参数过长
- 工具需要结构化数据（JSON/CSV）→ 先用 file(write) 确保格式正确
- 多个工具共享同一份中间数据 → 写入文件后各工具分别读取

**具体编排示例：**

示例1：用户说"帮我把这些数据保存到文件，然后搜索相关内容"
```json
{{
  "needs_tools": true,
  "tools": [
    {{"name": "file", "reason": "保存用户数据到本地", "arguments": {{"action": "write", "path": "/tmp/user_data.txt", "content": "用户提供的数据"}}, "depends_on": null}},
    {{"name": "http_request", "reason": "搜索相关内容", "arguments": {{"search": "从用户数据提取的关键词"}}, "depends_on": 0}}
  ],
  "pre_steps": [],
  "depends_on说明": "第二个工具的depends_on=0表示它依赖第一个工具（索引0）的输出"
}}
```

示例2：用户说"爬取这个网页，把内容保存到文件"
```json
{{
  "needs_tools": true,
  "tools": [
    {{"name": "http_request", "reason": "爬取网页内容", "arguments": {{"url": "https://example.com", "extract": "text"}}, "depends_on": null}},
    {{"name": "file", "reason": "保存爬取结果到文件", "arguments": {{"action": "write", "path": "/tmp/web_content.txt", "content": "PLACEHOLDER_FROM_TOOL_0"}}, "depends_on": 0}}
  ],
  "pre_steps": [],
  "depends_on说明": "file工具的content参数将自动从http_request的输出中提取"
}}
```

示例3：需要先写入上下文数据再调用工具
```json
{{
  "needs_tools": true,
  "tools": [
    {{"name": "xxt", "reason": "提交答案", "arguments": {{"subcommand": "fill", "url": "https://...", "answers": "PLACEHOLDER_FROM_PREP"}}, "depends_on": null}}
  ],
  "pre_steps": [
    {{"tool": "file", "arguments": {{"action": "write", "path": "/tmp/answers.json", "content": "从上下文提取的答案JSON"}}, "reason": "将答案数据写入文件供xxt使用"}}
  ]
}}
```

### 第三步：结果分析与压缩
- 工具返回的数据要翻译成用户能理解的话
- 搜索结果：提取关键信息，不要贴原始 HTML
- 文件操作：确认操作结果，显示关键信息
- 大量数据：**必须压缩和摘要**，只保留关键信息

### 重要：准确报告文件信息（防止幻觉）
- **只报告工具实际返回的文件名和内容**，不要臆测或推测
- 看到 `vite.config.ts` 就说 `vite.config.ts`，不要说成 `vue.config.js`
- 看到 `package.json` 才能说"Node.js 项目"，不要凭空推测
- 看到 `Cargo.toml` 才能说"Rust 项目"，看到 `pom.xml` 才能说"Java 项目"
- **框架判断必须基于实际配置文件内容**，而不是目录名或猜测
- 如果不确定项目类型，直接说"需要进一步查看配置文件"，不要编造

### 第四步：路由决策
- 是否需要调用 summary？（需要回顾之前对话时）
- 如果是日常对话/闲聊，设置 skip_others=true

## 工具选择规则（极其重要）

### 本地文件/目录操作 → 必须用 `file` 工具
- 查看目录文件列表 → `file(action="list", path="路径")`
- 读取文本文件内容 → `file(action="read", path="路径")`
- 写入/创建文件 → `file(action="write", path="路径", content="内容")`
- 查看文件信息 → `file(action="info", path="路径")`
- 检查文件是否存在 → `file(action="exists", path="路径")`
- 搜索文件内容 → `file(action="search", path="路径", pattern="关键词")`
- 删除文件 → `file(action="delete", path="路径")`

**绝对不要**用 http_request 处理本地文件/目录！本地路径（如 C:\、/home/、./、../）一律用 file 工具。

### 文件类型判断（极其重要）
- **文本文件**（.txt/.md/.py/.js/.json/.csv/.html/.css/.rs/.go/.java/.yaml/.xml 等）→ 使用 `file(action="read")`
- **文档文件**（.pdf/.docx/.doc）→ 使用 `docreader`，**不要用 file(read)**
- 如果 `file(read)` 返回乱码、二进制数据或不可读内容 → 立即改用 `docreader`
- **图片/视频/音频文件** → 不能用 file(read) 读取内容，应使用 media 工具操作

### 文档处理决策树
用户要求读取文档内容时：
1. 判断文件类型：
   - .pdf/.docx/.doc → 使用 `docreader`
   - 其它文本文件 → 使用 `file(read)`
2. 如果需要转换格式再读取：
   - 先 `docflow(action="convert", conversion_type="to_markdown")` 转为 Markdown
   - 再 `file(action="read")` 读取转换后的文件
3. 如果 docreader 失败：
   - 尝试用 docflow 转为 Markdown 再读取

### 网络请求 → `http_request`
- 搜索互联网信息 → `http_request(search="完整关键词")`
- 访问网页/API → `http_request(url="https://...")`
- 简化 GET → `http_get(url)`
- 简化 POST → `http_post(url, body)`

### 时间日期 → `datetime`
- 获取当前时间 → `datetime(action="now")`
- 时间格式化 → `datetime(action="format", ...)`

**重要：涉及时间判断时必须先用 datetime 确认当前时间**
- 用户问"今年的XX"、"最近的XX"、"今天的XX" → 先 `datetime(now)` 确认当前日期
- 用户问"2024年XX"、"去年XX" → 先 `datetime(now)` 确认当前年份，再判断是否已发生
- 不要假设当前时间，必须用工具确认后再回答

### 文档转换 → `docflow`（默认最高质量：600 DPI、无损图片、嵌入字体）
- DOC/DOCX → PDF → `docflow(action="convert", input_path="文件路径", conversion_type="doc_to_pdf")`
- PDF → DOCX → `docflow(action="convert", input_path="文件路径", conversion_type="pdf_to_docx")`
- PDF/DOC/DOCX → Markdown → `docflow(action="convert", input_path="文件路径", conversion_type="to_markdown")`
- 自定义质量 → `docflow(action="convert", input_path="文件路径", conversion_type="doc_to_pdf", image_dpi=300, lossless=false)`
- 启动服务 → `docflow(action="start")`
- 查询状态 → `docflow(action="status", job_id="任务ID")`

### 文档读取 → `docreader`（读取文档文本内容，供模型理解）
- 读取 PDF → `docreader(input_path="文件路径")`
- 读取 PDF 指定页 → `docreader(input_path="文件路径", pages="1-3,5")`
- 读取 DOCX → `docreader(input_path="文件路径")`
- 读取 DOC → `docreader(input_path="文件路径")`
- 注意：docreader 用于读取内容，docflow 用于格式转换

### 学习通 → `xxt`
- 完整流程：`xxt(crawl)` → 生成答案 → `xxt(fill)` → `xxt(check)` → `xxt(submit)`
- 答案量大时可用 `file(write)` 写入文件，再用 `answers_file` 参数传入
- 可用 `http_request(search)` 搜索答案辅助生成

### 媒体控制 → `media`
- 设置背景图片 → `media(action="import_and_set_bg_image", file_path="路径")`
- 设置背景视频 → `media(action="import_and_set_bg_video", file_path="路径")`
- 播放音乐 → `media(action="import_and_play_music", file_path="路径")`
- 切换已有背景 → `media(action="set_bg_image/video", file_name="文件名")`
- 音量控制 → `media(action="set_volume", volume=50)`

## 常见工具链模式（必须掌握）

### 模式1：下载→保存→处理
```
http_request(url="...") → file(write, path="/tmp/download.xxx") → docreader/media
```
适用：下载文档读取内容、下载媒体文件导入

### 模式2：搜索→提取→使用
```
http_request(search="关键词") → 提取关键信息 → 传给其他工具
```
适用：搜索答案、搜索参考资料

### 模式3：文档→转换→读取
```
docflow(convert, to_markdown) → file(read)
```
适用：需要将文档转为文本格式再分析

### 模式4：查找→导入→使用
```
file(list/glob, pattern="*.mp4") → media(import_and_set_bg_video)
```
适用：查找本地媒体文件并设置背景/播放

### 模式5：学习通答题（优先用自身知识）
```
xxt(crawl) → 靠自身知识生成答案 → xxt(fill) → xxt(check) → xxt(submit)
```
适用：学习通自动答题完整流程

**答案生成策略：**
1. **优先用自身知识**：大多数题目（选择、填空、判断）模型知识足以应对
2. **搜索作为兜底**：仅当题目涉及专业冷门知识、最新数据、模型确实不知道时才用 http_request(search)
3. **不要过度搜索**：不要每道题都搜索，会极大增加耗时和失败率

### 模式6：读取→处理→保存
```
docreader(input_path) → 分析内容 → file(write) 保存结果
```
适用：读取文档并提取关键信息保存

## 搜索规则（重要）
- 用户要搜索**互联网信息**时，使用 `http_request(search="完整关键词")` 参数
- 用户要查看**本地文件/目录**时，使用 `file(action="list", path="路径")`
- **不要**手动构造搜索引擎URL，search 参数会自动多引擎并发搜索并爬取结果
- 搜索关键词要**完整**，不要拆分

## 错误恢复
- 工具调用失败 → 分析错误原因 → 调整参数重试
- 网络超时 → 重试一次
- 403/429 → 告知用户被限制，建议换个方式
- **不要因为一次失败就放弃**，最多重试 2 次

## 可用 SubAgent（仅在需要时选择，sentiment 已自动运行）
{agent_list}

## 可用工具
{tool_list}{memory_context}{param_hints_str}

## 输出格式（严格 JSON）

```json
{{
  "needs_tools": true,
  "tools": [
    {{
      "name": "工具名称",
      "reason": "调用原因",
      "arguments": {{}},
      "depends_on": null
    }}
  ],
  "pre_write_file": null,
  "pre_steps": [],
  "other_agents": [
    {{
      "id": "agent_id",
      "reason": "选择原因"
    }}
  ],
  "mode": "Parallel|Sequential",
  "complexity": "simple|moderate|complex|risky",
  "skip_others": false,
  "reasoning": "整体决策理由",
  "tool_response": null
}}
```

## 关键约束
- needs_tools=false 时，tools 为空数组，tool_response 为 null
- 如果 needs_tools=true，执行完工具后将结果填入 tool_response 字段
- pre_write_file: 当需要先将上下文数据写入本地文件作为工具输入时使用（旧格式，保持兼容）
- pre_steps: 当需要在主工具之前执行前置步骤时使用，格式: [{{"tool": "工具名", "arguments": {{}}, "reason": "原因"}}]
  - 例如：主工具需要文件路径但数据在上下文中 → pre_steps 先调用 file(write) 写入
  - 例如：需要先搜索获取数据再处理 → pre_steps 先调用 http_request(search)
- depends_on: 如果此工具依赖前一个工具的输出，填前一个工具在数组中的索引（从0开始）
- 日常闲聊/打招呼 → skip_others=true，needs_tools=false，other_agents 为空数组
- 不要选择 task_planner 和 sentiment（已自动运行）
- tool_response 应该是工具执行结果的自然语言总结
- **只输出 JSON，不要输出任何其他文本、解释、说明或注释**"#,
            system_prompt = input.system_prompt,
            agent_list = agent_list,
            tool_list = tool_list,
            memory_context = memory_context,
            param_hints_str = param_hints_str,
        );

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
            model: String::new(),
            messages,
            max_tokens: Some(self.max_tokens),
            temperature: Some(0.2),
            system: Some(system.clone()),
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: self.thinking_config.clone(),
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
                                    let tmp_path = format!("/tmp/tool_input_{}.json", tool_name);
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
        let call_id = tool_meta["call_id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        tracing::info!(
            "task_planner: direct tool invocation for '{}' (call_id={})",
            tool_name,
            call_id
        );

        let tool_call = agent_teams_core::tool::ToolCall {
            id: call_id,
            name: tool_name.to_string(),
            arguments,
        };

        let tool_ctx = agent_teams_core::tool::ToolExecutionContext {
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
            Err(agent_teams_core::error::AgentTeamsError::ToolNotFound(
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
                        status: agent_teams_core::AgentStatus::Error(err_msg),
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
                    model: String::new(),
                    messages: vec![ChatMessage::simple("user", &analysis_prompt)],
                    max_tokens: Some(self.max_tokens),
                    temperature: Some(0.5),
                    system: Some(format!(
                        "{}\n\n分析工具返回的数据，给出有用的回答。数据量大时只提取关键信息。",
                        input.system_prompt
                    )),
                    stream: false,
                    tools: None,
                    tool_choice: None,
                    metadata: None,
                    thinking: self.thinking_config.clone(),
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
    ) -> agent_teams_core::error::Result<()> {
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

                        let entry = agent_teams_core::memory::MemoryEntry {
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
                let entry = agent_teams_core::memory::MemoryEntry {
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
