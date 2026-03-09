/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Rate limiting for townhall API.
//!
//! This module provides token bucket rate limiting per principal and per IP.
//! Features:
//! - Per-principal rate limiting (for authenticated requests)
//! - Per-IP rate limiting (for unauthenticated/anonymous requests)
//! - Stricter limits for mutating operations
//!
//! # Configuration
//! Rate limits can be configured in `tinytown.toml`:
//! ```toml
//! [townhall.rate_limit]
//! requests_per_minute = 60
//! burst_size = 10
//! mutating_requests_per_minute = 30
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::RwLock;

/// Token bucket for rate limiting.
#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, count: f64) -> bool {
        self.refill();
        if self.tokens >= count {
            self.tokens -= count;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
}

/// Rate limiter configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per minute for read operations
    pub requests_per_minute: u32,
    /// Burst size (max tokens)
    pub burst_size: u32,
    /// Maximum requests per minute for mutating operations
    pub mutating_requests_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 120,
            burst_size: 20,
            mutating_requests_per_minute: 60,
        }
    }
}

/// Shared rate limiter state.
#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: Arc<RwLock<HashMap<String, TokenBucket>>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a request should be allowed.
    ///
    /// Uses separate buckets for read vs mutating operations to ensure
    /// mutating requests always use the stricter rate limit regardless of
    /// which type of request came first.
    pub async fn check(&self, key: &str, is_mutating: bool) -> bool {
        let mut buckets = self.buckets.write().await;

        // Use separate bucket keys for read vs write operations
        let bucket_key = if is_mutating {
            format!("{key}:write")
        } else {
            format!("{key}:read")
        };

        let rate = if is_mutating {
            self.config.mutating_requests_per_minute as f64 / 60.0
        } else {
            self.config.requests_per_minute as f64 / 60.0
        };

        let bucket = buckets
            .entry(bucket_key)
            .or_insert_with(|| TokenBucket::new(self.config.burst_size as f64, rate));

        bucket.try_consume(1.0)
    }

    /// Clean up old buckets (call periodically).
    pub async fn cleanup(&self, max_age: Duration) {
        let mut buckets = self.buckets.write().await;
        let now = Instant::now();
        buckets.retain(|_, bucket| now.duration_since(bucket.last_refill) < max_age);
    }
}

/// Rate limit error response.
pub struct RateLimitExceeded;

impl IntoResponse for RateLimitExceeded {
    fn into_response(self) -> Response {
        let body = axum::Json(serde_json::json!({
            "error": "Rate limit exceeded",
            "status": 429,
            "retry_after_seconds": 60
        }));
        (StatusCode::TOO_MANY_REQUESTS, body).into_response()
    }
}

/// Get rate limit key from request (principal ID or IP).
fn get_rate_limit_key(request: &Request<Body>) -> String {
    // Try to get principal from extensions (set by auth middleware)
    if let Some(principal) = request.extensions().get::<super::auth::Principal>() {
        return format!("principal:{}", principal.id);
    }

    // Fall back to IP address from X-Forwarded-For or connection
    // Note: In production, ensure your reverse proxy sets this correctly
    if let Some(forwarded) = request.headers().get("x-forwarded-for")
        && let Ok(value) = forwarded.to_str()
        && let Some(ip) = value.split(',').next()
    {
        return format!("ip:{}", ip.trim());
    }

    // Default fallback
    "unknown".to_string()
}

/// Check if request method is mutating.
fn is_mutating_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    )
}

/// Create a rate limiting middleware.
///
/// This middleware checks rate limits before allowing requests through.
/// It should be applied after authentication middleware so that the principal
/// is available for per-principal rate limiting.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, RateLimitExceeded> {
    let key = get_rate_limit_key(&request);
    let is_mutating = is_mutating_method(request.method());

    if !limiter.check(&key, is_mutating).await {
        return Err(RateLimitExceeded);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_basic() {
        let mut bucket = TokenBucket::new(5.0, 1.0);

        // Should allow 5 requests initially
        for _ in 0..5 {
            assert!(bucket.try_consume(1.0));
        }

        // 6th request should fail
        assert!(!bucket.try_consume(1.0));
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let config = RateLimitConfig {
            requests_per_minute: 60,
            burst_size: 5,
            mutating_requests_per_minute: 30,
        };
        let limiter = RateLimiter::new(config);

        // Should allow burst_size requests
        for _ in 0..5 {
            assert!(limiter.check("test-key", false).await);
        }

        // Next request should be rate limited
        assert!(!limiter.check("test-key", false).await);
    }
}
