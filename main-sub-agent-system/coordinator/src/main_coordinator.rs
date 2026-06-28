use std::sync::Arc;
use std::time::Instant;

use agent_teams_core::agent_memory_cache::ExecutionPolicy;
use agent_teams_core::boxed_agent::{AgentInput, AgentOutput, BoxedAgent, MemoryAwareAgent};
use agent_teams_core::config::{CostOptimizationConfig, DegradationConfig};
use agent_teams_core::context::AgentContext;
use agent_teams_core::context_provider::ContextProvider;
use agent_teams_core::effect::AgentEffect;
use agent_teams_core::event::{EventBus, SystemEvent};
use agent_teams_core::hook::{HookContext, HookData, HookPoint, HookRegistry, HookResult};
use agent_teams_core::message::{AgentMessage, AgentStatus};
use agent_teams_core::plan::ExecutionPlan;
use agent_teams_core::provider::{AgentProgress, CompletionChunk, ProviderError};
use agent_teams_core::registry::AgentRegistry;
use futures::StreamExt;
use agent_teams_core::state::ApplyResult;
use agent_teams_core::unified_memory_bus::UnifiedMemoryBus;

use crate::aggregator::EffectAggregator;
use crate::cache::ResponseCache;
use crate::critic::CriticAgent;
use crate::memory_context_provider::MemoryContextProvider;
use crate::memory_manager::MemoryManager;
use crate::plan_cache::PlanCache;
use crate::plan_executor::PipelineExecutor;
use crate::sub_agent_cache::SubAgentCache;
use crate::summary_background::SummaryServiceHandle;

use agent_teams_agents::main_agent::MainAgent;

/// Minimum quality threshold for storing agent output as memory
const MIN_QUALITY_THRESHOLD: f32 = 0.5;
/// Default timeout for pipeline executor in milliseconds
const DEFAULT_PIPELINE_TIMEOUT_MS: u64 = 120_000;

/// Result of the coordinator's execution
#[derive(Debug, Clone, Default)]
pub struct MainCoordinatorResult {
    pub narrative: String,
    pub effects: Vec<AgentEffect>,
    pub quality: f32,
    pub apply_result: Option<ApplyResult>,
    pub plan: Option<ExecutionPlan>,
    pub thinking: Option<serde_json::Value>,
    pub agent_statuses: std::collections::HashMap<String, AgentStatus>,
    pub total_duration_ms: u64,
    pub cache_hit: bool,
}

/// Intermediate result from pipeline preparation, shared between sync and streaming paths.
struct PreparedPipeline {
    enriched_ctx: AgentContext,
    msg: AgentMessage,
    plan: ExecutionPlan,
    sub_results: Vec<(String, AgentOutput)>,
    aggregated_effects: Vec<AgentEffect>,
    cache_key: String,
    start: Instant,
    is_simple: bool,
}

/// Main coordinator: orchestrates MainAgent + SubAgent execution
pub struct MainAgentCoordinator {
    main_agent: Arc<MainAgent>,
    registry: Arc<AgentRegistry>,
    effect_aggregator: EffectAggregator,
    critic: Option<CriticAgent>,
    plan_executor: PipelineExecutor,
    cache: Arc<ResponseCache>,
    plan_cache: Arc<PlanCache>,
    hook_registry: Option<Arc<HookRegistry>>,
    event_bus: Option<EventBus>,
    context_providers: Vec<Arc<dyn ContextProvider>>,
    cost_config: Option<CostOptimizationConfig>,
    degradation_config: Option<DegradationConfig>,
    memory_manager: Option<Arc<MemoryManager>>,
    summary_service: Option<SummaryServiceHandle>,
    /// Execution policy for controlling SubAgent invocation
    execution_policy: ExecutionPolicy,
    /// Unified memory bus for cross-agent cache coordination
    unified_bus: Option<Arc<UnifiedMemoryBus>>,
}

impl MainAgentCoordinator {
    /// Whether the main agent has thinking enabled
    pub fn main_agent_thinking_enabled(&self) -> bool {
        self.main_agent.thinking_enabled()
    }

    /// Thinking budget tokens for the main agent
    pub fn main_agent_thinking_budget(&self) -> u32 {
        self.main_agent.thinking_budget_tokens()
    }

    /// Emit a progress event to the stream channel
    fn emit_progress(
        tx: Option<&tokio::sync::mpsc::UnboundedSender<Result<CompletionChunk, ProviderError>>>,
        progress: AgentProgress,
    ) {
        if let Some(tx) = tx {
            let _ = tx.send(Ok(CompletionChunk {
                delta: String::new(),
                thinking_delta: None,
                done: false,
                usage: None,
                tool_call_delta: None,
                tool_status: None,
                sub_agent_results: None,
                companion_state: None,
                agent_progress: Some(progress),
            }));
        }
    }

    pub fn new(main_agent: Arc<MainAgent>, registry: Arc<AgentRegistry>) -> Self {
        let sub_agent_cache = Arc::new(SubAgentCache::new(1000, 60));
        let plan_executor =
            PipelineExecutor::new(DEFAULT_PIPELINE_TIMEOUT_MS).with_cache(sub_agent_cache);

        Self {
            main_agent,
            registry,
            effect_aggregator: EffectAggregator::new(),
            critic: None,
            plan_executor,
            cache: Arc::new(ResponseCache::new(500)),
            plan_cache: Arc::new(PlanCache::new(500, 300)),
            hook_registry: None,
            event_bus: None,
            context_providers: Vec::new(),
            cost_config: None,
            degradation_config: None,
            memory_manager: None,
            summary_service: None,
            execution_policy: ExecutionPolicy::default(),
            unified_bus: None,
        }
    }

    /// Set the execution policy for controlling SubAgent invocation
    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
        self.execution_policy = policy;
        self
    }

    /// Get the current execution policy
    pub fn execution_policy(&self) -> &ExecutionPolicy {
        &self.execution_policy
    }

    pub fn with_cost_config(mut self, config: CostOptimizationConfig) -> Self {
        self.cost_config = Some(config);
        self
    }

    pub fn with_degradation_config(mut self, config: DegradationConfig) -> Self {
        self.degradation_config = Some(config);
        self
    }

    pub fn with_critic(mut self, critic: CriticAgent) -> Self {
        self.critic = Some(critic);
        self
    }

    pub fn with_hooks(mut self, hooks: Arc<HookRegistry>) -> Self {
        self.hook_registry = Some(hooks);
        self
    }

    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn with_context_providers(mut self, providers: Vec<Arc<dyn ContextProvider>>) -> Self {
        self.context_providers = providers;
        self
    }

    pub fn with_memory_manager(mut self, mm: Arc<MemoryManager>) -> Self {
        // Register MemoryContextProvider so agents get memory context automatically
        let memory_context = Arc::new(MemoryContextProvider::new(mm.clone()));
        self.context_providers.push(memory_context);
        self.memory_manager = Some(mm);
        self
    }

    pub fn with_summary_service(mut self, handle: SummaryServiceHandle) -> Self {
        self.summary_service = Some(handle);
        self
    }

    pub fn with_unified_bus(mut self, bus: Arc<UnifiedMemoryBus>) -> Self {
        // Register MainAgent's memory cache with the bus
        bus.register_agent(
            "main_agent",
            Arc::new(self.main_agent.memory_cache().clone()),
        );
        self.plan_executor.set_unified_bus(bus.clone());
        self.unified_bus = Some(bus);
        self
    }

    /// Set the tool registry for providing tool info to all agents
    pub fn with_tool_registry(mut self, registry: Arc<agent_teams_core::tool::UnifiedToolRegistry>) -> Self {
        self.plan_executor.set_tool_registry(registry);
        self
    }

    /// Register all SubAgent memory caches with the unified bus
    pub async fn register_sub_agent_caches(&self) {
        let Some(ref bus) = self.unified_bus else {
            return;
        };

        let all_ids = self.registry.list().await;
        for agent_id in all_ids {
            if let Some(agent) = self.registry.get(&agent_id).await {
                if let Some(memory_aware) = agent.as_memory_aware() {
                    let cache = memory_aware.memory_cache();
                    bus.register_agent(&agent_id, Arc::new(cache.clone()));
                    tracing::debug!("Registered memory cache for agent: {}", agent_id);
                }
            }
        }
    }

    /// Get reference to memory manager (for deferred initialization)
    pub fn memory_manager_ref(&self) -> Option<&Arc<MemoryManager>> {
        self.memory_manager.as_ref()
    }

    /// Populate context with prompt fragments from all registered context providers
    async fn populate_context(&self, ctx: &mut AgentContext) {
        for provider in &self.context_providers {
            if let Some(fragment) = provider.provide(ctx).await {
                let fragments = Arc::make_mut(&mut ctx.prompt_fragments);
                fragments.push(fragment);
            }
        }
    }

    /// Get reference to the main agent
    pub fn main_agent(&self) -> &MainAgent {
        &self.main_agent
    }

    /// Run hooks for a given point. Returns Some(Halt reason) if any hook halts the flow.
    async fn run_hooks(
        &self,
        point: HookPoint,
        ctx: &mut AgentContext,
        msg: &mut Option<AgentMessage>,
        response: &mut Option<String>,
    ) -> Option<String> {
        let hooks = match &self.hook_registry {
            Some(reg) => reg.get_hooks(&point),
            None => return None,
        };
        if hooks.is_empty() {
            return None;
        }
        let mut extra = serde_json::Value::Null;
        let session_id = ctx.session_id.clone();
        for hook in hooks {
            let hook_ctx = HookContext {
                point: point.clone(),
                agent_id: None,
                session_id: &session_id,
            };
            let mut data = HookData {
                message: msg,
                context: ctx,
                response_content: response,
                extra: &mut extra,
            };
            match hook.execute(hook_ctx, &mut data).await {
                HookResult::Continue | HookResult::Modified => {}
                HookResult::Halt(reason) => {
                    tracing::info!("Hook '{}' halted flow: {}", hook.name(), reason);
                    return Some(reason);
                }
            }
        }
        None
    }

    /// Generate a stable cache key using FNV-1a hash algorithm.
    /// Includes session_id to prevent cross-session cache pollution
    /// (different sessions have different conversation context).
    fn cache_key(ctx: &AgentContext, msg: &AgentMessage) -> String {
        agent_teams_core::hash::fnv1a_hash_str(&[&ctx.session_id, &msg.message_type, &msg.content])
    }

    /// Ensure a specific agent is present in the plan. Adds it if missing.
    fn ensure_agent_in_plan(mut plan: ExecutionPlan, agent_id: &str) -> ExecutionPlan {
        let already_present = plan.stages.iter().any(|s| s.sub_agent_ids.contains(&agent_id.to_string()))
            || plan.nodes.iter().any(|n| matches!(n, agent_teams_core::plan::PlanNode::Agent { agent_id: id, .. } if id == agent_id));

        if already_present {
            return plan;
        }

        tracing::info!("Adding '{}' to plan (was missing)", agent_id);

        if plan.nodes.is_empty() {
            plan.stages.push(agent_teams_core::plan::PlanStage {
                name: format!("ensure_{}", agent_id),
                sub_agent_ids: vec![agent_id.to_string()],
                mode: agent_teams_core::pipeline::StageMode::Parallel,
                required: false,
                timeout_ms: Some(30_000),
                message_override: None,
            });
        } else {
            plan.nodes.push(agent_teams_core::plan::PlanNode::Agent {
                agent_id: agent_id.to_string(),
                input_transform: None,
            });
        }

        plan
    }

    /// Check if a message is a simple query (for cost optimization).
    /// Uses weighted scoring: hard blockers for truly complex requests,
    /// soft penalties for question marks and other indicators.
    fn is_simple_query(msg: &AgentMessage) -> bool {
        let content = &msg.content;
        let lower = content.to_lowercase();

        // Hard blockers: URLs, code, multi-sentence analysis requests
        let hard_blockers = [
            "http://", "https://", "://", "{", "}",
            "为什么", "如何", "怎么", "解释一下", "分析一下", "比较一下",
            "explain", "analyze", "compare", "what if",
            "步骤", "流程", "方案", "建议", "策略",
            "帮我", "能不能帮我", "可以帮我", "怎么办",
            "tell me", "help me", "could you help",
        ];
        for pattern in &hard_blockers {
            if content.contains(pattern) {
                return false;
            }
        }

        // Weighted scoring for simplicity
        let mut score: f32 = 0.0;

        // Length scoring
        if content.len() < 20 {
            score += 0.5;
        } else if content.len() < 50 {
            score += 0.4;
        } else if content.len() < 100 {
            score += 0.2;
        }

        // Word count scoring (Chinese: count chars, English: count words)
        let word_count = content.split_whitespace().count();
        if word_count <= 2 {
            score += 0.3;
        } else if word_count <= 4 {
            score += 0.1;
        }

        // Social exchange patterns (strong signal)
        let simple_patterns = [
            "你好", "hi", "hello", "ok", "好的", "谢谢", "thanks",
            "嗯", "是的", "对", "行", "没问题", "知道了", "明白",
            "好的谢谢", "收到", "了解", "嗯嗯", "哈哈",
        ];
        for pattern in &simple_patterns {
            if lower == *pattern || (lower.starts_with(pattern) && content.len() < 20) {
                score += 0.4;
                break;
            }
        }

        // Question mark penalty (soft, not hard blocker)
        let question_marks = content.matches('?').count() + content.matches('？').count();
        if question_marks > 0 {
            // Simple questions like "你好？" or "ok?" get small penalty
            // Complex questions like "how does X work? explain Y?" get larger penalty
            score -= 0.15 * question_marks.min(3) as f32;
        }

        // Chinese question particles (吗/呢/吧) in short messages are simple questions
        let has_question_particle = content.ends_with('吗')
            || content.ends_with('呢')
            || content.ends_with('吧');
        if has_question_particle && content.len() < 15 {
            score += 0.2; // Offset the question mark penalty
        }

        score >= 0.6
    }

    /// Shared pipeline preparation: hooks, memory init, context, plan, execute.
    /// Returns None if hooks halt the request (caller should return a halt response).
    async fn prepare_pipeline(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
        progress_tx: Option<&tokio::sync::mpsc::UnboundedSender<Result<CompletionChunk, ProviderError>>>,
    ) -> Option<PreparedPipeline> {
        let start = Instant::now();

        let key = Self::cache_key(ctx, msg);

        let is_simple = if self.execution_policy.force_sub_agent {
            false
        } else {
            self.cost_config
                .as_ref()
                .map(|c| c.skip_thinking_for_simple && Self::is_simple_query(msg))
                .unwrap_or(false)
        };

        let mut enriched_ctx = ctx.clone();

        let mut hook_msg = Some(msg.clone());
        let mut hook_response: Option<String> = None;
        if let Some(reason) = self
            .run_hooks(
                HookPoint::PreRun,
                &mut enriched_ctx,
                &mut hook_msg,
                &mut hook_response,
            )
            .await
        {
            tracing::info!("PreRun hooks halted request: {}", reason);
            return None;
        }

        // Emit progress: initializing
        Self::emit_progress(progress_tx, AgentProgress::StageStarted {
            stage_name: "initializing".to_string(),
            detail: "正在初始化记忆和上下文...".to_string(),
        });

        // Initialize memory
        if let (Some(mm), Some(user_id)) = (&self.memory_manager, &enriched_ctx.user_id) {
            match mm
                .initialize_session(user_id, &enriched_ctx.session_id, &msg.content)
                .await
            {
                Ok(memories) => {
                    self.warm_agent_caches(&memories).await;
                    enriched_ctx.working_memory = Arc::new(memories);
                }
                Err(e) => {
                    tracing::warn!("Memory initialization failed: {}", e);
                }
            }
        }

        // Populate context
        self.populate_context(&mut enriched_ctx).await;

        // PrePlan hooks
        self.run_hooks(
            HookPoint::PrePlan,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;

        // Emit progress: planning
        Self::emit_progress(progress_tx, AgentProgress::StageStarted {
            stage_name: "planning".to_string(),
            detail: "正在规划任务...".to_string(),
        });

        // Phase 1: Plan
        let plan_key = format!("plan:{}", key);
        let plan = if self.execution_policy.allow_plan_cache {
            if let Some(cached_plan) = self.plan_cache.get(&plan_key).await {
                tracing::debug!("Plan cache hit for key: {}", plan_key);
                if self.execution_policy.force_sub_agent
                    && !cached_plan
                        .stages
                        .iter()
                        .any(|s| !s.sub_agent_ids.is_empty())
                {
                    tracing::warn!("Cached plan has no SubAgent calls, regenerating");
                    let new_plan = self
                        .main_agent
                        .plan_task_with_policy(&enriched_ctx, msg, &self.execution_policy)
                        .await;
                    self.plan_cache.put(plan_key, new_plan.clone(), 300).await;
                    new_plan
                } else {
                    cached_plan
                }
            } else {
                let new_plan = self
                    .main_agent
                    .plan_task_with_policy(&enriched_ctx, msg, &self.execution_policy)
                    .await;
                self.plan_cache.put(plan_key, new_plan.clone(), 300).await;
                new_plan
            }
        } else {
            self.main_agent
                .plan_task_with_policy(&enriched_ctx, msg, &self.execution_policy)
                .await
        };

        // PostPlan hooks
        self.run_hooks(
            HookPoint::PostPlan,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;

        // task_planner is always kept — it handles routing decisions and tool execution.
        // sentiment is always kept — it's the baseline emotion analyzer.
        let tool_intent = self.main_agent.detect_tool_intent(msg);
        if tool_intent.is_likely {
            tracing::info!(
                "Tool intent detected (tool='{}', confidence={:.2})",
                tool_intent.suggested_tool, tool_intent.confidence
            );
        }
        let plan = Self::ensure_agent_in_plan(plan, "task_planner");

        // Wrap context in Arc once — all downstream methods clone the Arc (cheap)
        // instead of deep-cloning the entire AgentContext per agent invocation.
        let ctx_arc = Arc::new(enriched_ctx.clone());

        // Emit progress: executing agents
        let agent_ids: Vec<String> = plan.stages.iter()
            .flat_map(|s| s.sub_agent_ids.iter().cloned())
            .collect();
        Self::emit_progress(progress_tx, AgentProgress::StageStarted {
            stage_name: "executing".to_string(),
            detail: if agent_ids.is_empty() {
                "正在执行任务...".to_string()
            } else {
                format!("正在执行: {}", agent_ids.join(", "))
            },
        });

        // Phase 2: Execute plan
        // All tool execution is delegated to task_planner SubAgent via the orchestrator.
        let sub_results = if !plan.nodes.is_empty() {
            // PlanNode-based execution (tool calls delegated to task_planner SubAgent)
            self.plan_executor
                .execute_plan_with_nodes(
                    &ctx_arc,
                    &plan,
                    &self.registry,
                    &self.memory_manager,
                    &msg.content,
                )
                .await
        } else {
            // Legacy stage-based execution
            self.plan_executor
                .execute_with_policy(
                    &ctx_arc,
                    msg,
                    &plan,
                    &self.registry,
                    &self.execution_policy,
                )
                .await
        };

        // Phase 2.5: Dynamically invoke additional agents based on task_planner's routing decision.
        // task_planner is the sole authority for deciding which additional agents to call.
        // If task_planner determines it's a casual/daily conversation, no extra agents are invoked.
        Self::emit_progress(progress_tx, AgentProgress::StageStarted {
            stage_name: "routing".to_string(),
            detail: "正在分析路由决策...".to_string(),
        });
        let sub_results = self
            .invoke_agents_from_routing_decision(&ctx_arc, msg, sub_results)
            .await;
        tracing::debug!(
            "After Phase 2.5 (routing): agents={:?}",
            sub_results.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>()
        );

        // Phase 2.6: Check if any SubAgent output contains tool call requests
        let sub_results = self
            .handle_sub_agent_tool_requests(&ctx_arc, msg, sub_results)
            .await;
        tracing::debug!(
            "After Phase 2.6 (tool_requests): agents={:?}",
            sub_results.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>()
        );

        // Phase 2.7: Adaptive re-planning — if critical SubAgents failed, try alternatives
        let sub_results = self
            .adaptive_replan_if_needed(&ctx_arc, msg, &plan, sub_results)
            .await;
        tracing::debug!(
            "After Phase 2.7 (replan): agents={:?}",
            sub_results.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>()
        );

        // Phase 3: Aggregate effects
        self.run_hooks(
            HookPoint::PreAggregate,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;
        let all_effects: Vec<Vec<AgentEffect>> =
            sub_results.iter().map(|(_, r)| r.effects.clone()).collect();
        let aggregated_effects = self.effect_aggregator.aggregate(all_effects);
        self.run_hooks(
            HookPoint::PostAggregate,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;

        Some(PreparedPipeline {
            enriched_ctx,
            msg: msg.clone(),
            plan,
            sub_results,
            aggregated_effects,
            cache_key: key,
            start,
            is_simple,
        })
    }

    /// Handle a request through the full pipeline
    #[tracing::instrument(skip(self, ctx, msg), fields(session_id = %ctx.session_id))]
    pub async fn handle_request(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
    ) -> MainCoordinatorResult {
        // Fast path: check response cache before running the full pipeline
        let cache_key = Self::cache_key(ctx, msg);
        if let Some(cached_output) = self.cache.get(&cache_key).await {
            tracing::info!("Response cache hit, skipping pipeline for key: {}", cache_key);
            return MainCoordinatorResult {
                narrative: cached_output.content,
                quality: cached_output.quality,
                cache_hit: true,
                ..Default::default()
            };
        }

        let Some(pipeline) = self.prepare_pipeline(ctx, msg, None).await else {
            return MainCoordinatorResult {
                narrative: "Halted by hooks".to_string(),
                ..Default::default()
            };
        };

        let PreparedPipeline {
            mut enriched_ctx,
            msg,
            plan,
            sub_results,
            aggregated_effects,
            cache_key,
            start,
            is_simple,
        } = pipeline;

        // Determine if we should skip critic
        let skip_critic = if self.execution_policy.force_sub_agent {
            false
        } else {
            self.cost_config
                .as_ref()
                .map(|c| c.skip_critic_for_simple && is_simple)
                .unwrap_or(false)
        };

        // Synthesize results
        let synthesized = self
            .main_agent
            .synthesize_results(&enriched_ctx, &msg, &sub_results)
            .await;

        // Critic review
        let mut hook_msg = Some(msg.clone());
        let mut hook_response: Option<String> = None;
        self.run_hooks(
            HookPoint::PreCritic,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;
        // Prepare SubAgent results for critic cross-validation
        let sub_results_for_critic: Vec<(String, String)> = sub_results
            .iter()
            .filter(|(_, r)| !r.content.is_empty())
            .map(|(id, r)| (id.clone(), r.content.clone()))
            .collect();

        let (narrative, critic_effects, thinking_content) = if skip_critic {
            tracing::debug!("Skipping critic for simple query");
            (synthesized.content, vec![], synthesized.thinking)
        } else if let Some(critic) = &self.critic {
            let issues = critic
                .critique_with_context(&enriched_ctx, &synthesized.content, &sub_results_for_critic)
                .await;
            let critic_fx: Vec<AgentEffect> = issues
                .iter()
                .map(|(sev, msg)| AgentEffect::ReviewNote {
                    severity: sev.clone(),
                    message: msg.clone(),
                    agent_id: "critic".to_string(),
                    target_agent_id: None,
                })
                .collect();

            if issues
                .iter()
                .any(|(sev, _)| matches!(sev, agent_teams_core::effect::ReviewSeverity::Critical))
            {
                let re_synthesized = self
                    .main_agent
                    .synthesize_results(&enriched_ctx, &msg, &sub_results)
                    .await;
                (re_synthesized.content, critic_fx, re_synthesized.thinking)
            } else {
                (synthesized.content, critic_fx, synthesized.thinking)
            }
        } else {
            (synthesized.content, vec![], synthesized.thinking)
        };
        self.run_hooks(
            HookPoint::PostCritic,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;

        let mut final_effects = aggregated_effects;
        final_effects.extend(synthesized.effects);
        final_effects.extend(critic_effects);

        // Collect agent statuses
        let agent_statuses: std::collections::HashMap<String, AgentStatus> = sub_results
            .iter()
            .map(|(id, r)| (id.clone(), r.status.clone()))
            .collect();

        // Publish events
        if let Some(bus) = &self.event_bus {
            bus.publish(SystemEvent::ContentGenerated {
                agent_id: "main_agent".to_string(),
                session_id: enriched_ctx.session_id.clone(),
            });
            bus.publish(SystemEvent::PipelineCompleted {
                session_id: enriched_ctx.session_id.clone(),
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Write to cache (only content + quality needed for cache hits — skip cloning effects)
        self.cache
            .put(
                cache_key,
                AgentOutput {
                    content: narrative.clone(),
                    effects: Vec::new(), // Effects not needed on cache retrieval
                    quality: synthesized.quality,
                    ..Default::default()
                },
            )
            .await;

        // PostRun hooks
        self.run_hooks(
            HookPoint::PostRun,
            &mut enriched_ctx,
            &mut hook_msg,
            &mut hook_response,
        )
        .await;

        // Record turn to memory
        if let Some(mm) = &self.memory_manager {
            if let Err(e) = mm
                .record_turn(
                    &enriched_ctx.session_id,
                    &msg.content,
                    &narrative,
                    &final_effects,
                )
                .await
            {
                tracing::warn!("Failed to record turn to memory: {}", e);
            }

            // Extract and store key facts asynchronously (non-blocking)
            let mm_clone = mm.clone();
            let user_id = enriched_ctx.user_id.clone().unwrap_or_default();
            let session_id = enriched_ctx.session_id.clone();
            let user_msg = msg.content.clone();
            let assistant_msg = narrative.clone();
            tokio::spawn(async move {
                if let Err(e) = mm_clone
                    .extract_and_store_facts(&user_id, &session_id, &user_msg, &assistant_msg)
                    .await
                {
                    tracing::warn!("Fact extraction failed for session {}: {}", session_id, e);
                }
            });
        }

        // Sync all memories through unified bus if available, otherwise fallback
        self.sync_all_memories(
            &enriched_ctx.session_id,
            &sub_results,
            &narrative,
            &final_effects,
        )
        .await;

        MainCoordinatorResult {
            narrative,
            effects: final_effects,
            quality: synthesized.quality,
            apply_result: None,
            plan: Some(plan),
            thinking: thinking_content.map(|t| serde_json::json!({"reasoning": t})),
            agent_statuses,
            total_duration_ms: start.elapsed().as_millis() as u64,
            cache_hit: false,
        }
    }

    /// Adaptive re-planning: if critical SubAgents failed or returned very low quality,
    /// try to execute alternative agents with similar capabilities to recover.
    async fn adaptive_replan_if_needed(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        _plan: &ExecutionPlan,
        sub_results: Vec<(String, AgentOutput)>,
    ) -> Vec<(String, AgentOutput)> {
        // Check for failed or very low quality results
        let failures: Vec<&str> = sub_results
            .iter()
            .filter(|(_, r)| {
                matches!(r.status, AgentStatus::Error(_) | AgentStatus::Timeout) || r.quality < 0.2
            })
            .map(|(id, _)| id.as_str())
            .collect();

        if failures.is_empty() {
            return sub_results;
        }

        tracing::warn!(
            "Adaptive re-plan: {} SubAgent(s) failed: {:?}",
            failures.len(),
            failures
        );

        // Find alternative agents that weren't already called
        let called_ids: std::collections::HashSet<&str> = sub_results
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();

        let all_agents = self.registry.list().await;

        // Capability similarity mapping: find agents with overlapping capabilities
        let capability_groups: std::collections::HashMap<&str, Vec<&str>> = [
            ("tool_request", vec!["task_planner"]),
            ("sentiment_analysis", vec!["sentiment"]),
            ("conversation_summary", vec!["summary"]),
            ("escalation_check", vec!["task_planner"]),
            ("routing_decision", vec!["task_planner"]),
        ]
        .into_iter()
        .collect();

        // Find the best alternatives based on failed agent capabilities
        let mut scored_alternatives: Vec<(String, f32)> = Vec::new();
        for alt_id in &all_agents {
            if called_ids.contains(alt_id.as_str()) || alt_id == "main_agent" {
                continue;
            }
            let mut score: f32 = 0.0;
            // Score based on capability overlap with failed agents
            for failed_id in &failures {
                for group in capability_groups.values() {
                    if group.contains(failed_id) && group.contains(&alt_id.as_str()) {
                        score += 1.0; // Same capability group = high relevance
                    }
                }
            }
            // Bonus for general-purpose agents
            if alt_id == "sentiment" || alt_id == "task_planner" {
                score += 0.5;
            }
            scored_alternatives.push((alt_id.clone(), score));
        }

        // Sort by score (highest first), fallback to any available if no scored matches
        scored_alternatives.sort_by(|a, b| b.1.total_cmp(&a.1));

        let fallback_ids: Vec<String> = if scored_alternatives.iter().any(|(_, s)| *s > 0.0) {
            scored_alternatives
                .into_iter()
                .filter(|(_, s)| *s > 0.0)
                .take(2)
                .map(|(id, _)| id)
                .collect()
        } else {
            // No capability match found, use any available agent
            all_agents
                .into_iter()
                .filter(|id| !called_ids.contains(id.as_str()) && id != "main_agent")
                .take(2)
                .collect()
        };

        if fallback_ids.is_empty() {
            tracing::debug!("No alternative agents available for re-planning");
            return sub_results;
        }

        let mut results = sub_results;

        tracing::info!(
            "Adaptive re-plan: trying alternative agents: {:?}",
            fallback_ids
        );

        let fallback_results = self
            .plan_executor
            .execute_with_policy(
                ctx,
                msg,
                &ExecutionPlan {
                    stages: vec![agent_teams_core::plan::PlanStage {
                        name: "adaptive_fallback".to_string(),
                        sub_agent_ids: fallback_ids,
                        mode: agent_teams_core::pipeline::StageMode::Parallel,
                        required: false,
                        timeout_ms: Some(15_000),
                        message_override: Some(AgentMessage::new(format!(
                            "之前的分析遇到了点问题，你来帮忙看看：\n\n{}",
                            msg.content
                        ))),
                    }],
                    strategy: "Adaptive fallback".to_string(),
                    estimated_duration_ms: 15_000,
                    confidence: 0.5,
                    nodes: Vec::new(),
                    tool_intent: None,
                },
                &self.registry,
                &self.execution_policy,
            )
            .await;

        results.extend(fallback_results);
        results
    }

    /// Dynamically invoke additional agents based on task_planner's routing decision.
    ///
    /// task_planner is the sole authority for deciding which Sub Agents to call beyond
    /// the baseline (task_planner + sentiment). This method:
    /// 1. Parses task_planner's routing_decision effect
    /// 2. If task_planner says `skip_others` (e.g. daily conversation), does nothing
    /// 3. Otherwise, invokes the agents task_planner selected (excluding already-called ones)
    async fn invoke_agents_from_routing_decision(
        &self,
        ctx: &Arc<AgentContext>,
        msg: &AgentMessage,
        sub_results: Vec<(String, AgentOutput)>,
    ) -> Vec<(String, AgentOutput)> {
        // Find task_planner's routing decision
        let routing_decision = sub_results.iter().find_map(|(id, output)| {
            if id != "task_planner" {
                return None;
            }
            output.effects.iter().find_map(|effect| {
                if let AgentEffect::Custom {
                    effect_type, data, ..
                } = effect
                {
                    if effect_type == "routing_decision" {
                        return Some(data.clone());
                    }
                }
                None
            })
        });

        let Some(decision) = routing_decision else {
            tracing::debug!("No routing_decision found from task_planner, skipping dynamic invocation");
            return sub_results;
        };

        // Check if task_planner says to skip additional agents (daily conversation)
        let skip_others = decision
            .get("skip_others")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if skip_others {
            tracing::info!(
                "task_planner decided skip_others=true (daily conversation), no additional agents invoked"
            );
            return sub_results;
        }

        // Extract selected agents from task_planner's decision
        let selected_agents: Vec<String> = decision
            .get("selected_agents")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if selected_agents.is_empty() {
            tracing::info!("task_planner routing decision has no additional agents to invoke");
            return sub_results;
        }

        // Filter out already-called agents and baseline agents (task_planner, sentiment).
        // However, allow re-execution of agents that previously returned errors or very low quality
        // (e.g., tool_agent hitting max iterations) — task_planner explicitly routed to them for a reason.
        let already_called: std::collections::HashMap<&str, &AgentOutput> = sub_results
            .iter()
            .map(|(id, output)| (id.as_str(), output))
            .collect();

        let additional_agents: Vec<String> = selected_agents
            .into_iter()
            .filter(|id| {
                if id == "task_planner" || id == "sentiment" || id == "main_agent" || id == "tool_agent" || id == "knowledge" {
                    return false;
                }
                match already_called.get(id.as_str()) {
                    None => true, // Not yet called — include
                    Some(output) => {
                        // Already called — allow re-execution if previous result was poor
                        let is_poor = output.quality < 0.6
                            || matches!(output.status, agent_teams_core::message::AgentStatus::Error(_))
                            || output.status == agent_teams_core::message::AgentStatus::Timeout
                            || output.content.contains("maximum iterations");
                        if is_poor {
                            tracing::info!(
                                "Allowing re-execution of '{}' (previous quality={:.2}, status={:?})",
                                id, output.quality, output.status
                            );
                        }
                        is_poor
                    }
                }
            })
            .collect();

        if additional_agents.is_empty() {
            tracing::info!(
                "All task_planner-selected agents already called, no additional invocation needed"
            );
            return sub_results;
        }

        let mode = decision
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("Parallel");

        let stage_mode = match mode {
            "Sequential" => agent_teams_core::pipeline::StageMode::Sequential,
            _ => agent_teams_core::pipeline::StageMode::Parallel,
        };

        tracing::info!(
            "task_planner routing decision: invoking additional agents {:?} (mode: {})",
            additional_agents,
            mode
        );

        let additional_plan = ExecutionPlan {
            stages: vec![agent_teams_core::plan::PlanStage {
                name: "task_planner_routed".to_string(),
                sub_agent_ids: additional_agents,
                mode: stage_mode,
                required: false,
                timeout_ms: Some(300_000),
                message_override: None,
            }],
            strategy: format!(
                "Dynamic routing by task_planner ({})",
                decision
                    .get("reasoning")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            ),
            estimated_duration_ms: 120_000,
            confidence: 0.8,
            nodes: Vec::new(),
            tool_intent: None,
        };

        let additional_results = self
            .plan_executor
            .execute_with_policy(ctx, msg, &additional_plan, &self.registry, &self.execution_policy)
            .await;

        // Merge results: replace old results for re-executed agents, append new ones
        let mut results = sub_results;
        for (id, output) in additional_results {
            if let Some(existing) = results.iter_mut().find(|(existing_id, _)| existing_id == &id) {
                tracing::info!("Replacing previous result for re-executed agent '{}'", id);
                *existing = (id, output);
            } else {
                results.push((id, output));
            }
        }
        results
    }

    /// Handle tool call requests from SubAgent outputs.
    /// Detects [[tool:name]] syntax in SubAgent content and executes the tool via task_planner.
    async fn handle_sub_agent_tool_requests(
        &self,
        ctx: &Arc<AgentContext>,
        _msg: &AgentMessage,
        sub_results: Vec<(String, AgentOutput)>,
    ) -> Vec<(String, AgentOutput)> {
        let mut results = sub_results;
        let mut tool_calls_to_execute: Vec<(String, serde_json::Value)> = Vec::new();

        // Scan all SubAgent outputs for tool call requests
        for (agent_id, output) in &results {
            if output.content.is_empty() {
                continue;
            }

            // Find ALL [[tool:...]] patterns in the output (not just the first)
            let mut search_from = 0;
            while let Some(start) = output.content[search_from..].find("[[tool:") {
                let abs_start = search_from + start;
                if let Some(end) = output.content[abs_start..].find("]]") {
                    let tool_ref = &output.content[abs_start + 7..abs_start + end];
                    let parts: Vec<&str> = tool_ref.splitn(2, '|').collect();
                    let raw_name = parts[0].trim().to_string();
                    let args_str = if parts.len() > 1 { parts[1].trim() } else { "{}" };

                    // Parse function call syntax: tool_name(key="value", key2=123)
                    let (tool_name, args) = if raw_name.contains('(') && raw_name.ends_with(')') {
                        let Some(paren_start) = raw_name.find('(') else { continue; };
                        let name = raw_name[..paren_start].trim().to_string();
                        let params_str = &raw_name[paren_start + 1..raw_name.len() - 1];
                        let mut args_map = serde_json::Map::new();
                        for param in params_str.split(',') {
                            let param = param.trim();
                            if let Some(eq_pos) = param.find('=') {
                                let key = param[..eq_pos].trim().to_string();
                                let val_str = param[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                                if let Ok(n) = val_str.parse::<i64>() {
                                    args_map.insert(key, serde_json::json!(n));
                                } else if let Ok(f) = val_str.parse::<f64>() {
                                    args_map.insert(key, serde_json::json!(f));
                                } else if val_str == "true" {
                                    args_map.insert(key, serde_json::json!(true));
                                } else if val_str == "false" {
                                    args_map.insert(key, serde_json::json!(false));
                                } else {
                                    args_map.insert(key, serde_json::json!(val_str));
                                }
                            }
                        }
                        (name, serde_json::Value::Object(args_map))
                    } else {
                        let parsed_args: serde_json::Value = serde_json::from_str(args_str)
                            .unwrap_or(serde_json::json!({}));
                        (raw_name, parsed_args)
                    };

                    let is_empty_args = args.as_object().is_none_or(|m| m.is_empty());
                    if is_empty_args {
                        tracing::warn!(
                            "SubAgent '{}' requested tool call '{}' with empty args, skipping",
                            agent_id, tool_name
                        );
                    } else {
                        tracing::info!(
                            "SubAgent '{}' requested tool call: {} (args: {})",
                            agent_id, tool_name, args
                        );
                        tool_calls_to_execute.push((tool_name, args));
                    }

                    search_from = abs_start + end + 2; // move past ]]
                } else {
                    break; // malformed, no closing ]]
                }
            }
        }

        // If there are tool calls to execute, delegate to task_planner
        if !tool_calls_to_execute.is_empty() {
            let task_planner = match self.registry.get("task_planner").await {
                Some(agent) => agent,
                None => {
                    tracing::error!("task_planner SubAgent not registered");
                    return results;
                }
            };

            for (tool_name, args) in tool_calls_to_execute {
                let tool_meta = serde_json::json!({
                    "tool_name": tool_name,
                    "arguments": args,
                    "call_id": uuid::Uuid::new_v4().to_string(),
                });

                let content = format!(
                    "[TOOL_CALL]\n{}\n[/TOOL_CALL]\n\nSubAgent 请求调用工具 `{}`，请直接执行该工具并返回结果。",
                    tool_meta, tool_name
                );

                let agent_input = AgentInput {
                    system_prompt: ctx.build_system_prompt(),
                    content,
                    recent_history: ctx.recent_history.as_ref().clone(),
                    prior_effects: ctx.turn_effects.clone(),
                    session_id: Some(ctx.session_id.clone()),
                    user_id: ctx.user_id.clone(),
                    available_tools: Vec::new(),
                    agent_context: Some(ctx.clone()),
                };

                let output = task_planner.run(agent_input).await;

                results.push(("task_planner".to_string(), output));
            }
        }

        results
    }

    /// Warm agent-local caches with preloaded memories.
    /// Clears stale caches from previous sessions before loading new ones
    /// to enforce strict session isolation.
    async fn warm_agent_caches(&self, memories: &[agent_teams_core::memory::MemoryEntry]) {
        if memories.is_empty() {
            return;
        }

        // Clear MainAgent's hot cache to prevent cross-session leakage
        self.main_agent.memory_cache().clear_hot();

        if let Some(bus) = &self.unified_bus {
            // Clear shared cache to remove entries from previous sessions
            bus.clear_shared_cache().await;
            // Write current session's memories into shared cache
            for entry in memories {
                bus.shared_cache().put(entry.clone()).await;
            }
            tracing::debug!(
                "Cleared and re-warmed shared cache with {} session memories",
                memories.len()
            );
        } else {
            tracing::debug!(
                "Warming agent caches with {} memories (no unified bus)",
                memories.len()
            );
        }
    }

    /// Sync agent-local memories to global memory store
    async fn sync_agent_memories(&self, session_id: &str, sub_results: &[(String, AgentOutput)]) {
        if let Some(mm) = &self.memory_manager {
            let store = mm.long_term_store();
            for (agent_id, output) in sub_results {
                if !output.content.is_empty() && output.quality > MIN_QUALITY_THRESHOLD {
                    let entry = crate::memory_helpers::build_agent_output_memory_entry(
                        agent_id,
                        &output.content,
                        session_id,
                        output.quality,
                    );
                    if let Err(e) = store.store(entry).await {
                        tracing::warn!("Failed to sync memory for agent {}: {}", agent_id, e);
                    }
                }
            }
        }
    }

    /// Sync MainAgent synthesis output to memory
    async fn sync_main_agent_memory(
        &self,
        session_id: &str,
        narrative: &str,
        effects: &[AgentEffect],
    ) {
        if let Some(mm) = &self.memory_manager {
            if narrative.is_empty() {
                return;
            }
            let entry = crate::memory_helpers::build_main_agent_memory_entry(
                narrative,
                session_id,
                effects.len(),
            );
            if let Err(e) = mm.long_term_store().store(entry).await {
                tracing::warn!("Failed to sync main agent memory: {}", e);
            }
        }
    }

    /// Unified memory sync: uses UnifiedMemoryBus if available, otherwise falls back to manual sync
    async fn sync_all_memories(
        &self,
        session_id: &str,
        sub_results: &[(String, AgentOutput)],
        narrative: &str,
        effects: &[AgentEffect],
    ) {
        let sync_start = std::time::Instant::now();

        if let Some(ref bus) = self.unified_bus {
            // Flush all registered agent caches through the bus
            bus.flush_all_agents().await;
            tracing::debug!(
                "Flushed all agent caches via unified bus ({} agents registered)",
                bus.registered_agent_count()
            );
            // When unified bus is available, it handles agent cache flushing.
            // Still sync main agent memory separately as it's not covered by the bus.
            self.sync_main_agent_memory(session_id, narrative, effects)
                .await;
        } else {
            // Fallback: manual sync when no unified bus is available
            self.sync_agent_memories(session_id, sub_results).await;
            self.sync_main_agent_memory(session_id, narrative, effects)
                .await;
        }

        let sync_elapsed = sync_start.elapsed();
        tracing::info!(
            "Memory sync completed in {}μs for session {} ({} sub-agents)",
            sync_elapsed.as_micros(),
            session_id,
            sub_results.len()
        );
    }

    /// Handle request with error recovery
    #[tracing::instrument(skip(self, ctx, msg), fields(session_id = %ctx.session_id))]
    pub async fn handle_request_with_recovery(
        &self,
        ctx: &AgentContext,
        msg: &AgentMessage,
    ) -> MainCoordinatorResult {
        let result = self.handle_request(ctx, msg).await;

        if !result.narrative.is_empty() {
            return result;
        }

        tracing::warn!("Pipeline produced empty result, falling back to MainAgent direct");
        let ctx_arc = Arc::new(ctx.clone());
        let input = AgentInput {
            system_prompt: ctx.build_system_prompt(),
            content: msg.content.clone(),
            recent_history: ctx.recent_history.as_ref().clone(),
            prior_effects: std::sync::Arc::clone(&ctx.turn_effects),
            session_id: Some(ctx.session_id.clone()),
            user_id: ctx.user_id.clone(),
            available_tools: Vec::new(),
            agent_context: Some(ctx_arc),
        };
        let direct_output = self.main_agent.run(input).await;

        MainCoordinatorResult {
            narrative: direct_output.content,
            effects: direct_output.effects,
            quality: direct_output.quality * 0.8,
            agent_statuses: std::collections::HashMap::from([(
                "main_agent".to_string(),
                AgentStatus::Success,
            )]),
            total_duration_ms: result.total_duration_ms,
            ..Default::default()
        }
    }

    /// Handle a request with true streaming support
    /// Returns a stream of chunks that can be sent to the client.
    ///
    /// During `prepare_pipeline` (which may take a long time for tool execution),
    /// emits periodic `tool_status::Running` events to keep the SSE connection alive
    /// and prevent frontend timeout.
    pub async fn handle_request_stream(
        self: &Arc<Self>,
        ctx: &AgentContext,
        msg: &AgentMessage,
    ) -> Box<dyn futures::Stream<Item = Result<CompletionChunk, ProviderError>> + Unpin + Send>
    {
        // Fast path: check response cache before running the full pipeline.
        // Cache key is content-only (no session_id) to enable cross-session reuse.
        let cache_key = Self::cache_key(ctx, msg);
        if let Some(cached_output) = self.cache.get(&cache_key).await {
            tracing::info!("Response cache hit, skipping pipeline for key: {}", cache_key);
            let content = cached_output.content;
            let chunk = CompletionChunk {
                delta: content,
                thinking_delta: None,
                done: true,
                usage: None,
                tool_call_delta: None,
                tool_status: None,
                sub_agent_results: None,
                companion_state: None,
                agent_progress: None,
            };
            return Box::new(Box::pin(futures::stream::once(async move { Ok(chunk) })));
        }

        // Spawn the entire pipeline in a task, sending all events through a single channel.
        // This allows real-time progress delivery to the client instead of buffering
        // everything until prepare_pipeline completes.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<CompletionChunk, ProviderError>>();

        // Clone data for the spawned task — Arc fields clone cheaply
        let coord = self.clone(); // Arc<Self> clone
        let mut ctx_owned = ctx.clone();
        let msg_owned = msg.clone();

        // Set up tool event channel: task_planner emits events here, we forward to SSE
        let (tool_event_tx, mut tool_event_rx) = tokio::sync::mpsc::unbounded_channel::<agent_teams_core::tool::ToolStatusEvent>();
        ctx_owned.tool_event_tx = Some(Arc::new(tool_event_tx));

        tokio::spawn(async move {
            // Forward tool events from task_planner to the SSE channel
            let tool_forward_tx = tx.clone();
            let tool_forward_handle = tokio::spawn(async move {
                while let Some(event) = tool_event_rx.recv().await {
                    tracing::debug!("Forwarding tool event to SSE: {:?}", &event);
                    let chunk = CompletionChunk {
                        delta: String::new(),
                        thinking_delta: None,
                        done: false,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: Some(event),
                        sub_agent_results: None,
                companion_state: None,
                        agent_progress: None,
                    };
                    if tool_forward_tx.send(Ok(chunk)).is_err() {
                        tracing::debug!("SSE channel closed, stopping tool event forwarding");
                        break;
                    }
                }
                tracing::debug!("Tool event forwarding task ended");
            });
            // Heartbeat task: emits thinking_delta every 5 seconds as fallback
            // if no real progress is being sent through the channel.
            let heartbeat_tx = tx.clone();
            let heartbeat_handle = tokio::spawn(async move {
                let stages = [
                    "正在分析请求...",
                    "正在规划任务...",
                    "正在执行子任务...",
                    "正在整合结果...",
                ];
                let mut tick_count: usize = 0;
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    let stage_msg = stages[tick_count.min(stages.len() - 1)].to_string();
                    tick_count += 1;
                    let chunk = CompletionChunk {
                        delta: String::new(),
                        thinking_delta: Some(stage_msg),
                        done: false,
                        usage: None,
                        tool_call_delta: None,
                        tool_status: None,
                        sub_agent_results: None,
                companion_state: None,
                        agent_progress: None,
                    };
                    if heartbeat_tx.send(Ok(chunk)).is_err() {
                        break; // receiver dropped
                    }
                }
            });

            // Run prepare_pipeline — progress events go directly into the channel
            let pipeline = coord.prepare_pipeline(&ctx_owned, &msg_owned, Some(&tx)).await;

            // Stop heartbeat immediately
            heartbeat_handle.abort();

            // Give the tool forwarding task a moment to flush any remaining events
            // that were emitted right before prepare_pipeline returned.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tool_forward_handle.abort();

            let Some(pipeline) = pipeline else {
                let _ = tx.send(Ok(CompletionChunk {
                    delta: "Halted by hooks".to_string(),
                    thinking_delta: None,
                    done: true,
                    usage: None,
                    tool_call_delta: None,
                    tool_status: None,
                    sub_agent_results: None,
                companion_state: None,
                    agent_progress: None,
                }));
                return;
            };

            let PreparedPipeline {
                enriched_ctx,
                msg,
                plan: _,
                sub_results,
                aggregated_effects,
                cache_key,
                start: _,
                is_simple: _,
            } = pipeline;

            // Tool execution events are now forwarded in real-time from AgentToolLoop
            // via tool_event_tx → tool_event_rx → SSE channel. No need to emit synthetic
            // events here from sub_results.

            // Build SubAgent result summaries for the frontend thinking log.
            // Deduplicate by agent_id: keep the entry with the highest quality.
            tracing::info!(
                "Building sub_agent_summaries from {} results",
                sub_results.len()
            );
            let mut sub_agent_summaries: Vec<agent_teams_core::provider::SubAgentResultSummary> =
                Vec::new();
            for (id, r) in &sub_results {
                if r.content.is_empty() {
                    continue;
                }
                // Extract sticker recommendation from sentiment agent metadata
                let sticker = if id == "sentiment" {
                    r.metadata.as_ref()
                        .and_then(|m| m.get("sticker"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                };
                let summary = agent_teams_core::provider::SubAgentResultSummary {
                    agent_id: id.clone(),
                    content_summary: r.content.chars().take(8192).collect(),
                    thinking: r.thinking.clone(),
                    quality: r.quality,
                    sticker,
                };
                if let Some(existing) = sub_agent_summaries.iter_mut().find(|s| s.agent_id == *id) {
                    if summary.quality > existing.quality {
                        tracing::debug!(
                            "Deduplicating sub_agent '{}': replacing quality {:.2} with {:.2}",
                            id, existing.quality, summary.quality
                        );
                        *existing = summary;
                    }
                } else {
                    sub_agent_summaries.push(summary);
                }
            }
            if !sub_agent_summaries.is_empty() {
                let _ = tx.send(Ok(CompletionChunk {
                    delta: String::new(),
                    thinking_delta: None,
                    done: false,
                    usage: None,
                    tool_call_delta: None,
                    tool_status: None,
                    sub_agent_results: Some(sub_agent_summaries),
                    agent_progress: None,
                    companion_state: None,
                }));
            }

            // Emit progress: synthesis started
            MainAgentCoordinator::emit_progress(Some(&tx), AgentProgress::SynthesisStarted);

            // Synthesize results with true streaming
            let synthesized_stream = coord
                .main_agent
                .synthesize_results_stream(&enriched_ctx, &msg, &sub_results)
                .await;

            // Forward synthesis chunks through the channel while tracking full content
            let mut full_content = String::new();
            let mut final_effects = aggregated_effects;
            final_effects.extend(
                sub_results
                    .iter()
                    .flat_map(|(_, r): &(String, AgentOutput)| r.effects.clone()),
            );

            // Capture sub-agent sync data for post-streaming memory sync
            let sub_sync_data: Vec<(String, String, f32)> = sub_results
                .iter()
                .filter(|(_, r)| !r.content.is_empty() && r.quality > MIN_QUALITY_THRESHOLD)
                .map(|(id, r)| (id.clone(), r.content.clone(), r.quality))
                .collect();

            futures::pin_mut!(synthesized_stream);
            while let Some(chunk_result) = synthesized_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        full_content.push_str(&chunk.delta);
                        let is_done = chunk.done;
                        if tx.send(Ok(chunk)).is_err() {
                            return; // receiver dropped
                        }
                        if is_done {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
            // tx is dropped here when the task completes, closing the receiver stream

            // Publish events
            if let Some(bus) = &coord.event_bus {
                bus.publish(SystemEvent::ContentGenerated {
                    agent_id: "main_agent".to_string(),
                    session_id: enriched_ctx.session_id.clone(),
                });
            }

            // Post-streaming: write to cache
            coord
                .cache
                .put(
                    cache_key,
                    AgentOutput {
                        content: full_content.clone(),
                        effects: Vec::new(), // Effects not needed on cache retrieval
                        quality: 0.9,
                        ..Default::default()
                    },
                )
                .await;

            // Post-streaming: record turn and extract facts
            if let Some(mm) = &coord.memory_manager {
                let session_id = enriched_ctx.session_id.clone();
                let user_id = enriched_ctx.user_id.clone().unwrap_or_default();
                let user_msg = msg.content.clone();

                let _ = mm
                    .record_turn(
                        &session_id,
                        &user_msg,
                        &full_content,
                        &final_effects,
                    )
                    .await;

                // Extract facts asynchronously
                let mm_clone = mm.clone();
                let sid = session_id.clone();
                let uid = user_id.clone();
                let um = user_msg.clone();
                let am = full_content.clone();
                tokio::spawn(async move {
                    if let Err(e) = mm_clone
                        .extract_and_store_facts(&uid, &sid, &um, &am)
                        .await
                    {
                        tracing::warn!(
                            "Fact extraction failed for session {}: {}",
                            sid,
                            e
                        );
                    }
                });

                // Sync sub-agent memories to global store
                let store = mm.long_term_store();
                for (agent_id, content, quality) in sub_sync_data.iter() {
                    let entry = crate::memory_helpers::build_agent_output_memory_entry(
                        agent_id,
                        content,
                        &session_id,
                        *quality,
                    );
                    if let Err(e) = store.store(entry).await {
                        tracing::warn!("Failed to sync agent output to memory: {}", e);
                    }
                }

                // Sync main agent synthesis output
                if !full_content.is_empty() {
                    let main_entry = crate::memory_helpers::build_main_agent_memory_entry(
                        &full_content,
                        &session_id,
                        final_effects.len(),
                    );
                    if let Err(e) = store.store(main_entry).await {
                        tracing::warn!("Failed to sync main agent output to memory: {}", e);
                    }
                }
            }
        }); // end of spawned task

        // Return the channel receiver as the stream — events arrive in real-time
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Box::new(Box::pin(stream))
    }

    /// Start a background task that listens for memory change events
    /// and invalidates related caches (plan cache, sub-agent cache).
    ///
    /// Call this once after construction. The task runs until the event
    /// bus is dropped (i.e. the coordinator is dropped).
    pub fn start_memory_event_listener(&self) {
        let Some(ref bus) = self.unified_bus else {
            return;
        };
        let mut rx = bus.memory_event_bus().subscribe();
        let unified_bus = self.unified_bus.clone();
        let plan_cache = self.plan_cache.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                match event {
                    agent_teams_core::memory_event_bus::MemoryChangeEvent::Stored {
                        agent_id,
                        tags,
                        ..
                    } => {
                        tracing::debug!(
                            "Memory event: agent={} stored new memory with tags {:?}",
                            agent_id,
                            tags
                        );
                        // Invalidate plan cache entries that depend on these tags
                        for tag in &tags {
                            plan_cache.invalidate_by_tag(tag).await;
                        }
                        // Invalidate shared cache entries from this agent so they
                        // get refreshed on the next query
                        if let Some(ref bus) = unified_bus {
                            bus.shared_cache().invalidate_agent(&agent_id).await;
                        }
                    }
                    agent_teams_core::memory_event_bus::MemoryChangeEvent::Updated {
                        agent_id,
                        ..
                    } => {
                        tracing::debug!("Memory event: agent={} updated memory", agent_id);
                    }
                    agent_teams_core::memory_event_bus::MemoryChangeEvent::Invalidated {
                        agent_id,
                        memory_id,
                    } => {
                        tracing::debug!(
                            "Memory event: agent={} invalidated memory {}",
                            agent_id,
                            memory_id
                        );
                    }
                    agent_teams_core::memory_event_bus::MemoryChangeEvent::SessionEnded {
                        session_id,
                    } => {
                        tracing::info!("Memory event: session {} ended, clearing shared cache", session_id);
                        if let Some(ref bus) = unified_bus {
                            bus.shared_cache().invalidate_session(&session_id).await;
                        }
                    }
                }
            }
            tracing::debug!("Memory event listener stopped");
        });
    }
}
