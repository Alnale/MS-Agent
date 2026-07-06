//! Tool execution engine — high-availability wrapper with retry, circuit breaker, concurrency control
//!
//! This module provides `ToolExecutionEngine`, a resilient wrapper around tool execution
//! that adds circuit breaking, retry with backoff, concurrency limiting, and result caching.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::error::{AgentTeamsError, Result};
use crate::tool::{
    RetryPolicy, ToolCall, ToolExecutionContext, ToolResult, ToolStatusEvent, UnifiedToolRegistry,
};
use crate::tool_cache::{ToolCacheConfig, ToolResultCache};

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

#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    /// Healthy — requests flow through; failures accumulate.
    Closed,
    /// Tripped — requests rejected until `open_duration` elapses.
    Open,
    /// Cooldown elapsed — one trial request allowed through; concurrent
    /// callers are rejected until the trial resolves (success → Closed,
    /// failure → Open). Prevents a stampede on a still-broken backend.
    HalfOpen,
}

#[derive(Debug, Clone)]
struct CircuitBreakerState {
    state: CircuitState,
    failure_count: u32,
    threshold: u32,
    last_failure: Option<Instant>,
    open_duration: Duration,
}

impl CircuitBreakerState {
    fn new(threshold: u32, open_duration_secs: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            threshold,
            last_failure: None,
            open_duration: Duration::from_secs(open_duration_secs),
        }
    }

    /// Whether the breaker is in a non-closed state (Open or HalfOpen).
    /// Used for introspection/tests; gating logic uses `should_reject`.
    #[allow(dead_code)]
    fn is_open(&self) -> bool {
        self.state != CircuitState::Closed
    }

    /// Returns true if the caller should be rejected. Implements the
    /// Open→HalfOpen transition: once `open_duration` elapses the first
    /// caller is allowed through as a trial (HalfOpen), while concurrent
    /// callers are rejected to avoid a stampede on a still-broken backend.
    fn should_reject(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => false,
            CircuitState::Open => {
                let elapsed = self
                    .last_failure
                    .map(|t| t.elapsed())
                    .unwrap_or(self.open_duration);
                if elapsed >= self.open_duration {
                    self.state = CircuitState::HalfOpen;
                    false // let the trial request through
                } else {
                    true
                }
            }
            CircuitState::HalfOpen => {
                // A trial is already in flight; reject until it resolves.
                true
            }
        }
    }

    fn record_success(&mut self) {
        // Trial succeeded (or normal success) — close the breaker.
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.last_failure = None;
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
        // In HalfOpen, any failure reopens immediately. In Closed, trip
        // once the threshold is reached.
        if self.state == CircuitState::HalfOpen || self.failure_count >= self.threshold {
            self.state = CircuitState::Open;
        }
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
        let mut cb = self
            .circuit_breakers
            .entry(call.name.clone())
            .or_insert_with(|| CircuitBreakerState::new(5, 60));
        if cb.should_reject() {
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

        // Check tool result cache first (parity with execute_with_resilience).
        // The events path previously bypassed the cache, causing redundant
        // tool calls when the same request was replayed via the streaming API.
        if let Some(ref cache) = self.tool_cache {
            if let Some(cached) = cache.get(call) {
                tracing::info!(
                    tool_call_id = %call.id,
                    tool_name = %call.name,
                    "Tool result cache hit (events path)"
                );
                return Ok(cached);
            }
        }

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
                    // Cache successful results (parity with execute_with_resilience)
                    if result.success {
                        if let Some(ref cache) = self.tool_cache {
                            cache.put(call, &result);
                        }
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
