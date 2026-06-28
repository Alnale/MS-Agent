use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use agent_teams_core::boxed_agent::AgentOutput;
use agent_teams_core::error::{AgentTeamsError, Result};
use agent_teams_core::provider::{ChatMessage, CompletionRequest, LlmProvider, ToolChoice};
use agent_teams_core::tool::{
    RetryPolicy, Tool, ToolCall, ToolExecutionContext, ToolResult, ToolStatusEvent, UnifiedToolRegistry,
};
use agent_teams_core::tool_cache::{ToolCacheConfig, ToolResultCache};

use crate::tool_param_infer::ParameterInferrer;

/// Tool execution engine — high-availability wrapper with retry, circuit breaker, concurrency control
pub struct ToolExecutionEngine {
    pub registry: Arc<UnifiedToolRegistry>,
    retry_policy: RetryPolicy,
    circuit_breakers: DashMap<String, CircuitBreakerState>,
    metrics: Arc<ToolMetrics>,
    /// Concurrency limiter — max parallel tool executions
    concurrency_limiter: Arc<tokio::sync::Semaphore>,
    /// Tool result cache for avoiding redundant calls
    tool_cache: Option<Arc<ToolResultCache>>,
}

#[derive(Debug, Clone)]
struct CircuitBreakerState {
    failure_count: u32,
    threshold: u32,
    last_failure: Option<Instant>,
    open_duration: Duration,
}

impl CircuitBreakerState {
    fn new(threshold: u32, open_duration_secs: u64) -> Self {
        Self {
            failure_count: 0,
            threshold,
            last_failure: None,
            open_duration: Duration::from_secs(open_duration_secs),
        }
    }

    fn is_open(&self) -> bool {
        if self.failure_count >= self.threshold {
            if let Some(last) = self.last_failure {
                return last.elapsed() < self.open_duration;
            }
        }
        false
    }

    fn record_success(&mut self) {
        self.failure_count = 0;
        self.last_failure = None;
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
    }
}

/// Per-tool metrics snapshot
#[derive(Debug, Clone, Default)]
pub struct PerToolMetrics {
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub total_duration_ms: u64,
    pub avg_duration_ms: u64,
    pub p95_duration_ms: u64,
    /// Recent durations for p95 calculation (ring buffer, max 100)
    recent_durations: Vec<u64>,
}

impl PerToolMetrics {
    pub fn error_rate(&self) -> f64 {
        if self.calls == 0 {
            0.0
        } else {
            self.failures as f64 / self.calls as f64
        }
    }

    /// Compute p95 from recent durations (call only when snapshot is needed)
    pub fn compute_p95(&mut self) {
        if self.recent_durations.is_empty() {
            self.p95_duration_ms = 0;
            return;
        }
        let mut sorted = self.recent_durations.clone();
        sorted.sort_unstable();
        let p95_idx = ((sorted.len() as f64) * 0.95) as usize;
        self.p95_duration_ms = sorted.get(p95_idx.saturating_sub(1)).copied().unwrap_or(0);
    }

    fn update(&mut self, success: bool, duration_ms: u64) {
        self.calls += 1;
        if success {
            self.successes += 1;
        } else {
            self.failures += 1;
        }
        self.total_duration_ms += duration_ms;
        self.avg_duration_ms = self.total_duration_ms / self.calls;
        // Keep last 100 durations for p95 (ring buffer, O(1) insert)
        if self.recent_durations.len() < 100 {
            self.recent_durations.push(duration_ms);
        } else {
            // calls was already incremented above, so use calls-1 for 0-based index
            let idx = (self.calls as usize - 1) % 100;
            self.recent_durations[idx] = duration_ms;
        }
        // Recompute p95 every 10 calls to amortize the sort cost
        if self.calls.is_multiple_of(10) || self.calls <= 5 {
            self.compute_p95();
        }
    }
}

/// Tool execution metrics
#[derive(Debug, Default)]
pub struct ToolMetrics {
    pub total_calls: std::sync::atomic::AtomicU64,
    pub successful_calls: std::sync::atomic::AtomicU64,
    pub failed_calls: std::sync::atomic::AtomicU64,
    pub circuit_breaker_rejections: std::sync::atomic::AtomicU64,
    pub total_duration_ms: std::sync::atomic::AtomicU64,
    /// Per-tool metrics (tool_name -> metrics)
    per_tool: DashMap<String, PerToolMetrics>,
}

impl ToolMetrics {
    pub fn record_success(&self, tool_name: &str, duration: Duration) {
        use std::sync::atomic::Ordering;
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        self.successful_calls.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
        let mut entry = self.per_tool.entry(tool_name.to_string()).or_default();
        entry.update(true, duration.as_millis() as u64);
    }

    pub fn record_failure(&self, tool_name: &str, duration: Duration) {
        use std::sync::atomic::Ordering;
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        self.failed_calls.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
        let mut entry = self.per_tool.entry(tool_name.to_string()).or_default();
        entry.update(false, duration.as_millis() as u64);
    }

    pub fn record_circuit_breaker_rejection(&self, _tool_name: &str) {
        use std::sync::atomic::Ordering;
        self.circuit_breaker_rejections
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Get per-tool metrics snapshot
    pub fn per_tool_metrics(&self) -> Vec<(String, PerToolMetrics)> {
        self.per_tool
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Get error rate highest tools
    pub fn top_error_tools(&self, limit: usize) -> Vec<(String, f64)> {
        let mut tools: Vec<(String, f64)> = self
            .per_tool
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().error_rate()))
            .filter(|(_, rate)| *rate > 0.0)
            .collect();
        tools.sort_by(|a, b| b.1.total_cmp(&a.1));
        tools.into_iter().take(limit).collect()
    }

    /// Get slowest tools by avg duration
    pub fn slowest_tools(&self, limit: usize) -> Vec<(String, u64)> {
        let mut tools: Vec<(String, u64)> = self
            .per_tool
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().avg_duration_ms))
            .collect();
        tools.sort_by(|a, b| b.1.cmp(&a.1));
        tools.into_iter().take(limit).collect()
    }
}

impl ToolExecutionEngine {
    pub fn new(registry: Arc<UnifiedToolRegistry>) -> Self {
        Self {
            registry,
            retry_policy: RetryPolicy::default(),
            circuit_breakers: DashMap::new(),
            metrics: Arc::new(ToolMetrics::default()),
            concurrency_limiter: Arc::new(tokio::sync::Semaphore::new(10)),
            tool_cache: None,
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_max_concurrency(mut self, max: usize) -> Self {
        self.concurrency_limiter = Arc::new(tokio::sync::Semaphore::new(max));
        self
    }

    pub fn with_tool_cache(mut self, config: ToolCacheConfig) -> Self {
        self.tool_cache = Some(Arc::new(ToolResultCache::new(config)));
        self
    }

    /// Execute a tool call with resilience (circuit breaker + retry)
    #[tracing::instrument(skip(self, call, ctx), fields(
        tool_call_id = %call.id,
        tool_name = %call.name,
        agent_id = %ctx.agent_id,
        session_id = %ctx.session_id,
        request_id = %ctx.request_id,
    ))]
    pub async fn execute_with_resilience(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let start = Instant::now();
        tracing::info!("Tool call started");

        // Check tool result cache first
        if let Some(ref cache) = self.tool_cache {
            if let Some(cached) = cache.get(call) {
                tracing::info!(
                    tool_call_id = %call.id,
                    tool_name = %call.name,
                    "Tool result cache hit"
                );
                return Ok(cached);
            }
        }

        if let Some(result) = self.check_circuit_breaker(call) {
            return Ok(result);
        }

        let _permit = self.concurrency_limiter.acquire().await.map_err(|_| {
            AgentTeamsError::Internal("Failed to acquire concurrency permit".to_string())
        })?;

        let core_result = self.execute_with_retry_core(call, ctx, None).await;

        match core_result {
            Ok(result) => {
                self.metrics.record_success(&call.name, start.elapsed());
                if let Some(mut cb) = self.circuit_breakers.get_mut(&call.name) {
                    cb.record_success();
                }

                // Cache successful results
                if result.success {
                    if let Some(ref cache) = self.tool_cache {
                        cache.put(call, &result);
                    }
                }

                tracing::info!(
                    tool_call_id = %call.id,
                    tool_name = %call.name,
                    success = result.success,
                    duration_ms = result.execution_duration_ms,
                    "Tool call completed"
                );
                Ok(result)
            }
            Err(e) => {
                self.record_failure_and_trip(call, start.elapsed());
                tracing::error!(
                    tool_call_id = %call.id,
                    tool_name = %call.name,
                    agent_id = %ctx.agent_id,
                    duration_ms = start.elapsed().as_millis() as u64,
                    error = %e,
                    "Tool call failed after all retries"
                );
                Err(e)
            }
        }
    }

    fn calculate_backoff(&self, attempt: u32) -> Duration {
        let delay = self.retry_policy.base_delay_ms * (1u64 << attempt);
        Duration::from_millis(delay.min(self.retry_policy.max_delay_ms))
    }

    /// Check circuit breaker — returns ToolResult if circuit is open, None if OK to proceed.
    fn check_circuit_breaker(&self, call: &ToolCall) -> Option<ToolResult> {
        let cb = self
            .circuit_breakers
            .entry(call.name.clone())
            .or_insert_with(|| CircuitBreakerState::new(5, 60));
        if cb.is_open() {
            self.metrics.record_circuit_breaker_rejection(&call.name);
            Some(ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                success: false,
                output: serde_json::Value::Null,
                error: Some("Circuit breaker open".to_string()),
                execution_duration_ms: 0,
            })
        } else {
            None
        }
    }

    /// Record failure metrics and trip circuit breaker
    fn record_failure_and_trip(&self, call: &ToolCall, duration: Duration) {
        self.metrics.record_failure(&call.name, duration);
        self.circuit_breakers
            .entry(call.name.clone())
            .or_insert_with(|| CircuitBreakerState::new(5, 60))
            .record_failure();
    }

    /// Check if an error is retryable based on the retry policy.
    /// If retryable_errors is empty, all errors are retryable (backward compatible).
    fn is_retryable(&self, error: &AgentTeamsError) -> bool {
        if self.retry_policy.retryable_errors.is_empty() {
            return true;
        }
        let error_str = error.to_string().to_lowercase();
        self.retry_policy.retryable_errors.iter().any(|pattern| {
            error_str.contains(&pattern.to_lowercase())
        })
    }

    /// Core retry loop — executes the tool with retry, used by both resilience and events paths.
    async fn execute_with_retry_core(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        event_sink: Option<&tokio::sync::mpsc::UnboundedSender<ToolStatusEvent>>,
    ) -> Result<ToolResult> {
        let mut last_error = None;
        for attempt in 0..=self.retry_policy.max_retries {
            let result = if let Some(sink) = event_sink {
                self.registry.execute_with_events(call, ctx, false, Some(sink)).await
            } else {
                self.registry.execute(call, ctx).await
            };

            match result {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Check if error is retryable
                    if !self.is_retryable(&e) {
                        tracing::info!(
                            tool_call_id = %call.id,
                            tool_name = %call.name,
                            error = %e,
                            "Non-retryable error, failing immediately"
                        );
                        return Err(e);
                    }

                    tracing::warn!(
                        tool_call_id = %call.id,
                        tool_name = %call.name,
                        attempt = attempt + 1,
                        max_retries = self.retry_policy.max_retries,
                        error = %e,
                        "Tool call failed, retrying"
                    );
                    last_error = Some(e);
                    if attempt < self.retry_policy.max_retries {
                        let delay = self.calculate_backoff(attempt);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AgentTeamsError::ToolNotFound(format!("Tool execution failed: {}", call.name))
        }))
    }

    /// Execute a tool call with resilience AND real-time status event streaming.
    /// Combines circuit breaker + retry with ToolStatusEvent emission.
    pub async fn execute_with_events(
        &self,
        call: &ToolCall,
        ctx: &ToolExecutionContext,
        skip_approval: bool,
        event_sink: Option<&tokio::sync::mpsc::UnboundedSender<ToolStatusEvent>>,
    ) -> Result<ToolResult> {
        let start = Instant::now();

        if let Some(result) = self.check_circuit_breaker(call) {
            if let Some(sink) = event_sink {
                let _ = sink.send(ToolStatusEvent::Error {
                    call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    error: "Circuit breaker open".to_string(),
                });
            }
            return Ok(result);
        }

        let _permit = self.concurrency_limiter.acquire().await.map_err(|_| {
            AgentTeamsError::Internal("Failed to acquire concurrency permit".to_string())
        })?;

        // For events path, use the registry's execute_with_events directly in the retry loop
        let mut last_error = None;
        for attempt in 0..=self.retry_policy.max_retries {
            match self.registry.execute_with_events(call, ctx, skip_approval, event_sink).await {
                Ok(result) => {
                    self.metrics.record_success(&call.name, start.elapsed());
                    if let Some(mut cb) = self.circuit_breakers.get_mut(&call.name) {
                        cb.record_success();
                    }
                    return Ok(result);
                }
                Err(e) => {
                    tracing::warn!(
                        tool_call_id = %call.id,
                        tool_name = %call.name,
                        attempt = attempt + 1,
                        error = %e,
                        "Tool call with events failed, retrying"
                    );
                    last_error = Some(e);
                    if attempt < self.retry_policy.max_retries {
                        let delay = self.calculate_backoff(attempt);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        self.record_failure_and_trip(call, start.elapsed());
        Err(last_error.unwrap_or_else(|| {
            AgentTeamsError::ToolNotFound(format!("Tool execution failed: {}", call.name))
        }))
    }

    /// Get reference to metrics
    pub fn metrics(&self) -> &Arc<ToolMetrics> {
        &self.metrics
    }

    /// Get reference to registry
    pub fn registry(&self) -> &Arc<UnifiedToolRegistry> {
        &self.registry
    }

    /// Execute multiple tool calls with parallel support
    /// Groups calls by allow_parallel flag and executes accordingly
    pub async fn execute_batch(
        &self,
        calls: &[ToolCall],
        ctx: &ToolExecutionContext,
    ) -> Vec<Result<ToolResult>> {
        let (parallel_calls, sequential_calls) = self.partition_calls(calls);
        let mut results = Vec::new();

        // Execute parallel calls concurrently
        if !parallel_calls.is_empty() {
            let futures: Vec<_> = parallel_calls
                .iter()
                .map(|call| self.execute_with_resilience(call, ctx))
                .collect();
            results.extend(futures::future::join_all(futures).await);
        }

        // Execute sequential calls one by one
        for call in sequential_calls {
            results.push(self.execute_with_resilience(call, ctx).await);
        }

        results
    }

    /// Execute multiple tool calls with parallel support AND event streaming.
    pub async fn execute_batch_with_events(
        &self,
        calls: &[ToolCall],
        ctx: &ToolExecutionContext,
        skip_approval: bool,
        event_sink: Option<&tokio::sync::mpsc::UnboundedSender<ToolStatusEvent>>,
    ) -> Vec<Result<ToolResult>> {
        let (parallel_calls, sequential_calls) = self.partition_calls(calls);
        let mut results = Vec::new();

        if !parallel_calls.is_empty() {
            let futures: Vec<_> = parallel_calls
                .iter()
                .map(|call| self.execute_with_events(call, ctx, skip_approval, event_sink))
                .collect();
            results.extend(futures::future::join_all(futures).await);
        }

        for call in sequential_calls {
            results.push(self.execute_with_events(call, ctx, skip_approval, event_sink).await);
        }

        results
    }

    /// Partition calls into parallel and sequential groups
    fn partition_calls<'a>(&self, calls: &'a [ToolCall]) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
        let mut parallel = Vec::new();
        let mut sequential = Vec::new();

        for call in calls {
            if let Some(tool) = self.registry.get_tool(&call.name) {
                if tool.allow_parallel {
                    parallel.push(call);
                } else {
                    sequential.push(call);
                }
            } else {
                sequential.push(call);
            }
        }

        (parallel, sequential)
    }

    /// Get tool health status based on metrics
    pub fn get_tool_health(&self, tool_name: &str) -> ToolHealthStatus {
        let metrics = self.metrics.per_tool_metrics();
        if let Some((_, tool_metrics)) = metrics.iter().find(|(name, _)| name == tool_name) {
            let error_rate = tool_metrics.error_rate();
            let avg_duration = tool_metrics.avg_duration_ms;

            let status = if error_rate > 0.5 {
                HealthStatus::Unhealthy
            } else if error_rate > 0.2 || avg_duration > 10000 {
                HealthStatus::Degraded
            } else {
                HealthStatus::Healthy
            };

            ToolHealthStatus {
                tool_name: tool_name.to_string(),
                status,
                error_rate,
                avg_duration_ms: avg_duration,
                total_calls: tool_metrics.calls,
            }
        } else {
            ToolHealthStatus {
                tool_name: tool_name.to_string(),
                status: HealthStatus::Unknown,
                error_rate: 0.0,
                avg_duration_ms: 0,
                total_calls: 0,
            }
        }
    }
}

/// Tool health status
#[derive(Debug, Clone)]
pub struct ToolHealthStatus {
    pub tool_name: String,
    pub status: HealthStatus,
    pub error_rate: f64,
    pub avg_duration_ms: u64,
    pub total_calls: u64,
}

/// Health status
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

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
    pub param_inferrer: Option<Arc<ParameterInferrer>>,
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

    pub fn with_param_inferrer(mut self, inferrer: Arc<ParameterInferrer>) -> Self {
        self.param_inferrer = Some(inferrer);
        self
    }

    /// Truncate tool output for SSE events to avoid oversized payloads.
    /// Keeps essential metadata (url, batch info, titles, links) but removes
    /// large content fields like crawled_content, text, merged_text.
    fn truncate_tool_output_for_event(output: &serde_json::Value) -> serde_json::Value {
        const MAX_FIELD_LEN: usize = 500;

        let mut truncated = output.clone();

        // Truncate large string fields
        if let Some(obj) = truncated.as_object_mut() {
            for key in ["crawled_content", "text", "merged_text", "body"] {
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

            // Truncate text in results array
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
    /// This helps the LLM understand how to chain tools together.
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

        // Build data flow context from available tools for the system prompt
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

        // Clone tool list once — it doesn't change between iterations
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

            // Build request with tools
            let request = CompletionRequest {
                model: String::new(),
                messages: messages.clone(),
                tools: tools.clone(),
                tool_choice: Some(ToolChoice::Auto),
                max_tokens: Some(65536),
                temperature: Some(0.5),
                system: enhanced_system_prompt.clone(),
                stream: false,
                metadata: None,
                thinking: None,
            };

            let response = self.provider.complete(request).await.map_err(|e| {
                AgentTeamsError::Provider(format!("LLM call failed in tool loop: {}", e))
            })?;

            // If LLM didn't request tool calls, return the result
            if response.tool_calls.is_empty() {
                return Ok((AgentOutput {
                    content: response.content,
                    thinking: response.thinking,
                    quality: 0.9,
                    ..Default::default()
                }, tool_history));
            }

            // Guard: detect re-call of the same tool with same arguments.
            // If all requested tools were already executed, the LLM is looping — break.
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

            // Guard: if LLM returned thinking-only (no text) and the requested tool
            // was already called by name, the LLM is confused — break to prevent
            // spawning duplicate browser instances for tools like xxt.
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

            // Execute tool calls with parameter inference — batch parallel ones, run sequential ones in order
            let mut tool_results = Vec::new();
            let mut parallel_futures = Vec::new();
            let mut sequential_calls = Vec::new();

            for call in &response.tool_calls {
                // Enrich tool call with inferred parameters, using tool_history for cross-tool data flow
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

            // Helper to emit tool events if agent_context has a sender
            let emit_event = |event: agent_teams_core::tool::ToolStatusEvent| {
                if let Some(ref agent_ctx) = ctx.agent_context {
                    if let Some(ref tx) = agent_ctx.tool_event_tx {
                        tracing::debug!("Emitting tool event: {:?}", &event);
                        let _ = tx.send(event);
                    } else {
                        tracing::debug!("No tool_event_tx on agent_context");
                    }
                } else {
                    tracing::debug!("No agent_context on ctx");
                }
            };

            // Run parallel calls concurrently
            if !parallel_futures.is_empty() {
                // Emit executing events for all parallel calls
                for call in &response.tool_calls {
                    if let Some(tool) = self.tool_engine.registry().get_tool(&call.name) {
                        if tool.allow_parallel {
                            emit_event(agent_teams_core::tool::ToolStatusEvent::Executing {
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

                    // Emit completed event (truncate output to avoid oversized SSE events)
                    let truncated_output = Self::truncate_tool_output_for_event(&result.output);
                    emit_event(agent_teams_core::tool::ToolStatusEvent::Completed {
                        call_id: result.call_id.clone(),
                        tool_name: result.name.clone(),
                        success: result.success,
                        output: truncated_output,
                        error: result.error.clone(),
                        duration_ms: result.execution_duration_ms,
                    });

                    tool_results.push(result);
                    // Keep tool_history ordering consistent
                    if let Some(r) = tool_results.last() {
                        tool_history.push((call, r.clone()));
                    }
                }
            }

            // Run sequential calls one by one
            for call in sequential_calls {
                let key = format!("{}:{}", call.name, call.arguments);
                executed_tool_keys.insert(key);
                executed_tool_names.insert(call.name.clone());

                // Emit executing event
                emit_event(agent_teams_core::tool::ToolStatusEvent::Executing {
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

                // Emit completed event (truncate output to avoid oversized SSE events)
                let truncated_output = Self::truncate_tool_output_for_event(&result.output);
                emit_event(agent_teams_core::tool::ToolStatusEvent::Completed {
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

            // Append tool results to message history
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: response.content.clone(),
                cache_control: None,
                tool_call_id: None,
                tool_calls: Some(response.tool_calls.clone()),
            });

            for result in &tool_results {
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: serde_json::to_string(&result.compact()).unwrap_or_default(),
                    cache_control: None,
                    tool_call_id: Some(result.call_id.clone()),
                    tool_calls: None,
                });
            }

            // Smart error recovery: if a tool failed, analyze the error and inject a recovery hint
            // to help the LLM self-correct on the next iteration
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

        // Max iterations reached — return last response
        Ok((AgentOutput {
            content: "Tool execution loop reached maximum iterations.".to_string(),
            quality: 0.5,
            ..Default::default()
        }, tool_history))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_state() {
        let mut cb = CircuitBreakerState::new(3, 60);
        assert!(!cb.is_open());

        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open());

        cb.record_failure();
        assert!(cb.is_open());

        cb.record_success();
        assert!(!cb.is_open());
    }

    #[test]
    fn test_tool_metrics_default() {
        let metrics = ToolMetrics::default();
        use std::sync::atomic::Ordering;
        assert_eq!(metrics.total_calls.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.successful_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay_ms, 100);
        assert_eq!(policy.max_delay_ms, 5000);
    }
}
