use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;

/// Per-IP token bucket rate limiter.
///
/// Each IP gets `max_requests` tokens that refill at `refill_rate` per second.
/// Returns HTTP 429 when exhausted.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<String, Bucket>>,
    max_requests: u32,
    refill_rate: f64, // tokens per second
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            max_requests,
            refill_rate: max_requests as f64 / window_secs as f64,
        }
    }

    fn check(&self, ip: &str) -> bool {
        let now = Instant::now();

        let mut entry = self.buckets.entry(ip.to_string()).or_insert_with(|| Bucket {
            tokens: self.max_requests as f64,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(entry.last_refill).as_secs_f64();
        entry.tokens = (entry.tokens + elapsed * self.refill_rate).min(self.max_requests as f64);
        entry.last_refill = now;

        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Periodically evict stale buckets. Call once at startup.
    ///
    /// Returns a `JoinHandle` so the caller can abort the task on shutdown or
    /// in tests — previously the spawned task ran forever and leaked across
    /// `RateLimiter` instances (relevant in test suites that build many
    /// limiters). The handle may be dropped to let the task run independently.
    pub fn start_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        let buckets = self.buckets.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                let cutoff = Instant::now() - Duration::from_secs(600);
                buckets.retain(|_, b| b.last_refill > cutoff);
            }
        })
    }
}

/// Axum middleware for per-IP rate limiting.
pub async fn rate_limit_middleware(
    limiter: RateLimiter,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    let ip = extract_client_ip(&req);

    if !limiter.check(&ip) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "status": "error",
                "error": "Rate limit exceeded. Try again later.",
                "error_code": "rate_limited",
            })),
        )
            .into_response();
    }

    next.run(req).await
}

fn extract_client_ip(req: &Request) -> String {
    // Try X-Forwarded-For first (for reverse proxies)
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}
