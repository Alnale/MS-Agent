use std::sync::Arc;

use agent_teams_core::agent_memory_cache::ExecutionPolicy;
use agent_teams_core::boxed_agent::{AgentInput, AgentOutput};
use agent_teams_core::context::AgentContext;
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::message::{AgentMessage, AgentStatus};
use agent_teams_core::pipeline::StageMode;
use agent_teams_core::plan::{ExecutionPlan, PlanExecutionState, PlanNode, PlanStage, ToolIntent};
use agent_teams_core::registry::AgentRegistry;
use agent_teams_core::tool::UnifiedToolRegistry;

use agent_teams_core::unified_memory_bus::UnifiedMemoryBus;

use crate::orchestrator::Orchestrator;
use crate::sub_agent_cache::SubAgentCache;

/// Build AgentInput from context + message + prior effects.
/// Accepts a pre-wrapped `Arc<AgentContext>` to avoid repeated deep clones.
pub fn build_input(
    ctx: &Arc<AgentContext>,
    msg: &AgentMessage,
    prior_effects: Arc<Vec<AgentEffect>>,
    prior_context: &str,
) -> AgentInput {
    let mut system_prompt = ctx.build_system_prompt();
    if !prior_context.is_empty() {
        system_prompt.push_str(&format!(
            "\n\n## 之前阶段的执行结果（供参考）\n{}",
            prior_context
        ));
    }
    AgentInput {
        system_prompt,
        content: msg.content.clone(),
        recent_history: ctx.recent_history.as_ref().clone(),
        prior_effects,
        session_id: Some(ctx.session_id.clone()),
        user_id: ctx.user_id.clone(),
        available_tools: Vec::new(),
        agent_context: Some(ctx.clone()),
    }
}

/// Generate a stable sub-agent cache key using FNV-1a.
/// Key includes request_key (session_id) to prevent cross-session cache pollution.
fn sub_agent_cache_key(request_key: &str, agent_id: &str, content: &str) -> String {
    agent_teams_core::hash::fnv1a_hash_str(&[request_key, agent_id, content])
}

/// Pipeline executor: executes an ExecutionPlan with optional caching
pub struct PipelineExecutor {
    cache: Option<Arc<SubAgentCache>>,
    unified_bus: Option<Arc<UnifiedMemoryBus>>,
    tool_registry: Option<Arc<UnifiedToolRegistry>>,
    default_timeout_ms: u64,
}

impl PipelineExecutor {
    pub fn new(default_timeout_ms: u64) -> Self {
        Self {
            cache: None,
            unified_bus: None,
            tool_registry: None,
            default_timeout_ms,
        }
    }

    pub fn with_cache(mut self, cache: Arc<SubAgentCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_unified_bus(mut self, bus: Arc<UnifiedMemoryBus>) -> Self {
        self.unified_bus = Some(bus);
        self
    }

    pub fn with_tool_registry(mut self, registry: Arc<UnifiedToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Set unified bus after construction (for deferred initialization)
    pub fn set_unified_bus(&mut self, bus: Arc<UnifiedMemoryBus>) {
        self.unified_bus = Some(bus);
    }

    /// Set tool registry after construction
    pub fn set_tool_registry(&mut self, registry: Arc<UnifiedToolRegistry>) {
        self.tool_registry = Some(registry);
    }

    /// Extract the first URL from message content
    fn extract_url_from_content(content: &str) -> String {
        if let Some(start) = content.find("http") {
            let url_part = &content[start..];
            // Stop at whitespace, brackets, or non-ASCII characters (e.g. Chinese text after URL)
            let end = url_part.find(|c: char| {
                c.is_whitespace() || c == ')' || c == ']' || c == '>' || c == '"' || c == '\''
                    || (c as u32) > 127
            })
            .unwrap_or(url_part.len());
            url_part[..end].to_string()
        } else {
            String::new()
        }
    }

    /// Build tool-specific arguments based on tool name, subcommand, and user message.
    /// Different tools expect different parameter names (action vs subcommand, etc.).
    fn build_tool_arguments(
        tool_name: &str,
        subcommand: &str,
        content: &str,
    ) -> serde_json::Value {
        match tool_name {
            "datetime" => {
                let action = if !subcommand.is_empty() {
                    subcommand.to_string()
                } else {
                    Self::infer_datetime_action(content)
                };
                serde_json::json!({ "action": action })
            }
            "file" => {
                let action = if !subcommand.is_empty() {
                    subcommand.to_string()
                } else {
                    Self::infer_file_action(content)
                };
                let mut args = serde_json::json!({ "action": action });
                // Extract path if present
                if let Some(path) = Self::extract_file_path(content) {
                    args["path"] = serde_json::Value::String(path);
                }
                args
            }
            "xxt" => {
                serde_json::json!({
                    "subcommand": subcommand,
                    "url": Self::extract_url_from_content(content),
                })
            }
            "http_request" | "http_get" | "http_post" => {
                let url = Self::extract_url_from_content(content);
                if !url.is_empty() {
                    serde_json::json!({ "url": url })
                } else {
                    serde_json::json!({})
                }
            }
            "media" => {
                let action = if !subcommand.is_empty() {
                    subcommand.to_string()
                } else {
                    Self::infer_media_action(content)
                };
                let mut args = serde_json::json!({ "action": action });
                if let Some(path) = Self::extract_file_path(content) {
                    args["file_path"] = serde_json::Value::String(path);
                }
                args
            }
            "docflow" => {
                let action = if !subcommand.is_empty() {
                    subcommand.to_string()
                } else {
                    "convert".to_string()
                };
                serde_json::json!({ "action": action })
            }
            "docreader" => {
                let mut args = serde_json::json!({});
                if let Some(path) = Self::extract_file_path(content) {
                    args["input_path"] = serde_json::Value::String(path);
                }
                args
            }
            _ => serde_json::json!({}),
        }
    }

    /// Infer datetime action from user message
    fn infer_datetime_action(content: &str) -> String {
        let lower = content.to_lowercase();
        if lower.contains("时间戳") || lower.contains("timestamp") || lower.contains("unix") {
            if lower.contains("转") || lower.contains("from") { "from_unix".to_string() } else { "to_unix".to_string() }
        } else if lower.contains("时间差") || lower.contains("相差") || lower.contains("间隔") {
            "diff".to_string()
        } else if lower.contains("格式") || lower.contains("format") {
            "format".to_string()
        } else {
            "now".to_string()
        }
    }

    /// Infer file action from user message
    fn infer_file_action(content: &str) -> String {
        let lower = content.to_lowercase();
        if lower.contains("写") || lower.contains("保存") || lower.contains("创建") || lower.contains("新建") {
            "write".to_string()
        } else if lower.contains("列") || lower.contains("目录") {
            "list".to_string()
        } else if lower.contains("删") {
            "delete".to_string()
        } else if lower.contains("搜索") || lower.contains("grep") || lower.contains("查找") {
            "search".to_string()
        } else if lower.contains("信息") || lower.contains("大小") || lower.contains("存在") {
            "info".to_string()
        } else {
            "read".to_string()
        }
    }

    /// Infer media action from user message
    fn infer_media_action(content: &str) -> String {
        let lower = content.to_lowercase();
        if lower.contains("自定义视频") || lower.contains("切换到视频") || lower.contains("激活视频") || lower.contains("视频背景") {
            "activate_bg_video".to_string()
        } else if lower.contains("自定义图片") || lower.contains("切换到图片") || lower.contains("激活图片") || lower.contains("图片背景") {
            "activate_bg_image".to_string()
        } else if lower.contains("导入") && lower.contains("视频") {
            "import_and_set_bg_video".to_string()
        } else if lower.contains("导入") && lower.contains("图片") {
            "import_and_set_bg_image".to_string()
        } else if lower.contains("导入") && lower.contains("音乐") {
            "import_and_play_music".to_string()
        } else if lower.contains("设置") && lower.contains("视频") {
            "set_bg_video".to_string()
        } else if lower.contains("设置") && lower.contains("图片") {
            "set_bg_image".to_string()
        } else if lower.contains("播放") && !lower.contains("暂停") {
            "play_music".to_string()
        } else if lower.contains("暂停") {
            "pause_music".to_string()
        } else if lower.contains("恢复") {
            "resume_music".to_string()
        } else if lower.contains("下一首") || lower.contains("切歌") || lower.contains("换歌") {
            "next_track".to_string()
        } else if lower.contains("上一首") {
            "prev_track".to_string()
        } else if lower.contains("静音") {
            "toggle_mute".to_string()
        } else if lower.contains("音量") {
            "set_volume".to_string()
        } else if lower.contains("清除背景") || lower.contains("恢复默认") || lower.contains("默认背景") || lower.contains("切换回默认") {
            "clear_bg".to_string()
        } else if lower.contains("状态") {
            "get_status".to_string()
        } else if lower.contains("背景") {
            "activate_bg_video".to_string()
        } else {
            "get_status".to_string()
        }
    }

    /// Extract a file path from user message (simple heuristic)
    fn extract_file_path(content: &str) -> Option<String> {
        // Pattern 1: Look for Windows drive letter paths (C:\... or C:/...)
        // This handles cases like "帮我看看C:\Users\..." where path has no leading space
        for (i, ch) in content.char_indices() {
            if ch.is_ascii_alphabetic() && i + 1 < content.len() {
                let rest = &content[i + 1..];
                if rest.starts_with(':') && rest.len() > 2 {
                    let after_colon = &rest[1..];
                    if after_colon.starts_with('\\') || after_colon.starts_with('/') {
                        // Found a drive path, extract until whitespace or end
                        let path_end = content[i..].find(|c: char| c.is_whitespace() && c != ' ')
                            .or_else(|| content[i..].find(char::is_whitespace))
                            .unwrap_or(content.len() - i);
                        let path = &content[i..i + path_end];
                        // Trim trailing punctuation
                        let cleaned = path.trim_end_matches(['"', '\'', '`', '。', '，', '.', ',']);
                        if cleaned.len() > 3 {
                            return Some(cleaned.to_string());
                        }
                    }
                }
            }
        }

        // Pattern 2: Look for Unix-style paths (/home/... or ./...)
        for (i, ch) in content.char_indices() {
            if ch == '/' && i + 1 < content.len() {
                let next = content[i + 1..].chars().next();
                if next.map(|c| c.is_alphanumeric() || c == '.' || c == '~').unwrap_or(false) {
                    let path_end = content[i..].find(char::is_whitespace)
                        .unwrap_or(content.len() - i);
                    let path = &content[i..i + path_end];
                    let cleaned = path.trim_end_matches(['"', '\'', '`', '。', '，', '.', ',']);
                    if cleaned.len() > 1 {
                        return Some(cleaned.to_string());
                    }
                }
            }
        }

        // Pattern 3: Fallback - look for words with path separators
        for word in content.split_whitespace() {
            if word.contains('/') || word.contains('\\') {
                let cleaned = word.trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
                if cleaned.len() > 1 {
                    return Some(cleaned.to_string());
                }
            }
        }

        None
    }

    /// Look up cached output for an agent (returns None if cache bypassed or miss)
    async fn get_cached_output(
        &self,
        policy: &ExecutionPolicy,
        session_id: &str,
        agent_id: &str,
        content: &str,
    ) -> Option<AgentOutput> {
        if policy.cache_mode == agent_teams_core::agent_memory_cache::CacheMode::Bypass {
            return None;
        }
        let cache = self.cache.as_ref()?;
        let cache_key = sub_agent_cache_key(session_id, agent_id, content);
        cache.get(&cache_key, false).await
    }

    /// Build a summary of prior agent results for cross-stage context injection.
    /// Agents in later stages can see what earlier agents produced.
    fn build_prior_context(results: &[(String, AgentOutput)]) -> String {
        if results.is_empty() {
            return String::new();
        }

        let summaries: Vec<String> = results
            .iter()
            .filter(|(_, r)| !r.content.is_empty() && r.quality > 0.2)
            .map(|(id, r)| {
                let content_preview: String = r.content.chars().take(800).collect();
                let quality_label = if r.quality >= 0.8 {
                    "高"
                } else if r.quality >= 0.5 {
                    "中"
                } else {
                    "低"
                };
                let status_label = match &r.status {
                    agent_teams_core::message::AgentStatus::Success => "",
                    agent_teams_core::message::AgentStatus::Timeout => "[超时]",
                    agent_teams_core::message::AgentStatus::Error(_) => "[错误]",
                    _ => "",
                };
                format!("[{}][可信度:{}{}] {}", id, quality_label, status_label, content_preview)
            })
            .collect();

        if summaries.is_empty() {
            String::new()
        } else {
            format!(
                "## 其他角度的分析（酌情参考）\n{}",
                summaries.join("\n")
            )
        }
    }

    /// Execute plan with execution policy (enforces SubAgent calls, smart caching)
    pub async fn execute_with_policy(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        plan: &ExecutionPlan,
        registry: &AgentRegistry,
        policy: &ExecutionPolicy,
    ) -> Vec<(String, AgentOutput)> {
        let mut all_results = Vec::new();
        let tool_intent = plan.tool_intent.clone();

        for stage in &plan.stages {
            // Build context from prior stage results for cross-stage collaboration
            let prior_context = Self::build_prior_context(&all_results);

            let stage_results = match stage.mode {
                StageMode::Parallel => {
                    self.run_parallel_with_policy(ctx, msg, stage, registry, policy, &prior_context, &tool_intent)
                        .await
                }
                StageMode::Sequential => {
                    self.run_sequential_with_policy(ctx, msg, stage, registry, policy, &prior_context, &tool_intent)
                        .await
                }
            };

            // Inject prior context into stage results for next stage awareness
            all_results.extend(stage_results);

            // Re-inject prior context summary so next stage agents can see it
            if !prior_context.is_empty() {
                tracing::debug!(
                    "Cross-stage context available: {} chars from {} prior agents",
                    prior_context.len(),
                    all_results.len()
                );
            }
        }

        // Validate minimum call count (excluding main_agent)
        let actual_calls = all_results
            .iter()
            .filter(|(id, _)| id != "main_agent")
            .count();

        if policy.force_sub_agent && actual_calls < policy.min_sub_agent_calls {
            tracing::warn!(
                "SubAgent calls {} < minimum {}, forcing fallback",
                actual_calls,
                policy.min_sub_agent_calls
            );
            let fallback = self.run_fallback_agents(ctx, msg, registry).await;
            all_results.extend(fallback);
        }

        all_results
    }

    /// Force-mode parallel execution: cache used for hard entries, soft entries re-executed
    #[allow(clippy::too_many_arguments)]
    async fn run_parallel_with_policy(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        stage: &PlanStage,
        registry: &AgentRegistry,
        policy: &ExecutionPolicy,
        prior_context: &str,
        tool_intent: &Option<ToolIntent>,
    ) -> Vec<(String, AgentOutput)> {
        let mut results = Vec::new();
        let mut to_execute: Vec<(String, Option<AgentOutput>)> = Vec::new();

        // Gather all agents and their cached results (cache as context, never skip)
        for agent_id in &stage.sub_agent_ids {
            if agent_id == "main_agent" {
                continue;
            }

            let cached_output = self.get_cached_output(policy, &ctx.session_id, agent_id, &msg.content).await;

            if cached_output.is_some() {
                tracing::debug!(
                    "Cache hit for {}, injecting as context (still executing)",
                    agent_id
                );
            }
            to_execute.push((agent_id.clone(), cached_output));
        }

        // Execute ALL SubAgents — cache results injected as context, never skipped
        let mut handles = Vec::new();
        for (agent_id, cached) in &to_execute {
            if let Some(agent) = registry.get(agent_id).await {
                let mut input = build_input(ctx, msg, ctx.turn_effects.clone(), prior_context);

                // Inject tool intent for task_planner via [TOOL_CALL] metadata
                if agent_id == "task_planner" {
                    if let Some(ref intent) = tool_intent {
                        let subcommand_hint = intent.subcommand.as_deref().unwrap_or("");
                        let arguments = Self::build_tool_arguments(&intent.tool_name, subcommand_hint, &msg.content);
                        let tool_meta = serde_json::json!({
                            "tool_name": intent.tool_name,
                            "arguments": arguments,
                            "call_id": uuid::Uuid::new_v4().to_string(),
                        });
                        input.content = format!(
                            "[TOOL_CALL]\n{}\n[/TOOL_CALL]\n\n{}",
                            tool_meta, msg.content
                        );
                        tracing::info!(
                            "Injected tool_intent [TOOL_CALL] for task_planner: tool={}, args={}",
                            intent.tool_name, arguments
                        );
                    }
                }

                // Inject cached result as context reference in system_prompt
                if let Some(ref cached_output) = cached {
                    input.system_prompt.push_str(&format!(
                        "\n\n[之前的结果，供参考]\n{}\n",
                        cached_output.content.chars().take(500).collect::<String>()
                    ));
                }

                let id = agent_id.clone();
                let timeout_ms = stage.timeout_ms.unwrap_or(self.default_timeout_ms);
                let handle = tokio::spawn(async move {
                    let result = tokio::time::timeout(
                        std::time::Duration::from_millis(timeout_ms),
                        agent.run(input),
                    )
                    .await;
                    match result {
                        Ok(output) => (id, output),
                        Err(_) => (
                            id,
                            AgentOutput {
                                content: format!("Agent timed out after {}ms", timeout_ms),
                                status: AgentStatus::Timeout,
                                ..Default::default()
                            },
                        ),
                    }
                });
                handles.push(handle);
            }
        }

        // Collect results
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => tracing::error!("SubAgent task panicked: {}", e),
            }
        }

        // Write results to cache (for next time as context reference)
        if policy.cache_mode != agent_teams_core::agent_memory_cache::CacheMode::Bypass {
            if let Some(cache) = &self.cache {
                for (agent_id, output) in &results {
                    let cache_key = sub_agent_cache_key(&ctx.session_id, agent_id, &msg.content);
                    cache.put(cache_key, output.clone(), agent_id, false).await;
                }
            }
        }

        results
    }

    /// Force-mode sequential execution: cache used for hard entries, soft entries re-executed
    #[allow(clippy::too_many_arguments)]
    async fn run_sequential_with_policy(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        stage: &PlanStage,
        registry: &AgentRegistry,
        policy: &ExecutionPolicy,
        prior_context: &str,
        tool_intent: &Option<ToolIntent>,
    ) -> Vec<(String, AgentOutput)> {
        let mut results = Vec::new();
        let mut accumulated_effects: Arc<Vec<AgentEffect>> = Arc::new(Vec::new());
        let stage_msg = stage
            .message_override
            .clone()
            .unwrap_or_else(|| msg.clone());

        for agent_id in &stage.sub_agent_ids {
            if let Some(agent) = registry.get(agent_id).await {
                // Check cache for context injection (never skip execution)
                let cached_output = self.get_cached_output(policy, &ctx.session_id, agent_id, &msg.content).await;

                let mut input = build_input(ctx, &stage_msg, accumulated_effects.clone(), prior_context);

                // Inject tool intent for task_planner via [TOOL_CALL] metadata
                if agent_id == "task_planner" {
                    if let Some(ref intent) = tool_intent {
                        let subcommand_hint = intent.subcommand.as_deref().unwrap_or("");
                        let arguments = Self::build_tool_arguments(&intent.tool_name, subcommand_hint, &msg.content);
                        let tool_meta = serde_json::json!({
                            "tool_name": intent.tool_name,
                            "arguments": arguments,
                            "call_id": uuid::Uuid::new_v4().to_string(),
                        });
                        input.content = format!(
                            "[TOOL_CALL]\n{}\n[/TOOL_CALL]\n\n{}",
                            tool_meta, msg.content
                        );
                        tracing::info!(
                            "Injected tool_intent [TOOL_CALL] for task_planner: tool={}, args={}",
                            intent.tool_name, arguments
                        );
                    }
                }

                // Inject cached result as context reference
                if let Some(ref cached_output) = cached_output {
                    input.system_prompt.push_str(&format!(
                        "\n\n[之前的结果，供参考]\n{}\n",
                        cached_output.content.chars().take(500).collect::<String>()
                    ));
                }

                // Inject prior sibling results within same stage for collaboration
                if !results.is_empty() {
                    let prior_context = Self::build_prior_context(&results);
                    if !prior_context.is_empty() {
                        input.system_prompt.push_str(&format!(
                            "\n\n{}",
                            prior_context
                        ));
                    }
                }

                let timeout = stage.timeout_ms.unwrap_or(self.default_timeout_ms);

                let result = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout),
                    agent.run(input),
                )
                .await;

                let output = match result {
                    Ok(output) => {
                        Arc::make_mut(&mut accumulated_effects).extend_from_slice(&output.effects);
                        output
                    }
                    Err(_) => AgentOutput {
                        content: format!("Agent {} timed out", agent_id),
                        status: AgentStatus::Timeout,
                        ..Default::default()
                    },
                };

                // Write as soft cache
                if policy.cache_mode != agent_teams_core::agent_memory_cache::CacheMode::Bypass {
                    if let Some(cache) = &self.cache {
                        let cache_key = sub_agent_cache_key(&ctx.session_id, agent_id, &msg.content);
                        cache.put(cache_key, output.clone(), agent_id, false).await;
                    }
                }

                results.push((agent_id.clone(), output));
            }
        }

        results
    }

    /// Execute a PlanNode-based plan using the Orchestrator.
    /// All tool execution is delegated to the task_planner SubAgent via the orchestrator.
    pub async fn execute_plan_with_nodes(
        &self,
        ctx: &Arc<AgentContext>,
        plan: &ExecutionPlan,
        registry: &Arc<AgentRegistry>,
        memory_manager: &Option<Arc<crate::memory_manager::MemoryManager>>,
        current_msg: &str,
    ) -> Vec<(String, AgentOutput)> {
        if plan.nodes.is_empty() {
            // Fall back to legacy stage-based execution
            let policy = ExecutionPolicy::default();
            return self
                .execute_with_policy(
                    ctx,
                    &AgentMessage::new(String::new()),
                    plan,
                    registry.as_ref(),
                    &policy,
                )
                .await;
        }

        let mut orchestrator = Orchestrator::new(
            registry.clone(),
            memory_manager.clone(),
        );

        // Set tool registry if available
        if let Some(tool_reg) = &self.tool_registry {
            orchestrator = orchestrator.with_tool_registry(tool_reg.clone());
        }

        let mut state = PlanExecutionState::default();
        let mut results = Vec::new();

        for (idx, node) in plan.nodes.iter().enumerate() {
            state.current_index = idx;
            // Extract a human-readable label from the PlanNode.
            // For nested nodes (Parallel/Sequential), collect child names.
            let node_label = Self::extract_node_label(node);

            match orchestrator.execute_node(ctx, node, &mut state, current_msg).await {
                Ok(value) => {
                    if let Ok(output) = serde_json::from_value::<AgentOutput>(value.clone()) {
                        tracing::info!(
                            "Node '{}' (idx={}) → AgentOutput directly (quality={:.2}, effects={})",
                            node_label, idx, output.quality, output.effects.len()
                        );
                        results.push((node_label, output));
                    } else if let Some(agent_val) = value.get("agent") {
                        // Combined result from orchestrator: {"agent": {...}, "tool_results": [...]}
                        // Extract the agent output to preserve thinking/quality/effects
                        let agent_output = serde_json::from_value::<AgentOutput>(agent_val.clone())
                            .unwrap_or_else(|_| AgentOutput {
                                content: agent_val.to_string(),
                                quality: 0.7,
                                ..Default::default()
                            });
                        tracing::info!(
                            "Node '{}' (idx={}) → combined result, extracted agent (quality={:.2}, effects={})",
                            node_label, idx, agent_output.quality, agent_output.effects.len()
                        );
                        results.push((node_label, agent_output));
                    } else {
                        tracing::info!(
                            "Node '{}' (idx={}) → fallback string conversion",
                            node_label, idx
                        );
                        results.push((
                            node_label,
                            AgentOutput {
                                content: value.to_string(),
                                quality: 0.7,
                                ..Default::default()
                            },
                        ));
                    }
                    state.node_results.push(value);
                }
                Err(e) => {
                    tracing::error!("Node '{}' (idx={}) execution failed: {}", node_label, idx, e);
                    results.push((
                        node_label,
                        AgentOutput {
                            content: format!("Node execution failed: {}", e),
                            status: AgentStatus::Error(e.to_string()),
                            ..Default::default()
                        },
                    ));
                    if matches!(node, PlanNode::Agent { .. } | PlanNode::Tool { .. }) {
                        break;
                    }
                }
            }
        }

        results
    }

    /// Extract a human-readable label from a PlanNode.
    /// For nested nodes (Parallel/Sequential/Condition), recursively collects
    /// child agent_ids and tool_names joined with "+".
    fn extract_node_label(node: &PlanNode) -> String {
        match node {
            PlanNode::Agent { agent_id, .. } => agent_id.clone(),
            PlanNode::Tool { tool_name, .. } => tool_name.clone(),
            PlanNode::Parallel(children) | PlanNode::Sequential(children) => {
                let labels: Vec<String> = children.iter().map(Self::extract_node_label).collect();
                labels.join("+")
            }
            PlanNode::Condition { then_branch, else_branch, .. } => {
                let mut labels: Vec<String> = then_branch.iter().map(Self::extract_node_label).collect();
                labels.extend(else_branch.iter().map(Self::extract_node_label));
                format!("cond({})", labels.join("|"))
            }
        }
    }

    /// Fallback: call the first available SubAgent when minimum call count not met
    async fn run_fallback_agents(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        registry: &AgentRegistry,
    ) -> Vec<(String, AgentOutput)> {
        let mut results = Vec::new();
        // Try to call any available agent
        let all_ids = registry.list().await;
        for agent_id in all_ids.iter().take(1) {
            if let Some(agent) = registry.get(agent_id).await {
                let input = build_input(ctx, msg, Arc::new(Vec::new()), "");
                let output = agent.run(input).await;
                results.push((agent_id.clone(), output));
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_teams_core::agent_memory_cache::{CacheMode, ExecutionPolicy};

    #[test]
    fn test_execution_policy_enforces_sub_agent() {
        let policy = ExecutionPolicy::default();
        assert!(
            policy.force_sub_agent,
            "force_sub_agent must default to true"
        );
        assert!(
            policy.min_sub_agent_calls >= 1,
            "min_sub_agent_calls must be at least 1"
        );
        assert!(
            !policy.allow_response_cache,
            "allow_response_cache must default to false"
        );
    }

    #[test]
    fn test_execution_policy_cache_mode_default() {
        let policy = ExecutionPolicy::default();
        assert_eq!(policy.cache_mode, CacheMode::ReadWrite);
        assert!(policy.allow_plan_cache);
    }

    #[test]
    fn test_sub_agent_cache_key_deterministic() {
        let key1 = sub_agent_cache_key("req1", "agent1", "hello");
        let key2 = sub_agent_cache_key("req1", "agent1", "hello");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_sub_agent_cache_key_different_agents() {
        let key1 = sub_agent_cache_key("req1", "agent1", "hello");
        let key2 = sub_agent_cache_key("req1", "agent2", "hello");
        assert_ne!(key1, key2);
    }
}
