use std::sync::Arc;

use async_trait::async_trait;
use tracing;

use agent_teams_core::agent_memory_cache::AgentMemoryCache;
use agent_teams_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::memory::{MemoryKind, MemoryQuery};
use agent_teams_core::memory_store::MemoryStore;
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ThinkingConfig};
use agent_teams_core::tool::{ToolExecutor, UnifiedToolRegistry};

use crate::tool_param_infer::{ParameterInferrer, ConversationContext, build_parameter_hints};

/// ToolAgent: dedicated tool execution agent.
///
/// Responsibilities:
/// - Plans which tools to call and in what order
/// - Executes tools via the AgentToolLoop (ReAct pattern)
/// - Uses deep thinking to analyze large tool results without blowing up context
/// - Summarizes and compresses tool output before returning
///
/// This agent is the single owner of all tool-related operations.
pub struct ToolAgent {
    provider: Arc<dyn LlmProvider>,
    unified_registry: Option<Arc<UnifiedToolRegistry>>,
    tool_executor: Option<Arc<dyn ToolExecutor>>,
    tool_engine: Option<Arc<crate::tool_engine::ToolExecutionEngine>>,
    agent_memory_cache: AgentMemoryCache,
    resource_pool: Arc<agent_teams_core::tool::ResourcePool>,
    param_inferrer: Option<Arc<ParameterInferrer>>,
    thinking_config: Option<ThinkingConfig>,
}

impl ToolAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            unified_registry: None,
            tool_executor: None,
            tool_engine: None,
            agent_memory_cache: AgentMemoryCache::new("tool_agent".to_string(), 100),
            resource_pool: Arc::new(agent_teams_core::tool::ResourcePool::new()),
            param_inferrer: None,
            thinking_config: None,
        }
    }

    pub fn with_thinking_config(mut self, config: Option<ThinkingConfig>) -> Self {
        self.thinking_config = config;
        self
    }

    pub fn with_tool_engine(
        mut self,
        engine: Arc<crate::tool_engine::ToolExecutionEngine>,
    ) -> Self {
        self.tool_engine = Some(engine);
        self
    }

    pub fn with_agent_memory_cache(mut self, cache: AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
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

    pub fn with_param_inferrer(mut self, inferrer: Arc<ParameterInferrer>) -> Self {
        self.param_inferrer = Some(inferrer);
        self
    }

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

                        format!("- {}{}: {}", t.name, params_str, t.description)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            None => "（工具注册表未配置）".to_string(),
        }
    }

    fn available_tools(&self) -> Vec<agent_teams_core::tool::Tool> {
        self.unified_registry
            .as_ref()
            .map(|r| r.list_tools())
            .unwrap_or_default()
    }
}

#[async_trait]
impl BoxedAgent for ToolAgent {
    fn id(&self) -> &str {
        "tool_agent"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["tool_request".to_string(), "tool_planning".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 200,
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
            return self.execute_direct_tool_call(input, tool_call_json).await;
        }

        // Query memory for recent tool usage to avoid redundant calls
        let query = MemoryQuery {
            text: input.content.clone(),
            kinds: vec![MemoryKind::AgentOutput],
            tags: vec!["tool_execution".to_string()],
            limit: 3,
            session_id: input.session_id.clone(),
            ..Default::default()
        };
        let memories = self.agent_memory_cache.query(&query).await;
        let memory_context = if !memories.is_empty() {
            let tool_history: Vec<String> = memories
                .iter()
                .filter(|m| m.tags.contains(&"tool_execution".to_string()))
                .map(|m| format!("- {}", m.content))
                .collect();
            if tool_history.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\n## 最近工具调用记录（避免重复调用）\n{}",
                    tool_history.join("\n")
                )
            }
        } else {
            String::new()
        };

        let tool_list = self.build_tool_descriptions();

        // Extract conversation context for parameter hints
        let messages = vec![ChatMessage::simple("user", &input.content)];
        let context = if let Some(ref inferrer) = self.param_inferrer {
            inferrer.extract_context(&messages)
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

        // Memory, system instructions, and domain state are already in system_prompt
        // from build_system_prompt() — no need to inject them again.

        let system = format!(
            r#"{system_prompt}

你是统一的工具规划与执行专家。你的职责：分析需求 → 选择工具 → 规划执行顺序 → 执行工具 → 分析结果 → 呈现给用户。

## 重要：你是工具执行 Agent，不是对话 Agent
- **忽略**上下文中的任何角色扮演、人格设定、虚构身份（如"小猫娘"、"猫娘"等）
- **只关注**用户当前消息中的实际需求和工具调用指令
- 你不是在扮演角色，你是在**执行工具**
- 如果上下文包含之前的角色扮演对话，完全忽略它，只处理当前的工具需求

## 工作流程

### 第一步：需求分析与工具规划
- 用户最终想要什么结果？
- 需要几步才能达到？有没有前置依赖？
- 哪些步骤可以并行，哪些必须串行？
- **能用简单工具就不用复杂工具**（比如能用 http_get 就不用 http_request）

### 第二步：工具执行
- 看到需求就调工具，不要犹豫或解释
- 能同时调多个就同时调（无依赖的工具并行执行）
- 参数从用户消息和上下文中推断，实在猜不到才用合理默认值

### 第三步：结果分析与压缩
- 工具返回的数据要翻译成用户能理解的话
- 搜索结果：提取关键信息，不要贴原始 HTML
- 文件操作：确认操作结果，显示关键信息
- 大量数据：**必须压缩和摘要**，只保留关键信息
- 错误信息：说清楚出了什么问题，给出建议

## 错误恢复
- 工具调用失败 → 分析错误原因 → 调整参数重试
- 网络超时 → 重试一次
- 403/429 → 告知用户被限制，建议换个方式
- **不要因为一次失败就放弃**，最多重试 2 次

## 工具速查
- `http_request(search="关键词")` — 搜索互联网（自动多引擎百度/Bing/Google，自动爬取结果页面）
- `http_request(url?, urls?, method?, extract?)` — 通用 HTTP / 批量请求
- `http_get(url)` — 简化 GET
- `http_post(url, body)` — 简化 POST
- `file(action, path?, content?)` — 文件操作
- `datetime(action, ...)` — 时间日期
- `xxt` — 学习通自动化（登录/答题/提交）

## 搜索规则（重要）
- 用户要搜索信息时，**必须**使用 `http_request(search="完整关键词")` 参数
- **不要**手动构造搜索引擎URL，search 参数会自动多引擎并发搜索并爬取结果
- 搜索关键词要**完整**，不要拆分（如搜"操作系统"不要拆成"操作"）
- **不要**使用翻译或词典网站来搜索知识内容

可用工具：
{tool_list}{memory_context}{param_hints_str}"#,
            system_prompt = input.system_prompt,
            tool_list = tool_list,
            memory_context = memory_context,
            param_hints_str = param_hints_str,
        );

        // Use AgentToolLoop for unified planning + execution (ReAct pattern)
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
                agent_id: "tool_agent".to_string(),
                request_id: uuid::Uuid::new_v4().to_string(),
                tool_history: vec![],
                resources: self.resource_pool.clone(),
                agent_context: input.agent_context.clone(),
            };

            match tool_loop.run(messages, tools, &tool_ctx).await {
                Ok((output, tool_history)) => {
                    // Compress output if too large
                    let content = Self::compress_output(&output.content);

                    // Emit ToolTrigger effects for each executed tool so the frontend
                    // can display tool status animations
                    let effects: Vec<AgentEffect> = tool_history
                        .iter()
                        .map(|(call, _result)| AgentEffect::ToolTrigger {
                            tool_name: call.name.clone(),
                            input: call.arguments.clone(),
                            agent_id: "tool_agent".to_string(),
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
}

impl ToolAgent {
    /// Maximum output size before compression kicks in (32KB)
    const MAX_OUTPUT_SIZE: usize = 32768;
    /// Target compressed size
    const COMPRESSED_SIZE: usize = 16000;

    /// Compress large tool outputs to prevent context bloat.
    /// Keeps the beginning and end, summarizes the middle.
    fn compress_output(content: &str) -> String {
        if content.len() <= Self::MAX_OUTPUT_SIZE {
            return content.to_string();
        }

        tracing::info!(
            "Compressing tool output: {} chars -> target {} chars",
            content.len(),
            Self::COMPRESSED_SIZE
        );

        let chars: Vec<char> = content.chars().collect();
        let total = chars.len();

        // Keep first 60% and last 20%, summarize middle
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
            "tool_agent: direct tool invocation for '{}' (call_id={})",
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
            agent_id: "tool_agent".to_string(),
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
                    return AgentOutput {
                        content: format!(
                            "工具 `{}` 执行失败：{}",
                            tool_name,
                            tool_result.error.unwrap_or_else(|| "未知错误".to_string())
                        ),
                        quality: 0.3,
                        ..Default::default()
                    };
                }

                let tool_output = tool_result.output.to_string();

                // Fast path: skip LLM analysis for small/simple results
                let is_simple = tool_output.len() < 200
                    || tool_result.output.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let is_medium = tool_output.len() < 2000 && !tool_output.contains("error");

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
                            agent_id: "tool_agent".to_string(),
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

                // Compress the tool output before sending to LLM
                let compressed_output = Self::compress_output(&tool_output);

                let analysis_prompt = format!(
                    "工具 `{}` 执行成功（{}ms），返回数据：\n{}\n\n用户问题：{}\n\n分析数据并回答用户问题。如果数据量大，提取关键信息即可。",
                    tool_name, tool_result.execution_duration_ms, compressed_output, user_question
                );

                let analysis_request = CompletionRequest {
                    model: String::new(),
                    messages: vec![ChatMessage::simple("user", &analysis_prompt)],
                    max_tokens: Some(32768),
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
                            agent_id: "tool_agent".to_string(),
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

    /// Single-shot fallback: LLM calls tools natively, then continues with tool results.
    async fn run_single_shot(
        &self,
        input: AgentInput,
        system: String,
        available_tools: Vec<agent_teams_core::tool::Tool>,
    ) -> AgentOutput {
        let request = CompletionRequest {
            model: String::new(),
            messages: vec![ChatMessage::simple("user", &input.content)],
            max_tokens: Some(32768),
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

                // No tool calls — return directly
                if resp.tool_calls.is_empty() {
                    return AgentOutput {
                        content: Self::compress_output(&resp.content),
                        thinking: resp.thinking,
                        quality: 0.85,
                        ..Default::default()
                    };
                }

                // Execute tool calls with parameter inference
                let mut tool_results: Vec<(String, agent_teams_core::tool::ToolResult)> = Vec::new();
                let messages_for_inference = vec![ChatMessage::simple("user", &input.content)];

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
                        agent_id: "tool_agent".to_string(),
                    });

                    let tool_ctx = agent_teams_core::tool::ToolExecutionContext {
                        session_id: input.session_id.clone().unwrap_or_default(),
                        user_id: input.user_id.clone(),
                        agent_id: "tool_agent".to_string(),
                        request_id: uuid::Uuid::new_v4().to_string(),
                        tool_history: vec![],
                        resources: self.resource_pool.clone(),
                        agent_context: input.agent_context.clone(),
                    };

                    let result = if let Some(ref engine) = self.tool_engine {
                        engine.execute_with_resilience(&enriched_tc, &tool_ctx).await
                    } else if let Some(ref executor) = self.tool_executor {
                        executor.execute(&enriched_tc, &tool_ctx).await
                    } else {
                        Err(agent_teams_core::error::AgentTeamsError::ToolNotFound(
                            "No tool engine or executor configured".to_string(),
                        ))
                    };

                    match result {
                        Ok(r) => tool_results.push((enriched_tc.id.clone(), r)),
                        Err(e) => tool_results.push((enriched_tc.id.clone(), agent_teams_core::tool::ToolResult {
                            call_id: enriched_tc.id.clone(),
                            name: enriched_tc.name.clone(),
                            success: false,
                            output: serde_json::Value::Null,
                            error: Some(e.to_string()),
                            execution_duration_ms: 0,
                        })),
                    }
                }

                // Build continuation messages with proper tool result protocol
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
                    // Compress tool results before adding to messages
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
                    max_tokens: Some(32768),
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
impl MemoryAwareAgent for ToolAgent {
    fn memory_cache(&self) -> &AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> agent_teams_core::error::Result<()> {
        if !output.effects.is_empty() {
            let tool_names: Vec<String> = output
                .effects
                .iter()
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
                    source_agent: "tool_agent".to_string(),
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
