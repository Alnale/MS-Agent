use async_trait::async_trait;
use futures::Stream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use agent_core::provider::*;

/// Guard that records a success on the circuit breaker when dropped,
/// unless an error was seen during the stream's lifetime.
struct StreamSuccessGuard {
    state: Arc<RwLock<CircuitInner>>,
    saw_error: Arc<AtomicBool>,
}

impl Drop for StreamSuccessGuard {
    fn drop(&mut self) {
        if !self.saw_error.load(Ordering::Relaxed) {
            let mut s = self.state.write().unwrap_or_else(|e| e.into_inner());
            s.failure_count = 0;
            s.half_open_inflight = false;
            s.state = CircuitStateEnum::Closed;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitStateEnum {
    Closed,
    Open,
    HalfOpen,
}

struct CircuitInner {
    state: CircuitStateEnum,
    failure_count: u32,
    last_failure: Option<Instant>,
    half_open_inflight: bool,
}

/// Circuit breaker wrapper for LLM providers.
///
/// Uses `std::sync::RwLock` intentionally: the critical path (check_state,
/// record_success, record_failure) is a few atomic-like operations on small
/// fields. A `tokio::sync::RwLock` would add async overhead for no benefit
/// since the lock is never held across `.await` points.
pub struct CircuitBreakerProvider {
    inner: Box<dyn LlmProvider>,
    failure_threshold: u32,
    open_duration_secs: u64,
    inner_state: Arc<RwLock<CircuitInner>>,
}

impl CircuitBreakerProvider {
    pub fn new(
        inner: Box<dyn LlmProvider>,
        failure_threshold: u32,
        open_duration_secs: u64,
    ) -> Self {
        Self {
            inner,
            failure_threshold,
            open_duration_secs,
            inner_state: Arc::new(RwLock::new(CircuitInner {
                state: CircuitStateEnum::Closed,
                failure_count: 0,
                last_failure: None,
                half_open_inflight: false,
            })),
        }
    }

    fn check_state(&self) -> bool {
        // Fast path: read-only check for Closed state
        {
            let state = self.inner_state.read().unwrap_or_else(|e| e.into_inner());
            match state.state {
                CircuitStateEnum::Closed => return true,
                CircuitStateEnum::HalfOpen => return !state.half_open_inflight,
                CircuitStateEnum::Open => {
                    if let Some(last) = state.last_failure {
                        if last.elapsed().as_secs() < self.open_duration_secs {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
        }
        // Slow path: upgrade to write lock only when transitioning Open -> HalfOpen
        let mut state = self.inner_state.write().unwrap_or_else(|e| e.into_inner());
        if state.state == CircuitStateEnum::Open {
            if let Some(last) = state.last_failure {
                if last.elapsed().as_secs() >= self.open_duration_secs {
                    state.state = CircuitStateEnum::HalfOpen;
                    state.half_open_inflight = true;
                    return true;
                }
            }
        }
        false
    }

    fn record_success(&self) {
        let mut state = self.inner_state.write().unwrap_or_else(|e| e.into_inner());
        state.failure_count = 0;
        state.half_open_inflight = false;
        state.state = CircuitStateEnum::Closed;
    }

    fn record_failure(&self) {
        let mut state = self.inner_state.write().unwrap_or_else(|e| e.into_inner());
        state.failure_count += 1;
        state.last_failure = Some(Instant::now());
        state.half_open_inflight = false;
        if state.failure_count >= self.failure_threshold {
            state.state = CircuitStateEnum::Open;
            tracing::warn!("Circuit breaker opened for provider {}", self.inner.id());
        }
    }
}

#[async_trait]
impl LlmProvider for CircuitBreakerProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn models(&self) -> Vec<String> {
        self.inner.models()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, ProviderError> {
        if !self.check_state() {
            return Err(ProviderError::Unavailable(
                "Circuit breaker is open".to_string(),
            ));
        }
        match self.inner.complete(request).await {
            Ok(resp) => {
                self.record_success();
                Ok(resp)
            }
            Err(e) => {
                self.record_failure();
                Err(e)
            }
        }
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<
        Box<dyn Stream<Item = std::result::Result<CompletionChunk, ProviderError>> + Unpin + Send>,
        ProviderError,
    > {
        if !self.check_state() {
            return Err(ProviderError::Unavailable(
                "Circuit breaker is open".to_string(),
            ));
        }
        match self.inner.complete_stream(request).await {
            Ok(stream) => {
                use futures::StreamExt;
                let state = self.inner_state.clone();
                let threshold = self.failure_threshold;
                let saw_error = Arc::new(AtomicBool::new(false));
                let saw_error_clone = saw_error.clone();

                let wrapped = stream.inspect(move |result| {
                    if result.is_err() {
                        saw_error_clone.store(true, Ordering::Relaxed);
                        let mut s = state.write().unwrap_or_else(|e| e.into_inner());
                        s.failure_count += 1;
                        s.last_failure = Some(Instant::now());
                        s.half_open_inflight = false;
                        if s.failure_count >= threshold {
                            s.state = CircuitStateEnum::Open;
                        }
                    }
                });

                // Create a guard that records success when the stream is dropped
                // without having seen any errors.
                let guard = StreamSuccessGuard {
                    state: self.inner_state.clone(),
                    saw_error,
                };

                // Wrap the stream with the guard kept alive alongside it.
                // Pin<Box<...>> ensures Unpin.
                let guarded = futures::stream::unfold(
                    (Box::pin(wrapped), guard),
                    |(mut stream, guard)| async move {
                        use futures::StreamExt;
                        stream.next().await.map(|item| (item, (stream, guard)))
                    },
                );
                Ok(Box::new(Box::pin(guarded)))
            }
            Err(e) => {
                self.record_failure();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockProvider {
        fail_count: AtomicU32,
        max_fails: u32,
    }

    impl MockProvider {
        fn new(max_fails: u32) -> Self {
            Self {
                fail_count: AtomicU32::new(0),
                max_fails,
            }
        }
    }

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
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, ProviderError> {
            let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if count < self.max_fails {
                Err(ProviderError::Unavailable("mock failure".to_string()))
            } else {
                Ok(CompletionResponse {
                    content: "ok".to_string(),
                    thinking: None,
                    model: "mock-model".to_string(),
                    usage: TokenUsage::default(),
                    stop_reason: None,
                    tool_calls: vec![],
                    annotations: vec![],
                })
            }
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<
            Box<
                dyn Stream<Item = std::result::Result<CompletionChunk, ProviderError>>
                    + Unpin
                    + Send,
            >,
            ProviderError,
        > {
            Err(ProviderError::Other("not implemented".to_string()))
        }
    }

    fn make_request() -> CompletionRequest {
        CompletionRequest {
            model: "mock-model".to_string(),
            messages: vec![],
            input: None,
            max_tokens: None,
            temperature: None,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            response_format: None,
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_starts_closed() {
        let provider = CircuitBreakerProvider::new(Box::new(MockProvider::new(0)), 3, 60);
        let result = provider.complete(make_request()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_threshold() {
        let provider = CircuitBreakerProvider::new(
            Box::new(MockProvider::new(100)), // always fail
            3,
            60,
        );
        // Fail 3 times to open the circuit
        for _ in 0..3 {
            let _ = provider.complete(make_request()).await;
        }
        // Next call should be rejected immediately
        let result = provider.complete(make_request()).await;
        assert!(matches!(result, Err(ProviderError::Unavailable(_))));
    }

    #[tokio::test]
    async fn test_circuit_breaker_resets_on_success() {
        let provider = CircuitBreakerProvider::new(
            Box::new(MockProvider::new(2)), // fail first 2, then succeed
            3,
            60,
        );
        // Fail twice
        let _ = provider.complete(make_request()).await;
        let _ = provider.complete(make_request()).await;
        // Third call succeeds -> circuit resets
        let result = provider.complete(make_request()).await;
        assert!(result.is_ok());
        // Should still be closed
        let result = provider.complete(make_request()).await;
        assert!(result.is_ok());
    }
}
