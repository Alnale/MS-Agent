use async_trait::async_trait;
use futures::Stream;

use agent_core::provider::*;

/// Transparent retry wrapper for LLM providers
pub struct RetryProvider {
    inner: Box<dyn LlmProvider>,
    max_retries: u32,
    base_delay_ms: u64,
}

impl RetryProvider {
    pub fn new(inner: Box<dyn LlmProvider>, max_retries: u32, base_delay_ms: u64) -> Self {
        Self {
            inner,
            max_retries,
            base_delay_ms,
        }
    }

    fn is_retryable(err: &ProviderError) -> bool {
        matches!(
            err,
            ProviderError::Unavailable(_) | ProviderError::RateLimited { .. }
        )
    }
}

#[async_trait]
impl LlmProvider for RetryProvider {
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
        // Only clone once for retries instead of on every attempt
        let retry_request = if self.max_retries > 0 { Some(request.clone()) } else { None };
        let mut request = Some(request);
        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            // Take ownership on first attempt, use pre-cloned copy on retries
            let req = request.take().or_else(|| retry_request.clone())
                .expect("retry request should be available");
            match self.inner.complete(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) if Self::is_retryable(&e) && attempt < self.max_retries => {
                    let base = self
                        .base_delay_ms
                        .saturating_mul(2u64.saturating_pow(attempt));
                    let jitter = if self.base_delay_ms == 0 {
                        0
                    } else {
                        rand::random::<u64>() % self.base_delay_ms
                    };
                    let computed = base.saturating_add(jitter);
                    // Honor server-provided Retry-After (seconds) on 429s —
                    // never wait less than the server asked, but allow our own
                    // backoff to wait longer if it would have.
                    let delay = match &e {
                        ProviderError::RateLimited {
                            retry_after: Some(secs),
                        } => std::cmp::max(computed, secs.saturating_mul(1000)),
                        _ => computed,
                    };
                    tracing::warn!(
                        "Provider {} attempt {} failed, retrying in {}ms: {}",
                        self.inner.id(),
                        attempt + 1,
                        delay,
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| ProviderError::Other("Max retries exceeded".to_string())))
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> std::result::Result<
        Box<dyn Stream<Item = std::result::Result<CompletionChunk, ProviderError>> + Unpin + Send>,
        ProviderError,
    > {
        let retry_request = if self.max_retries > 0 { Some(request.clone()) } else { None };
        let mut request = Some(request);
        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            let req = request.take().or_else(|| retry_request.clone())
                .expect("retry request should be available");
            match self.inner.complete_stream(req).await {
                Ok(stream) => return Ok(stream),
                Err(e) if Self::is_retryable(&e) && attempt < self.max_retries => {
                    let base = self
                        .base_delay_ms
                        .saturating_mul(2u64.saturating_pow(attempt));
                    let jitter = if self.base_delay_ms == 0 {
                        0
                    } else {
                        rand::random::<u64>() % self.base_delay_ms
                    };
                    let computed = base.saturating_add(jitter);
                    // Honor server-provided Retry-After (seconds) on 429s —
                    // never wait less than the server asked, but allow our own
                    // backoff to wait longer if it would have.
                    let delay = match &e {
                        ProviderError::RateLimited {
                            retry_after: Some(secs),
                        } => std::cmp::max(computed, secs.saturating_mul(1000)),
                        _ => computed,
                    };
                    tracing::warn!(
                        "Provider {} stream attempt {} failed, retrying in {}ms: {}",
                        self.inner.id(),
                        attempt + 1,
                        delay,
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| ProviderError::Other("Max retries exceeded".to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockProvider {
        call_count: AtomicU32,
        fail_until: u32,
    }

    impl MockProvider {
        fn new(fail_until: u32) -> Self {
            Self {
                call_count: AtomicU32::new(0),
                fail_until,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn name(&self) -> &str {
            "Mock"
        }
        fn models(&self) -> Vec<String> {
            vec![]
        }

        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, ProviderError> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_until {
                Err(ProviderError::Unavailable("mock failure".to_string()))
            } else {
                Ok(CompletionResponse {
                    content: "ok".to_string(),
                    thinking: None,
                    model: "mock".to_string(),
                    usage: TokenUsage::default(),
                    stop_reason: None,
                    tool_calls: vec![],
                    annotations: vec![],
                })
            }
        }

        async fn complete_stream(
            &self,
            _req: CompletionRequest,
        ) -> std::result::Result<
            Box<
                dyn futures::Stream<Item = std::result::Result<CompletionChunk, ProviderError>>
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
            model: "mock".to_string(),
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
    async fn test_retry_succeeds_on_first_try() {
        let provider = RetryProvider::new(Box::new(MockProvider::new(0)), 3, 10);
        let result = provider.complete(make_request()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_failures() {
        let provider = RetryProvider::new(Box::new(MockProvider::new(2)), 3, 10);
        let result = provider.complete(make_request()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let provider = RetryProvider::new(Box::new(MockProvider::new(100)), 2, 10);
        let result = provider.complete(make_request()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_non_retryable_error_no_retry() {
        // Auth errors should not be retried
        struct AuthFailProvider;
        #[async_trait]
        impl LlmProvider for AuthFailProvider {
            fn id(&self) -> &str {
                "auth-fail"
            }
            fn name(&self) -> &str {
                "Auth Fail"
            }
            fn models(&self) -> Vec<String> {
                vec![]
            }
            async fn complete(
                &self,
                _req: CompletionRequest,
            ) -> std::result::Result<CompletionResponse, ProviderError> {
                Err(ProviderError::Auth("bad key".to_string()))
            }
            async fn complete_stream(
                &self,
                _req: CompletionRequest,
            ) -> std::result::Result<
                Box<
                    dyn futures::Stream<Item = std::result::Result<CompletionChunk, ProviderError>>
                        + Unpin
                        + Send,
                >,
                ProviderError,
            > {
                Err(ProviderError::Auth("bad key".to_string()))
            }
        }

        let provider = RetryProvider::new(Box::new(AuthFailProvider), 3, 10);
        let result = provider.complete(make_request()).await;
        assert!(matches!(result, Err(ProviderError::Auth(_))));
    }
}
