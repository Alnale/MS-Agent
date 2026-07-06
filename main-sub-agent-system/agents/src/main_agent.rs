use std::sync::{Arc, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use dashmap::DashMap;
use regex::Regex;
use tokio::sync::RwLock;

use agent_core::agent_memory_cache::{AgentMemoryCache, CacheMode, ExecutionPolicy};
use agent_core::boxed_agent::{
    AgentCapabilities, AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent,
};
use agent_core::context::AgentContext;
use agent_core::error::{AgentTeamsError, Result};
use agent_core::memory::{MemoryKind, MemoryQuery};
use agent_core::memory_store::MemoryStore;
use agent_core::message::AgentMessage;
use agent_core::pipeline::StageMode;
use agent_core::plan::{ExecutionPlan, PlanNode, PlanStage};
use agent_core::provider::{
    ChatMessage, CompletionChunk, CompletionRequest, LlmProvider, ProviderError, ThinkingConfig,
};
use agent_core::routing::RoutingTable;
use agent_core::sub_agent::SubAgentDescriptor;

use crate::decision_store::DecisionStore;
use crate::prompt;

/// Regex for [[tool:name]], [[tool:name|{json}]], [[tool:name:subcommand]], or [[tool:name:subcommand|{json}]] syntax
fn regex_tool_call() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[tool:(\w+)(?::(\w+))?(?:\|([^\]]*))?\]\]").expect("valid regex"))
}

/// Maximum characters to keep when truncating content for memory storage
pub const MEMORY_CONTENT_MAX_LEN: usize = 500;
/// Default timeout for SubAgent execution in milliseconds
pub const DEFAULT_AGENT_TIMEOUT_MS: u64 = 120_000;
/// Baseline agents that are always present in every plan
pub const BASELINE_AGENTS: &[&str] = &["sentiment", "task_planner"];

/// Decision record for self-optimization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecord {
    pub input_hash: String,
    pub plan: ExecutionPlan,
    pub outcome_quality: f32,
    pub actual_duration_ms: u64,
    pub timestamp: u64,
    pub agents_called: Vec<String>,
    pub agents_errored: Vec<String>,
}

/// Tool intent detection result
#[derive(Debug, Clone)]
pub struct ToolIntent {
    pub is_likely: bool,
    pub suggested_tool: String,
    pub confidence: f32,
}

/// MainAgent configuration
#[derive(Debug, Clone)]
pub struct MainAgentConfig {
    pub thinking_enabled: bool,
    pub thinking_budget_tokens: u32,
    pub critic_enabled: bool,
    pub max_refinement_rounds: u8,
    pub total_timeout_ms: u64,
    pub plan_cache_ttl_secs: u64,
    pub plan_cache_capacity: usize,
    pub default_model: String,
    /// Whether to use unified cache management (default: true)
    pub unified_cache_enabled: bool,
    /// Minimum number of SubAgent calls required per request (default: 1)
    pub min_sub_agent_calls: usize,
    /// Capacity of the memory event broadcast channel (default: 1000)
    pub memory_event_bus_capacity: usize,
    /// Capacity of the shared cross-agent memory cache (default: 10000)
    pub shared_cache_capacity: usize,
}

impl Default for MainAgentConfig {
    fn default() -> Self {
        Self {
            thinking_enabled: true,
            thinking_budget_tokens: 8192,
            critic_enabled: true,
            max_refinement_rounds: 1,
            total_timeout_ms: 90_000,
            plan_cache_ttl_secs: 300,
            plan_cache_capacity: 500,
            default_model: String::new(),
            unified_cache_enabled: true,
            min_sub_agent_calls: 1,
            memory_event_bus_capacity: 1000,
            shared_cache_capacity: 10000,
        }
    }
}

/// Main Agent: responsible for task understanding, decomposition, scheduling, synthesis
pub struct MainAgent {
    provider: Arc<dyn LlmProvider>,
    /// Arc-wrapped for cheap clone on read (descriptors rarely change after startup)
    sub_agents: RwLock<Arc<Vec<SubAgentDescriptor>>>,
    routing_table: RwLock<Arc<Option<RoutingTable>>>,
    decision_cache: DashMap<String, (ExecutionPlan, Instant)>,
    decision_store: RwLock<DecisionStore>,
    /// Optional memory store for memory-aware planning
    memory_store: Option<Arc<dyn MemoryStore>>,
    config: MainAgentConfig,
    /// Agent-local memory cache (always available)
    agent_memory_cache: AgentMemoryCache,
    /// Tool registry for native tool calling support
    tool_registry: Option<Arc<agent_core::tool::UnifiedToolRegistry>>,
}

impl MainAgent {
    pub async fn new(
        provider: Arc<dyn LlmProvider>,
        config: MainAgentConfig,
        routing_table: Option<RoutingTable>,
    ) -> Self {
        let decision_store = DecisionStore::new("data/decisions.json", 1000).await;
        Self {
            provider,
            sub_agents: RwLock::new(Arc::new(Vec::new())),
            routing_table: RwLock::new(Arc::new(routing_table)),
            decision_cache: DashMap::new(),
            decision_store: RwLock::new(decision_store),
            memory_store: None,
            config,
            agent_memory_cache: AgentMemoryCache::new("main_agent".to_string(), 200),
            tool_registry: None,
        }
    }

    /// Set the tool registry for native tool calling support
    pub fn with_tool_registry(mut self, registry: Arc<agent_core::tool::UnifiedToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Whether thinking is enabled for this agent
    pub fn thinking_enabled(&self) -> bool {
        self.config.thinking_enabled
    }

    /// Thinking budget tokens
    pub fn thinking_budget_tokens(&self) -> u32 {
        self.config.thinking_budget_tokens
    }

    /// Create a new MainAgent with custom decision store path
    pub async fn with_decision_store(
        provider: Arc<dyn LlmProvider>,
        config: MainAgentConfig,
        routing_table: Option<RoutingTable>,
        store_path: &str,
    ) -> Self {
        let decision_store = DecisionStore::new(store_path, 1000).await;
        Self {
            provider,
            sub_agents: RwLock::new(Arc::new(Vec::new())),
            routing_table: RwLock::new(Arc::new(routing_table)),
            decision_cache: DashMap::new(),
            decision_store: RwLock::new(decision_store),
            memory_store: None,
            config,
            agent_memory_cache: AgentMemoryCache::new("main_agent".to_string(), 200),
            tool_registry: None,
        }
    }

    /// Set memory store for memory-aware planning
    pub fn with_memory_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    /// Set the agent-local memory cache with custom configuration
    pub fn with_agent_memory_cache(mut self, cache: AgentMemoryCache) -> Self {
        self.agent_memory_cache = cache;
        self
    }

    /// Register a SubAgent descriptor
    pub async fn register_descriptor(&self, descriptor: SubAgentDescriptor) {
        let mut guard = self.sub_agents.write().await;
        let mut new_vec = (**guard).clone();
        new_vec.push(descriptor);
        *guard = Arc::new(new_vec);
    }

    /// Remove a SubAgent descriptor
    pub async fn remove_descriptor(&self, agent_id: &str) {
        let mut guard = self.sub_agents.write().await;
        let new_vec: Vec<_> = guard.iter().filter(|d| d.id != agent_id).cloned().collect();
        *guard = Arc::new(new_vec);
    }

    /// Merge a routing table
    pub async fn merge_routing_table(&self, table: RoutingTable) {
        let mut guard = self.routing_table.write().await;
        let mut new_rt = (**guard).clone();
        match new_rt.as_mut() {
            Some(existing) => existing.merge(table),
            None => new_rt = Some(table),
        }
        *guard = Arc::new(new_rt);
    }

    /// Invalidate the decision cache
    pub async fn invalidate_cache(&self) {
        self.decision_cache.clear();
    }

    /// Remove stale entries from the decision cache to prevent unbounded growth
    fn evict_stale_cache(&self) {
        if self.decision_cache.len() > self.config.plan_cache_capacity {
            let ttl = self.config.plan_cache_ttl_secs;
            self.decision_cache
                .retain(|_, (_, created)| created.elapsed().as_secs() < ttl);
        }
    }

    /// Insert into decision cache with bounded eviction
    fn cache_insert(&self, key: String, plan: ExecutionPlan) {
        self.evict_stale_cache();
        self.decision_cache.insert(key, (plan, Instant::now()));
    }

    /// Get all registered SubAgent descriptors
    pub async fn get_sub_agents(&self) -> Arc<Vec<SubAgentDescriptor>> {
        self.sub_agents.read().await.clone()
    }

    /// Record outcome for self-optimization
    pub async fn record_outcome(&self, input_hash: &str, quality: f32, duration_ms: u64) {
        tracing::info!(
            "Decision outcome recorded: hash={}, quality={}, duration={}ms",
            input_hash,
            quality,
            duration_ms
        );

        // Look up the cached plan for this input
        let plan = self
            .decision_cache
            .get(input_hash)
            .map(|r| r.value().0.clone());

        if let Some(plan) = plan {
            let agents_called: Vec<String> = plan
                .stages
                .iter()
                .flat_map(|s| s.sub_agent_ids.clone())
                .collect();

            let record = DecisionRecord {
                input_hash: input_hash.to_string(),
                plan,
                outcome_quality: quality,
                actual_duration_ms: duration_ms,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                agents_called,
                agents_errored: Vec::new(),
            };

            let store = self.decision_store.write().await;
            store.add_record(record).await;
        }
    }

    /// Get decision history
    pub async fn get_decision_history(&self) -> Vec<DecisionRecord> {
        let store = self.decision_store.read().await;
        store.get_records().await
    }

    /// Generate input hash for caching using FNV-1a algorithm
    fn input_hash(msg_type: &str, content: &str) -> String {
        agent_core::hash::fnv1a_hash_str(&[msg_type, content])
    }

    fn thinking_config(&self) -> Option<ThinkingConfig> {
        if self.config.thinking_enabled {
            Some(ThinkingConfig {
                enabled: true,
                budget_tokens: self.config.thinking_budget_tokens,
                strategy: "Auto".to_string(),
            })
        } else {
            None
        }
    }

    /// Parse natural language message into structured tool arguments.
    /// Detect explicit [[tool:name]] syntax only.
    /// All natural-language tool routing is fully delegated to the LLM.
    pub fn detect_tool_intent(&self, msg: &AgentMessage) -> ToolIntent {
        // Only honor explicit [[tool:name]] or [[tool:name|{json}]] syntax
        if let Some(caps) = regex_tool_call().captures(&msg.content) {
            let tool_name = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            tracing::info!("detect_tool_intent: explicit tool syntax detected: '{}'", tool_name);
            return ToolIntent {
                is_likely: true,
                suggested_tool: tool_name,
                confidence: 0.99,
            };
        }

        // No heuristic — let the LLM decide everything
        ToolIntent {
            is_likely: false,
            suggested_tool: "auto".to_string(),
            confidence: 0.0,
        }
    }

    /// Infer tool subcommand from user message content
    fn infer_subcommand(content: &str, tool_name: &str) -> Option<String> {
        let lower = content.to_lowercase();
        match tool_name {
            "xxt" => {
                if lower.contains("登录") || lower.contains("login") {
                    Some("login".to_string())
                } else if lower.contains("爬取") || lower.contains("抓取") || lower.contains("crawl") {
                    Some("crawl".to_string())
                } else if lower.contains("填充") || lower.contains("填写") || lower.contains("fill") {
                    Some("fill".to_string())
                } else if lower.contains("提交") || lower.contains("submit") {
                    Some("submit".to_string())
                } else if lower.contains("截图") || lower.contains("screenshot") {
                    Some("screenshot".to_string())
                } else if lower.contains("检查") || lower.contains("check") {
                    Some("check".to_string())
                } else {
                    Some("crawl".to_string()) // default for xxt
                }
            }
            "file" => {
                if lower.contains("列") || lower.contains("目录") || lower.contains("查看目录")
                    || lower.contains("查看文件夹") || lower.contains("文件夹") || lower.contains("文件列表")
                    || lower.contains("list") || lower.contains("ls") || lower.contains("dir")
                    || lower.contains("directory") || lower.contains("folder") {
                    Some("list".to_string())
                } else if lower.contains("写") || lower.contains("保存") || lower.contains("创建") || lower.contains("新建")
                    || lower.contains("write") || lower.contains("save") || lower.contains("create") {
                    Some("write".to_string())
                } else if lower.contains("删") || lower.contains("删除") || lower.contains("delete") || lower.contains("remove") {
                    Some("delete".to_string())
                } else if lower.contains("信息") || lower.contains("大小") || lower.contains("属性")
                    || lower.contains("info") || lower.contains("stat") || lower.contains("size") {
                    Some("info".to_string())
                } else if lower.contains("存在") || lower.contains("有没有") || lower.contains("exists") || lower.contains("exist") {
                    Some("exists".to_string())
                } else if lower.contains("搜索文件") || lower.contains("grep") || lower.contains("查找文件")
                    || lower.contains("search") {
                    Some("search".to_string())
                } else {
                    Some("read".to_string()) // default for file
                }
            }
            "media" => {
                if lower.contains("自定义视频") || lower.contains("切换到视频") || lower.contains("激活视频") || lower.contains("视频背景") {
                    Some("activate_bg_video".to_string())
                } else if lower.contains("自定义图片") || lower.contains("切换到图片") || lower.contains("激活图片") || lower.contains("图片背景") {
                    Some("activate_bg_image".to_string())
                } else if lower.contains("导入") && lower.contains("视频") {
                    Some("import_and_set_bg_video".to_string())
                } else if lower.contains("导入") && lower.contains("图片") {
                    Some("import_and_set_bg_image".to_string())
                } else if lower.contains("导入") && lower.contains("音乐") {
                    Some("import_and_play_music".to_string())
                } else if lower.contains("设置") && lower.contains("视频") {
                    Some("set_bg_video".to_string())
                } else if lower.contains("设置") && lower.contains("图片") {
                    Some("set_bg_image".to_string())
                } else if lower.contains("播放") && !lower.contains("暂停") {
                    Some("play_music".to_string())
                } else if lower.contains("暂停") {
                    Some("pause_music".to_string())
                } else if lower.contains("恢复") {
                    Some("resume_music".to_string())
                } else if lower.contains("下一首") || lower.contains("切歌") || lower.contains("换歌") {
                    Some("next_track".to_string())
                } else if lower.contains("上一首") {
                    Some("prev_track".to_string())
                } else if lower.contains("静音") {
                    Some("toggle_mute".to_string())
                } else if lower.contains("音量") {
                    Some("set_volume".to_string())
                } else if lower.contains("清除背景") || lower.contains("恢复默认") || lower.contains("默认背景") || lower.contains("切换回默认") {
                    Some("clear_bg".to_string())
                } else if lower.contains("状态") {
                    Some("get_status".to_string())
                } else if lower.contains("背景") {
                    Some("activate_bg_video".to_string())
                } else {
                    Some("get_status".to_string())
                }
            }
            _ => None,
        }
    }

    /// Enhance plan with memory-based insights
    async fn enhance_plan_with_memory(&self, plan: &ExecutionPlan, query: &str) -> ExecutionPlan {
        let store = match self.memory_store.as_ref() {
            Some(s) => s,
            None => return plan.clone(),
        };

        // Query historical similar task summaries
        let historical = store
            .retrieve(MemoryQuery {
                text: query.to_string(),
                kinds: vec![MemoryKind::Summary],
                limit: 5,
                min_weight: 0.0,
                ..Default::default()
            })
            .await
            .ok();

        if let Some(hist) = historical {
            // Check if historical decisions had low quality
            for entry in &hist.entries {
                if entry.weight < 0.3 {
                    // Historical path had poor results, degrade confidence
                    tracing::debug!(
                        "Low-quality historical memory found (weight={:.2}), degrading plan confidence",
                        entry.weight
                    );
                    let mut adjusted = plan.clone();
                    adjusted.confidence = (adjusted.confidence * 0.8).max(0.3);
                    adjusted.strategy = format!(
                        "{} (memory-adjusted: historical low quality)",
                        adjusted.strategy
                    );
                    return adjusted;
                }
            }
        }

        plan.clone()
    }

    /// Four-level routing strategy (crate-private — use plan_task_with_policy externally)
    #[tracing::instrument(skip(self, ctx, msg), fields(session_id = %ctx.session_id))]
    pub(crate) async fn plan_task(&self, ctx: &AgentContext, msg: &AgentMessage) -> ExecutionPlan {
        self.evict_stale_cache();
        let hash = Self::input_hash(&msg.message_type, &msg.content);

        // Level 0: Check cache
        if let Some(entry) = self.decision_cache.get(&hash) {
            let (plan, created) = entry.value();
            if created.elapsed().as_secs() < self.config.plan_cache_ttl_secs {
                tracing::debug!("Plan cache hit for hash: {}", hash);
                return plan.clone();
            }
            drop(entry);
            self.decision_cache.remove(&hash);
        }

        // Tool intent detection: when confident, carry the detected tool info
        // as metadata so task_planner can use the fast path instead of relying
        // on its own LLM to recognize the tool need.
        let tool_intent = self.detect_tool_intent(msg);
        let plan_tool_intent: Option<agent_core::plan::ToolIntent> = if tool_intent.is_likely && tool_intent.confidence >= 0.65 {
            let subcommand = Self::infer_subcommand(&msg.content, &tool_intent.suggested_tool);
            tracing::info!(
                "Tool intent detected (suggested: {}, confidence: {:.2}, subcommand: {:?}) — injecting into plan",
                tool_intent.suggested_tool, tool_intent.confidence, subcommand
            );
            Some(agent_core::plan::ToolIntent {
                tool_name: tool_intent.suggested_tool.clone(),
                confidence: tool_intent.confidence,
                subcommand,
            })
        } else {
            if tool_intent.is_likely {
                tracing::info!(
                    "Tool intent detected but low confidence ({:.2}) — deferring to task_planner LLM",
                    tool_intent.confidence
                );
            }
            None
        };

        // Clone Arc pointers (cheap, no deep copy)
        let sub_agents = self.sub_agents.read().await.clone();
        let routing_table = self.routing_table.read().await.clone();
        if let Some(table) = routing_table.as_ref() {
            if let Some(target) = table.evaluate(ctx, msg) {
                let plan = ExecutionPlan {
                    stages: vec![PlanStage {
                        name: "direct_route".to_string(),
                        sub_agent_ids: vec![target.agent_id.clone()],
                        mode: StageMode::Sequential,
                        required: true,
                        timeout_ms: Some(DEFAULT_AGENT_TIMEOUT_MS),
                        message_override: None,
                    }],
                    strategy: format!("RoutingTable matched: {}", target.agent_id),
                    estimated_duration_ms: 5_000,
                    confidence: 0.95,
                    nodes: Vec::new(),
                    tool_intent: plan_tool_intent.clone(),
                };
                self.cache_insert(hash, plan.clone());
                return plan;
            }
        }

        // Level 2: Single SubAgent direct routing
        if sub_agents.len() == 1 {
            let plan = ExecutionPlan {
                stages: vec![PlanStage {
                    name: "single_agent".to_string(),
                    sub_agent_ids: vec![sub_agents[0].id.clone()],
                    mode: StageMode::Sequential,
                    required: true,
                    timeout_ms: Some(DEFAULT_AGENT_TIMEOUT_MS),
                    message_override: None,
                }],
                strategy: "Single SubAgent direct".to_string(),
                estimated_duration_ms: 5_000,
                confidence: 0.9,
                nodes: Vec::new(),
                tool_intent: plan_tool_intent.clone(),
            };
            self.cache_insert(hash, plan.clone());
            return plan;
        }

        // Level 3: Single type-matched agent (fast path, no LLM call)
        let matching: Vec<&SubAgentDescriptor> = sub_agents
            .iter()
            .filter(|sa| sa.capabilities.matches_message_type(&msg.message_type))
            .collect();

        if matching.len() == 1 {
            let plan = ExecutionPlan {
                stages: vec![PlanStage {
                    name: "type_matched".to_string(),
                    sub_agent_ids: vec![matching[0].id.clone()],
                    mode: StageMode::Sequential,
                    required: true,
                    timeout_ms: Some(DEFAULT_AGENT_TIMEOUT_MS),
                    message_override: None,
                }],
                strategy: format!("Message type matched: {}", msg.message_type),
                estimated_duration_ms: 10_000,
                confidence: 0.8,
                nodes: Vec::new(),
                tool_intent: plan_tool_intent.clone(),
            };
            self.cache_insert(hash, plan.clone());
            return plan;
        }

        // Level 4: LLM classify — always used when multiple sub-agents exist.
        // This ensures ALL agents (tool_agent, task_planner, knowledge, sentiment, summary, etc.)
        // are considered, not just the ones whose message_types match.
        let msg_clone = msg.clone();
        let sub_agents_clone = sub_agents.clone();
        let provider = self.provider.clone();
        let thinking_cfg = self.thinking_config();
        let default_model = self.config.default_model.clone();

        // Get available tools for native tool calling, filtered by allowed_tools if set
        let available_tools = self.tool_registry.as_ref().map(|reg| {
            let all = reg.list_tools();
            if ctx.allowed_tools.is_empty() {
                all
            } else {
                all.into_iter().filter(|t| ctx.allowed_tools.contains(&t.name)).collect()
            }
        });

        let base_plan = match Self::llm_classify_static(
            &provider,
            &msg_clone,
            &sub_agents_clone,
            thinking_cfg,
            &default_model,
            available_tools,
        )
        .await
        {
            Ok(mut plan) => {
                plan.tool_intent = plan_tool_intent.clone();
                plan
            }
            Err(e) => {
                tracing::warn!("LLM classification failed: {}, using default plan", e);
                let mut plan = Self::fallback_plan(&sub_agents);
                plan.tool_intent = plan_tool_intent.clone();
                plan
            }
        };

        // Enhance plan with memory-based insights
        let plan = self
            .enhance_plan_with_memory(&base_plan, &msg.content)
            .await;
        self.cache_insert(hash, plan.clone());
        plan
    }

    /// Plan task with execution policy (enforces SubAgent calls)
    #[tracing::instrument(skip(self, ctx, msg, policy), fields(session_id = %ctx.session_id))]
    pub async fn plan_task_with_policy(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        policy: &ExecutionPolicy,
    ) -> ExecutionPlan {
        // Note: evict_stale_cache() is called inside plan_task()
        let hash = Self::input_hash(&msg.message_type, &msg.content);

        // Level 0: Check plan cache (if allowed)
        if policy.allow_plan_cache {
            if let Some(entry) = self.decision_cache.get(&hash) {
                let (plan, created) = entry.value();
                if created.elapsed().as_secs() < self.config.plan_cache_ttl_secs {
                    tracing::debug!("Plan cache hit for hash: {}", hash);

                    // Force check: if cached plan has no SubAgent calls, regenerate
                    if policy.force_sub_agent && !Self::has_sub_agent_calls(plan) {
                        tracing::warn!("Cached plan has no sub-agent calls, regenerating");
                    } else {
                        return plan.clone();
                    }
                }
            }
        }

        // Generate new plan
        let base_plan = self.plan_task(ctx, msg).await;

        // Ensure mandatory agents are always present:
        // - task_planner: always called (baseline routing/planning)
        // - sentiment: always called (baseline emotion analysis)
        // - tool_agent: called when plan contains tool nodes
        let base_plan = self.ensure_mandatory_agents(base_plan).await;

        // Enforce SubAgent calls if policy requires it
        let plan = if policy.force_sub_agent {
            self.enforce_sub_agent_calls(base_plan, policy.min_sub_agent_calls, msg)
                .await
        } else {
            base_plan
        };

        // Enhance plan with memory-based insights
        let plan = self.enhance_plan_with_memory(&plan, &msg.content).await;

        if policy.allow_plan_cache {
            self.cache_insert(hash, plan.clone());
        }
        plan
    }

    /// Strict mode: force at least `min_calls` Sub Agent invocations
    pub async fn plan_task_strict(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        min_calls: usize,
    ) -> ExecutionPlan {
        let policy = ExecutionPolicy {
            force_sub_agent: true,
            min_sub_agent_calls: min_calls,
            allow_plan_cache: true,
            allow_response_cache: false,
            cache_mode: CacheMode::ReadWrite,
        };
        self.plan_task_with_policy(ctx, msg, &policy).await
    }

    /// Check if a plan contains any SubAgent calls
    fn has_sub_agent_calls(plan: &ExecutionPlan) -> bool {
        plan.stages.iter().any(|s| !s.sub_agent_ids.is_empty())
            || plan.nodes.iter().any(|n| matches!(n, PlanNode::Agent { .. }))
    }

    /// Ensure mandatory agents are always present in the plan.
    ///
    /// Rules:
    /// - Baseline agents (task_planner + sentiment) are ALWAYS called
    /// - task_planner handles both routing AND tool execution
    /// - Other agents (summary) are called ONLY when in the plan
    async fn ensure_mandatory_agents(&self, mut plan: ExecutionPlan) -> ExecutionPlan {
        // Collect all agent IDs already in the plan
        let existing_ids: std::collections::HashSet<String> = plan
            .stages
            .iter()
            .flat_map(|s| s.sub_agent_ids.iter().cloned())
            .chain(plan.nodes.iter().filter_map(|n| {
                if let PlanNode::Agent { agent_id, .. } = n {
                    Some(agent_id.clone())
                } else {
                    None
                }
            }))
            .collect();

        let mut mandatory_missing: Vec<String> = Vec::new();

        // Always require baseline agents
        for agent_id in BASELINE_AGENTS {
            if !existing_ids.contains(*agent_id) {
                mandatory_missing.push(agent_id.to_string());
            }
        }

        if mandatory_missing.is_empty() {
            return plan;
        }

        tracing::info!(
            "Injecting mandatory agents: {:?} (existing: {:?})",
            mandatory_missing,
            existing_ids
        );

        if plan.nodes.is_empty() {
            // Stage-based plan: add to stages only
            plan.stages.push(PlanStage {
                name: "mandatory_baseline".to_string(),
                sub_agent_ids: mandatory_missing.clone(),
                mode: StageMode::Parallel,
                required: false,
                timeout_ms: Some(30_000),
                message_override: None,
            });
        } else {
            // Node-based plan: add to nodes only (stages are not executed by execute_plan_with_nodes)
            for agent_id in &mandatory_missing {
                plan.nodes.push(PlanNode::Agent {
                    agent_id: agent_id.clone(),
                    input_transform: None,
                });
            }
        }

        plan.strategy = format!(
            "{} (mandatory: +{} baseline agents)",
            plan.strategy,
            mandatory_missing.len()
        );

        plan
    }

    /// Ensure plan has at least `min_calls` SubAgent invocations.
    /// Only considers baseline agents (task_planner + sentiment) as candidates.
    /// Other agents are dynamically invoked by task_planner's routing decision.
    async fn enforce_sub_agent_calls(
        &self,
        mut plan: ExecutionPlan,
        min_calls: usize,
        _msg: &AgentMessage,
    ) -> ExecutionPlan {
        // Count agents from both stages and nodes to avoid duplicates
        let stage_agent_ids: std::collections::HashSet<String> = plan
            .stages
            .iter()
            .flat_map(|s| s.sub_agent_ids.iter().cloned())
            .collect();
        let node_agent_ids = Self::collect_node_agent_ids(&plan.nodes);
        let mut existing_ids = stage_agent_ids.clone();
        existing_ids.extend(node_agent_ids);

        let current_calls = existing_ids.iter().filter(|id| *id != "main_agent").count();

        if current_calls >= min_calls {
            return plan;
        }

        // Only consider baseline agents as enforcement candidates.
        // Other agents (summary) are dynamically invoked by task_planner.
        let mut needed = min_calls - current_calls;
        let mut added = Vec::new();

        for agent_id in BASELINE_AGENTS {
            if needed == 0 {
                break;
            }
            if !existing_ids.contains(*agent_id) {
                added.push(agent_id.to_string());
                needed -= 1;
            }
        }

        if !added.is_empty() {
            let added_count = added.len();
            plan.stages.push(PlanStage {
                name: "enforced_sub_agents".to_string(),
                sub_agent_ids: added.clone(),
                mode: StageMode::Parallel,
                required: false,
                timeout_ms: Some(15_000),
                message_override: None,
            });
            // For node-based plans, also add Agent nodes
            if !plan.nodes.is_empty() {
                for agent_id in &added {
                    plan.nodes.push(PlanNode::Agent {
                        agent_id: agent_id.clone(),
                        input_transform: None,
                    });
                }
            }
            plan.strategy = format!(
                "{} (enforced: +{} baseline agents)",
                plan.strategy, added_count
            );
        }

        plan
    }

    /// Recursively collect agent IDs from PlanNode tree
    fn collect_node_agent_ids(nodes: &[PlanNode]) -> std::collections::HashSet<String> {
        let mut ids = std::collections::HashSet::new();
        for node in nodes {
            match node {
                PlanNode::Agent { agent_id, .. } => {
                    ids.insert(agent_id.clone());
                }
                PlanNode::Condition { then_branch, else_branch, .. } => {
                    ids.extend(Self::collect_node_agent_ids(then_branch));
                    ids.extend(Self::collect_node_agent_ids(else_branch));
                }
                PlanNode::Parallel(children) | PlanNode::Sequential(children) => {
                    ids.extend(Self::collect_node_agent_ids(children));
                }
                PlanNode::Tool { .. } => {}
            }
        }
        ids
    }

    fn fallback_plan(_sub_agents: &[SubAgentDescriptor]) -> ExecutionPlan {
        // Only use baseline agents.
        // Other agents are dynamically invoked by task_planner's routing decision.
        let fallback_ids: Vec<String> = BASELINE_AGENTS.iter().map(|s| s.to_string()).collect();
        ExecutionPlan {
            stages: vec![PlanStage {
                name: "fallback".to_string(),
                sub_agent_ids: fallback_ids,
                mode: StageMode::Parallel,
                required: true,
                timeout_ms: Some(DEFAULT_AGENT_TIMEOUT_MS),
                message_override: None,
            }],
            strategy: "LLM fallback (baseline only: task_planner + sentiment)".to_string(),
            estimated_duration_ms: 15_000,
            confidence: 0.5,
            nodes: Vec::new(),
            tool_intent: None,
        }
    }

    /// LLM-based task classification with native tool calling support.
    ///
    /// Two-phase approach:
    /// Phase 1: If tools are available, let the LLM decide whether to call a tool.
    /// Phase 2: If no tool call, classify the message to select SubAgents.
    async fn llm_classify_static(
        provider: &Arc<dyn LlmProvider>,
        msg: &AgentMessage,
        sub_agents: &[SubAgentDescriptor],
        thinking: Option<ThinkingConfig>,
        default_model: &str,
        _available_tools: Option<Vec<agent_core::tool::Tool>>,
    ) -> Result<ExecutionPlan> {
        // This function handles SubAgent classification via LLM.
        // Tool routing is deferred to task_planner's routing decision (Phase 2.5).

        // Phase 2: SubAgent classification (original logic)
        let prompt = prompt::build_classification_prompt(sub_agents, &msg.content);

        // When thinking is enabled, max_tokens must exceed budget_tokens to leave room for text output
        let thinking_budget = thinking.as_ref().filter(|t| t.enabled).map(|t| t.budget_tokens).unwrap_or(0);
        let max_tokens = if thinking_budget > 0 {
            // budget + room for classification output (JSON is ~200-500 tokens)
            Some((thinking_budget + 2048).max(16384))
        } else {
            Some(4096)
        };

        let request = CompletionRequest {
            model: default_model.to_string(),
            messages: vec![ChatMessage::simple("user", prompt)],
            max_tokens,
            temperature: Some(0.1),
            thinking,
            ..Default::default()
        };

        let response = provider
            .complete(request)
            .await
            .map_err(|e| AgentTeamsError::PlanGenerationFailed(e.to_string()))?;

        // Parse LLM response — extract JSON from possible markdown code blocks or surrounding text
        let content = response.content.trim();
        let json_str = Self::extract_json_from_response(content);
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| AgentTeamsError::PlanGenerationFailed(format!("Invalid JSON: {} (raw: {})", e, content.chars().take(200).collect::<String>())))?;

        let agent_ids: Vec<String> = parsed["sub_agents"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        // Validate agent IDs exist (main_agent is always valid)
        let valid_ids: Vec<String> = agent_ids
            .into_iter()
            .filter(|id| id == "main_agent" || sub_agents.iter().any(|sa| &sa.id == id))
            .collect();

        if valid_ids.is_empty() {
            return Err(AgentTeamsError::PlanGenerationFailed(
                "No valid agent IDs".to_string(),
            ));
        }

        // Filter out main_agent from results — only SubAgents should execute
        let filtered_ids: Vec<String> = valid_ids
            .into_iter()
            .filter(|id| id != "main_agent")
            .collect();

        // If no sub-agents remain (e.g. LLM classified only main_agent for casual chat),
        // return an empty plan — baseline agents (task_planner + sentiment) will be
        // injected by ensure_mandatory_agents().
        if filtered_ids.is_empty() {
            tracing::info!(
                "LLM classified no sub-agents (casual chat?), returning baseline-only plan. Reasoning: {}",
                parsed["reasoning"].as_str().unwrap_or("")
            );
            return Ok(ExecutionPlan {
                stages: vec![],
                strategy: format!(
                    "LLM classified: no additional agents needed ({})",
                    parsed["reasoning"].as_str().unwrap_or("")
                ),
                estimated_duration_ms: 5_000,
                confidence: 0.8,
                nodes: Vec::new(),
                tool_intent: None,
            });
        }

        let mode = match parsed["mode"].as_str().unwrap_or("Sequential") {
            "Parallel" => StageMode::Parallel,
            _ => StageMode::Sequential,
        };

        Ok(ExecutionPlan {
            stages: vec![PlanStage {
                name: "llm_classified".to_string(),
                sub_agent_ids: filtered_ids,
                mode,
                required: true,
                timeout_ms: Some(DEFAULT_AGENT_TIMEOUT_MS),
                message_override: None,
            }],
            strategy: format!(
                "LLM classified: {}",
                parsed["reasoning"].as_str().unwrap_or("")
            ),
            estimated_duration_ms: 15_000,
            confidence: 0.7,
            nodes: Vec::new(),
            tool_intent: None,
        })
    }

    /// Extract JSON from LLM response that may contain markdown code blocks or surrounding text.
    pub fn extract_json_from_response(content: &str) -> &str {
        let trimmed = content.trim();

        // 1. Try direct parse first (pure JSON response)
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return trimmed;
        }

        // 2. Extract from ```json ... ``` or ``` ... ``` code blocks
        if let Some(start) = trimmed.find("```json") {
            let json_start = start + 7; // len("```json")
            if let Some(end) = trimmed[json_start..].find("```") {
                return trimmed[json_start..json_start + end].trim();
            }
        }
        if let Some(start) = trimmed.find("```") {
            let json_start = start + 3;
            if let Some(end) = trimmed[json_start..].find("```") {
                let candidate = trimmed[json_start..json_start + end].trim();
                if candidate.starts_with('{') || candidate.starts_with('[') {
                    return candidate;
                }
            }
        }

        // 3. Find first { ... } or [ ... ] block in the text
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if end > start {
                    return &trimmed[start..=end];
                }
            }
        }

        // 4. Return as-is and let the caller handle the error
        trimmed
    }

    /// Build synthesis messages (shared logic for sync and stream paths)
    fn build_synthesis_messages(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        sub_results: &[(String, AgentOutput)],
    ) -> Vec<ChatMessage> {
        let sub_results_text: Vec<(String, String)> = sub_results
            .iter()
            .map(|(id, resp)| {
                let mut content = resp.content.clone();
                // Include metadata summary if available for richer synthesis context
                if let Some(ref meta) = resp.metadata {
                    if let Some(tool_name) = meta.get("tool_name").and_then(|v| v.as_str()) {
                        let exec_ms = meta.get("execution_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                        let skipped = meta.get("skipped_analysis").and_then(|v| v.as_bool()).unwrap_or(false);
                        if skipped {
                            content = format!("[工具{}执行, {}ms, 原始结果] {}", tool_name, exec_ms, content);
                        }
                    }
                }
                (id.clone(), content)
            })
            .collect();

        // Extract quality scores for the synthesis prompt
        let quality_scores: Vec<(String, f32)> = sub_results
            .iter()
            .map(|(id, resp)| (id.clone(), resp.quality))
            .collect();

        let synthesis_prompt = prompt::build_synthesis_prompt_with_quality_and_tools(
            &msg.content,
            &sub_results_text,
            &ctx.system_instructions,
            &quality_scores,
            self.tool_registry.as_deref(),
        );

        let mut messages: Vec<ChatMessage> = ctx
            .recent_history
            .iter()
            .filter_map(|entry| {
                let role = entry.get("sender_type")?.as_str()?;
                let content = entry.get("content")?.as_str()?;
                if content.trim().is_empty() {
                    return None;
                }
                let chat_role = match role {
                    "assistant" => "assistant",
                    _ => "user",
                };
                Some(ChatMessage {
                    role: chat_role.to_string(),
                    content: content.to_string(),
                    cache_control: None,
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            })
            .collect();
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: synthesis_prompt,
            cache_control: None,
            images: None,
            tool_call_id: None,
            tool_calls: None,
        });
        messages
    }

    /// Synthesize results from multiple SubAgents
    pub async fn synthesize_results(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        sub_results: &[(String, AgentOutput)],
    ) -> AgentOutput {
        if sub_results.is_empty() {
            return AgentOutput {
                content: "没有获取到分析结果。".to_string(),
                ..Default::default()
            };
        }

        let messages = self.build_synthesis_messages(ctx, msg, sub_results);

        let request = CompletionRequest {
            model: self.config.default_model.clone(),
            messages,
            max_tokens: Some(128000),
            temperature: Some(0.5),
            thinking: self.thinking_config(),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(response) => {
                // Warn if response was truncated by the model
                if response.stop_reason.as_deref() == Some("max_tokens") {
                    tracing::warn!(
                        "Synthesis response truncated (stop_reason=max_tokens). \
                         Consider increasing max_tokens or simplifying the prompt."
                    );
                }

                let all_effects: Vec<_> = sub_results
                    .iter()
                    .flat_map(|(_, r)| r.effects.clone())
                    .collect();

                AgentOutput {
                    content: response.content,
                    thinking: response.thinking,
                    effects: all_effects,
                    quality: 0.9,
                    ..Default::default()
                }
            }
            Err(e) => {
                tracing::warn!("Synthesis failed: {}, using highest quality result", e);
                sub_results
                    .iter()
                    .max_by(|a, b| a.1.quality.total_cmp(&b.1.quality))
                    .map(|(_, r)| r.clone())
                    .unwrap_or_default()
            }
        }
    }

    /// Synthesize results from multiple SubAgents with streaming support
    pub async fn synthesize_results_stream(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        sub_results: &[(String, AgentOutput)],
    ) -> Box<
        dyn futures::Stream<Item = std::result::Result<CompletionChunk, ProviderError>>
            + Unpin
            + Send,
    > {
        if sub_results.is_empty() {
            let chunk = CompletionChunk {
                delta: "没有获取到分析结果。".to_string(),
                done: true,
                ..Default::default()
            };
            return Box::new(Box::pin(futures::stream::once(async move { Ok(chunk) })));
        }

        let messages = self.build_synthesis_messages(ctx, msg, sub_results);

        let request = CompletionRequest {
            model: self.config.default_model.clone(),
            messages: messages.clone(),
            max_tokens: Some(128000),
            temperature: Some(0.5),
            stream: true,
            thinking: self.thinking_config(),
            ..Default::default()
        };

        match self.provider.complete_stream(request).await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::warn!("Stream synthesis failed: {}, falling back to non-stream", e);
                // Fallback to non-streaming
                let request = CompletionRequest {
                    model: self.config.default_model.clone(),
                    messages,
                    max_tokens: Some(128000),
                    temperature: Some(0.5),
                    thinking: self.thinking_config(),
                    ..Default::default()
                };

                match self.provider.complete(request).await {
                    Ok(response) => {
                        let chunk = CompletionChunk {
                            delta: response.content,
                            thinking_delta: response.thinking,
                            done: true,
                            usage: Some(response.usage),
                            ..Default::default()
                        };
                        Box::new(Box::pin(futures::stream::once(async move { Ok(chunk) })))
                    }
                    Err(e) => {
                        let chunk = CompletionChunk {
                            delta: format!("Error: {}", e),
                            done: true,
                            ..Default::default()
                        };
                        Box::new(Box::pin(futures::stream::once(async move { Ok(chunk) })))
                    }
                }
            }
        }
    }
}

#[async_trait]
impl BoxedAgent for MainAgent {
    fn id(&self) -> &str {
        "main_agent"
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            message_types: vec!["*".to_string()],
            requires_llm: true,
            supports_streaming: false,
            priority: 1000,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_memory_aware(&self) -> Option<&dyn agent_core::boxed_agent::MemoryAwareAgent> {
        Some(self)
    }

    async fn run(&self, input: AgentInput) -> AgentOutput {
        // Build messages with conversation history for context continuity
        let mut messages: Vec<ChatMessage> = input
            .recent_history
            .iter()
            .filter_map(|entry| {
                let role = entry.get("sender_type")?.as_str()?;
                let content = entry.get("content")?.as_str()?;
                if content.trim().is_empty() {
                    return None;
                }
                let chat_role = match role {
                    "assistant" => "assistant",
                    _ => "user",
                };
                Some(ChatMessage {
                    role: chat_role.to_string(),
                    content: content.to_string(),
                    cache_control: None,
                    images: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            })
            .collect();
        // Append current user message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: input.content,
            cache_control: None,
            images: None,
            tool_call_id: None,
            tool_calls: None,
        });

        let request = CompletionRequest {
            model: self.config.default_model.clone(),
            messages,
            max_tokens: Some(128000),
            temperature: Some(0.7),
            system: Some(input.system_prompt),
            thinking: self.thinking_config(),
            ..Default::default()
        };

        match self.provider.complete(request).await {
            Ok(resp) => AgentOutput {
                content: resp.content,
                thinking: resp.thinking,
                quality: 0.8,
                ..Default::default()
            },
            Err(e) => AgentOutput::error(format!("Error: {}", e)),
        }
    }
}

#[async_trait]
impl MemoryAwareAgent for MainAgent {
    fn memory_cache(&self) -> &AgentMemoryCache {
        &self.agent_memory_cache
    }

    async fn sync_to_memory(
        &self,
        store: &Arc<dyn MemoryStore>,
        session_id: &str,
        output: &AgentOutput,
    ) -> Result<()> {
        // Extract key facts from the output for memory storage
        if output.content.len() > 50 {
            let fact_entry = agent_core::memory::MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: Some(session_id.to_string()),
                kind: agent_core::memory::MemoryKind::AgentOutput,
                content: output
                    .content
                    .chars()
                    .take(MEMORY_CONTENT_MAX_LEN)
                    .collect(),
                data: None,
                embedding: None,
                weight: 0.6,
                created_at: chrono::Utc::now(),
                last_accessed_at: chrono::Utc::now(),
                access_count: 0,
                tags: vec!["main_agent_output".to_string()],
                source_agent: "main_agent".to_string(),
                confirmed: false,
                content_hash: Some(agent_core::memory::compute_content_hash(
                    &output.content,
                )),
                confidence: 0.8,
                parent_id: None,
                version: 1,
                archived: false,
                compressed_from: vec![],
            };
            store.store(fact_entry).await?;
        }

        // Flush local cache to global store
        self.agent_memory_cache.flush_all().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::context::AgentContext;
    use agent_core::message::AgentMessage;
    use agent_core::sub_agent::SubAgentDescriptor;

    fn make_test_provider() -> Arc<dyn LlmProvider> {
        struct MockProvider;
        #[async_trait]
        impl LlmProvider for MockProvider {
            fn id(&self) -> &str {
                "mock"
            }
            fn name(&self) -> &str {
                "Mock Provider"
            }
            fn models(&self) -> Vec<String> {
                vec!["mock-model".to_string()]
            }
            async fn complete(
                &self,
                _req: CompletionRequest,
            ) -> std::result::Result<
                agent_core::provider::CompletionResponse,
                agent_core::provider::ProviderError,
            > {
                Ok(agent_core::provider::CompletionResponse {
                    content: r#"{"sub_agents": ["knowledge"], "reasoning": "test", "mode": "Sequential"}"#.to_string(),
                    thinking: None,
                    model: "mock-model".to_string(),
                    usage: agent_core::provider::TokenUsage::default(),
                    stop_reason: Some("stop".to_string()),
                    tool_calls: vec![],
                    annotations: vec![],
                })
            }
            async fn complete_stream(
                &self,
                _req: CompletionRequest,
            ) -> std::result::Result<
                Box<
                    dyn futures::Stream<
                            Item = std::result::Result<
                                agent_core::provider::CompletionChunk,
                                agent_core::provider::ProviderError,
                            >,
                        > + Unpin
                        + Send,
                >,
                agent_core::provider::ProviderError,
            > {
                Err(agent_core::provider::ProviderError::Other(
                    "not supported".to_string(),
                ))
            }
        }
        Arc::new(MockProvider)
    }

    fn make_test_descriptor(id: &str) -> SubAgentDescriptor {
        SubAgentDescriptor {
            id: id.to_string(),
            capabilities: AgentCapabilities {
                message_types: vec!["user_input".to_string()],
                requires_llm: true,
                supports_streaming: false,
                priority: 50,
            },
            expertise: format!("test agent {}", id),
            available_tools: Vec::new(),
            depends_on: Vec::new(),
            priority: 50,
            fallback_agent_id: None,
            optional: false,
            default_effects: Vec::new(),
            version: None,
        }
    }

    #[tokio::test]
    async fn test_never_skip_sub_agent() {
        let provider = make_test_provider();
        let agent = MainAgent::new(provider, MainAgentConfig::default(), None).await;

        // Register test sub-agents
        agent
            .register_descriptor(make_test_descriptor("sentiment"))
            .await;
        agent
            .register_descriptor(make_test_descriptor("task_planner"))
            .await;

        let ctx = AgentContext {
            session_id: "test_session".to_string(),
            ..AgentContext::default()
        };

        // Test various input types
        let test_inputs = vec![
            AgentMessage::new("hello".to_string()),
            AgentMessage::new("复杂问题".to_string()),
            AgentMessage::new("knowledge_query".to_string()),
        ];

        for msg in test_inputs {
            let plan = agent.plan_task_strict(&ctx, &msg, 1).await;
            let sub_agent_count = plan
                .stages
                .iter()
                .flat_map(|s| &s.sub_agent_ids)
                .filter(|id| *id != "main_agent")
                .count();
            assert!(
                sub_agent_count >= 1,
                "Plan should contain at least 1 sub-agent call for input '{}', got {}",
                msg.content,
                sub_agent_count
            );
        }
    }

    #[tokio::test]
    async fn test_plan_task_strict_min_calls() {
        let provider = make_test_provider();
        let agent = MainAgent::new(provider, MainAgentConfig::default(), None).await;

        agent
            .register_descriptor(make_test_descriptor("sentiment"))
            .await;
        agent
            .register_descriptor(make_test_descriptor("task_planner"))
            .await;

        let ctx = AgentContext {
            session_id: "test_session".to_string(),
            ..AgentContext::default()
        };
        let msg = AgentMessage::new("test query".to_string());

        // Request at least 1 sub-agent call (sentiment is the only baseline agent)
        let plan = agent.plan_task_strict(&ctx, &msg, 1).await;
        let sub_agent_count = plan
            .stages
            .iter()
            .flat_map(|s| &s.sub_agent_ids)
            .filter(|id| *id != "main_agent")
            .count();
        assert!(
            sub_agent_count >= 1,
            "Plan should contain at least 1 sub-agent call, got {}",
            sub_agent_count
        );
    }
}
