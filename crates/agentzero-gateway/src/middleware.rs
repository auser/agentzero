use axum::{
    body::Body,
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Gateway middleware configuration
// ---------------------------------------------------------------------------

/// Configuration for gateway middleware layers.
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Maximum request body size in bytes. Default: 1 MB.
    pub max_body_bytes: usize,
    /// Global rate limit: max requests per window. 0 = unlimited. Default: 600.
    pub rate_limit_max: u64,
    /// Per-identity rate limit: max requests per window per API key/bearer. 0 = use global only. Default: 0.
    pub rate_limit_per_identity: u64,
    /// Rate limit window in seconds. Default: 60.
    pub rate_limit_window_secs: u64,
    /// CORS: allowed origins. Empty = no CORS headers. "*" = allow all.
    pub cors_allowed_origins: Vec<String>,
    /// Whether TLS is enabled. When true, HSTS headers are added to all responses.
    pub tls_enabled: bool,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024, // 1 MB
            rate_limit_max: 600,         // 10 req/s over 60s window
            rate_limit_per_identity: 0,  // disabled by default (use global only)
            rate_limit_window_secs: 60,
            cors_allowed_origins: vec![],
            tls_enabled: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Rate limiter (sliding window counter with per-identity support)
// ---------------------------------------------------------------------------

/// Per-identity sliding window counter.
struct WindowCounter {
    max_requests: u64,
    window_secs: u64,
    counter: AtomicU64,
    window_start: std::sync::Mutex<Instant>,
}

impl WindowCounter {
    fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            counter: AtomicU64::new(0),
            window_start: std::sync::Mutex::new(Instant::now()),
        }
    }

    fn try_acquire(&self) -> bool {
        if self.max_requests == 0 {
            return true;
        }
        let mut start = self.window_start.lock().expect("rate limiter lock");
        let elapsed = start.elapsed().as_secs();
        if elapsed >= self.window_secs {
            *start = Instant::now();
            self.counter.store(1, Ordering::Relaxed);
            return true;
        }
        drop(start);
        let count = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        count <= self.max_requests
    }

    fn remaining(&self) -> u64 {
        if self.max_requests == 0 {
            return u64::MAX;
        }
        self.max_requests
            .saturating_sub(self.counter.load(Ordering::Relaxed))
    }

    fn current_count(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    fn window_reset_secs(&self) -> u64 {
        let start = self.window_start.lock().expect("rate limiter lock");
        self.window_secs.saturating_sub(start.elapsed().as_secs())
    }

    /// Returns true if the window has been idle (no requests) and is expired.
    fn is_expired(&self) -> bool {
        let start = self.window_start.lock().expect("rate limiter lock");
        start.elapsed().as_secs() >= self.window_secs * 2
    }
}

/// Rate limiter with both global and per-identity sliding window counters.
///
/// - **Global counter**: applied to all requests (identified as `"_global"`).
/// - **Per-identity counters**: applied per API key (`key_id`) or bearer token.
///   Requires `per_identity_max > 0` to be enabled.
///
/// Designed for single-process use (not distributed).
pub struct RateLimiter {
    global: WindowCounter,
    per_identity_max: u64,
    window_secs: u64,
    identities: DashMap<String, WindowCounter>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// - `global_max`: global rate limit (0 = unlimited).
    /// - `per_identity_max`: per-identity rate limit (0 = per-identity limiting disabled).
    /// - `window_secs`: sliding window duration in seconds.
    pub fn new(global_max: u64, window_secs: u64) -> Self {
        Self {
            global: WindowCounter::new(global_max, window_secs),
            per_identity_max: 0,
            window_secs,
            identities: DashMap::new(),
        }
    }

    /// Set the per-identity rate limit. Must be called before use.
    pub fn with_per_identity(mut self, per_identity_max: u64) -> Self {
        self.per_identity_max = per_identity_max;
        self
    }

    /// Try to acquire a permit for a given identity.
    /// Returns `(allowed, limit, remaining, reset_secs)`.
    pub fn try_acquire_for(&self, identity: &str) -> (bool, u64, u64, u64) {
        // Always check global limit first.
        if !self.global.try_acquire() {
            let limit = self.effective_limit();
            return (false, limit, 0, self.global.window_reset_secs());
        }

        // If per-identity limiting is disabled, use global counters for headers.
        if self.per_identity_max == 0 {
            let limit = self.global.max_requests;
            let remaining = self.global.remaining();
            let reset = self.global.window_reset_secs();
            return (true, limit, remaining, reset);
        }

        // Per-identity check.
        let counter = self
            .identities
            .entry(identity.to_string())
            .or_insert_with(|| WindowCounter::new(self.per_identity_max, self.window_secs));
        let allowed = counter.try_acquire();
        let limit = self.per_identity_max;
        let remaining = counter.remaining();
        let reset = counter.window_reset_secs();
        (allowed, limit, remaining, reset)
    }

    /// Legacy method for backward compatibility: tries global-only acquisition.
    pub fn try_acquire(&self) -> bool {
        self.global.try_acquire()
    }

    /// Current global count.
    pub fn current_count(&self) -> u64 {
        self.global.current_count()
    }

    /// Remaining in global window.
    pub fn remaining(&self) -> u64 {
        self.global.remaining()
    }

    /// Remove per-identity buckets that have been idle for 2x the window duration.
    pub fn gc_expired_identities(&self) -> usize {
        let before = self.identities.len();
        self.identities.retain(|_, counter| !counter.is_expired());
        before - self.identities.len()
    }

    fn effective_limit(&self) -> u64 {
        if self.per_identity_max > 0 {
            self.per_identity_max
        } else {
            self.global.max_requests
        }
    }
}

// ---------------------------------------------------------------------------
// Request size limit middleware
// ---------------------------------------------------------------------------

/// Middleware that rejects requests with a `content-length` exceeding the limit.
pub async fn request_size_limit(request: Request<Body>, next: Next, max_bytes: usize) -> Response {
    if let Some(content_length) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
    {
        if content_length > max_bytes {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "request body too large ({} bytes, max {})",
                    content_length, max_bytes
                ),
            )
                .into_response();
        }
    }
    next.run(request).await
}

// ---------------------------------------------------------------------------
// Rate limiting middleware
// ---------------------------------------------------------------------------

/// Rate limit response header names.
pub const X_RATELIMIT_LIMIT: &str = "x-ratelimit-limit";
pub const X_RATELIMIT_REMAINING: &str = "x-ratelimit-remaining";
pub const X_RATELIMIT_RESET: &str = "x-ratelimit-reset";

/// Extract a rate-limit identity key from the request's Authorization header.
///
/// - `Bearer az_...` (API key) → hash to a short key ID for bucketing.
/// - `Bearer <other>` (session/bearer token) → `"bearer"`.
/// - No Authorization header → `"_anonymous"`.
fn extract_rate_limit_identity(request: &Request<Body>) -> String {
    let Some(auth) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return "_anonymous".to_string();
    };
    let Some(token) = auth.strip_prefix("Bearer ") else {
        return "_anonymous".to_string();
    };
    if token.starts_with("az_") {
        // API key — use first 16 chars as identity bucket key (collision-resistant enough).
        let key_prefix = &token[..token.len().min(16)];
        return format!("key:{key_prefix}");
    }
    // Bearer/session token — single shared bucket.
    "bearer".to_string()
}

/// Middleware that enforces global and per-identity rate limits.
/// Adds `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and `X-RateLimit-Reset` response headers.
pub async fn rate_limit(request: Request<Body>, next: Next, limiter: Arc<RateLimiter>) -> Response {
    let identity = extract_rate_limit_identity(&request);
    let (allowed, limit, remaining, reset) = limiter.try_acquire_for(&identity);

    if !allowed {
        let path = request.uri().path().to_string();
        crate::audit::audit(
            crate::audit::AuditEvent::RateLimited,
            &format!("rate limit exceeded for identity: {identity}"),
            &identity,
            &path,
        );
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            format!("rate limit exceeded ({remaining} remaining in window)"),
        )
            .into_response();
        let headers = resp.headers_mut();
        let reset_str = reset.to_string();
        if let Ok(v) = reset_str.parse() {
            headers.insert(header::RETRY_AFTER, v);
        }
        if let Ok(v) = limit.to_string().parse() {
            headers.insert(X_RATELIMIT_LIMIT, v);
        }
        headers.insert(
            X_RATELIMIT_REMAINING,
            "0".parse().expect("valid header value"),
        );
        if let Ok(v) = reset_str.parse() {
            headers.insert(X_RATELIMIT_RESET, v);
        }
        return resp;
    }

    let mut response = next.run(request).await;

    // Add rate limit headers to successful responses.
    if let Ok(v) = limit.to_string().parse() {
        response.headers_mut().insert(X_RATELIMIT_LIMIT, v);
    }
    if let Ok(v) = remaining.to_string().parse() {
        response.headers_mut().insert(X_RATELIMIT_REMAINING, v);
    }
    if let Ok(v) = reset.to_string().parse() {
        response.headers_mut().insert(X_RATELIMIT_RESET, v);
    }

    response
}

// ---------------------------------------------------------------------------
// CORS middleware
// ---------------------------------------------------------------------------

/// Build CORS response headers for a preflight OPTIONS request.
pub fn cors_preflight_response(allowed_origins: &[String], origin: &str) -> Response {
    if !is_origin_allowed(allowed_origins, origin) {
        return StatusCode::FORBIDDEN.into_response();
    }

    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)
        .header(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, POST, PUT, PATCH, DELETE, OPTIONS",
        )
        .header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Authorization, Content-Type, X-Pairing-Code, X-Request-Id",
        )
        .header(header::ACCESS_CONTROL_MAX_AGE, "3600")
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Middleware that adds CORS headers to responses.
pub async fn cors_middleware(
    request: Request<Body>,
    next: Next,
    allowed_origins: Vec<String>,
) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Handle preflight.
    if request.method() == Method::OPTIONS {
        if let Some(ref origin) = origin {
            return cors_preflight_response(&allowed_origins, origin);
        }
        return StatusCode::NO_CONTENT.into_response();
    }

    let mut response = next.run(request).await;

    // Add CORS headers to the response.
    if let Some(ref origin) = origin {
        if is_origin_allowed(&allowed_origins, origin) {
            let headers = response.headers_mut();
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                origin.parse().unwrap_or_else(|_| "*".parse().unwrap()),
            );
        }
    }

    response
}

fn is_origin_allowed(allowed: &[String], origin: &str) -> bool {
    if allowed.is_empty() {
        return false;
    }
    allowed.iter().any(|a| a == "*" || a == origin)
}

// ---------------------------------------------------------------------------
// HSTS middleware
// ---------------------------------------------------------------------------

/// Middleware that adds `Strict-Transport-Security` headers to all responses.
/// Should only be applied when TLS is active.
pub async fn hsts_middleware(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    // max-age=31536000 (1 year), includeSubDomains as per OWASP recommendation.
    response.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=31536000; includeSubDomains"
            .parse()
            .expect("valid HSTS header value"),
    );
    response
}

// ---------------------------------------------------------------------------
// Correlation ID middleware
// ---------------------------------------------------------------------------

/// Header name for request correlation IDs.
pub const X_REQUEST_ID: &str = "x-request-id";

/// Middleware that propagates or generates a correlation ID (`X-Request-Id`).
///
/// If the incoming request contains an `X-Request-Id` header, that value is used.
/// Otherwise a new UUID v4 is generated. The ID is:
/// 1. Added to the current tracing span as `request_id`.
/// 2. Returned in the `X-Request-Id` response header.
pub async fn correlation_id(request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let span = tracing::info_span!("request", request_id = %request_id);
    let _guard = span.enter();

    let mut response = next.run(request).await;

    if let Ok(header_value) = request_id.parse() {
        response.headers_mut().insert(X_REQUEST_ID, header_value);
    }

    response
}

// ---------------------------------------------------------------------------
// Request metrics middleware
// ---------------------------------------------------------------------------

/// Middleware that records request count, status, and latency as Prometheus metrics.
pub async fn request_metrics(request: Request<Body>, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let start = Instant::now();
    crate::gateway_metrics::inc_active_connections();

    let response = next.run(request).await;

    crate::gateway_metrics::dec_active_connections();
    let status = response.status().as_u16();
    let duration = start.elapsed().as_secs_f64();
    crate::gateway_metrics::record_request(&method, &path, status, duration);

    response
}

// ---------------------------------------------------------------------------
// Graceful shutdown signal handler
// ---------------------------------------------------------------------------

/// Wait for a shutdown signal (SIGTERM/SIGINT on Unix, Ctrl+C on all platforms).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl-c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received, draining connections...");
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rate limiter tests ---

    #[test]
    fn rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new(5, 60);
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn rate_limiter_unlimited_always_allows() {
        let limiter = RateLimiter::new(0, 60);
        for _ in 0..1000 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn rate_limiter_remaining_decreases() {
        let limiter = RateLimiter::new(10, 60);
        assert_eq!(limiter.remaining(), 10);
        limiter.try_acquire();
        assert_eq!(limiter.remaining(), 9);
    }

    #[test]
    fn rate_limiter_current_count() {
        let limiter = RateLimiter::new(10, 60);
        assert_eq!(limiter.current_count(), 0);
        limiter.try_acquire();
        limiter.try_acquire();
        assert_eq!(limiter.current_count(), 2);
    }

    // --- CORS tests ---

    #[test]
    fn is_origin_allowed_wildcard() {
        assert!(is_origin_allowed(&["*".to_string()], "https://example.com"));
    }

    #[test]
    fn is_origin_allowed_specific() {
        let origins = vec!["https://example.com".to_string()];
        assert!(is_origin_allowed(&origins, "https://example.com"));
        assert!(!is_origin_allowed(&origins, "https://evil.com"));
    }

    #[test]
    fn is_origin_allowed_empty_denies() {
        assert!(!is_origin_allowed(&[], "https://example.com"));
    }

    // --- Per-identity rate limiter tests ---

    #[test]
    fn per_identity_isolation_key_a_at_limit_does_not_block_key_b() {
        let limiter = RateLimiter::new(100, 60).with_per_identity(3);
        // Exhaust key_a's per-identity limit.
        for _ in 0..3 {
            let (allowed, _, _, _) = limiter.try_acquire_for("key:az_key_a_12345");
            assert!(allowed);
        }
        let (allowed, _, _, _) = limiter.try_acquire_for("key:az_key_a_12345");
        assert!(!allowed, "key_a should be rate limited");

        // key_b should still work.
        let (allowed, limit, remaining, _) = limiter.try_acquire_for("key:az_key_b_99999");
        assert!(allowed, "key_b should not be affected by key_a's limit");
        assert_eq!(limit, 3);
        assert_eq!(remaining, 2);
    }

    #[test]
    fn global_limit_applies_across_all_identities() {
        let limiter = RateLimiter::new(5, 60).with_per_identity(100);
        // Global limit is 5 — use different identities but exhaust global.
        for i in 0..5 {
            let (allowed, _, _, _) = limiter.try_acquire_for(&format!("key:identity_{i}"));
            assert!(allowed);
        }
        // 6th request from a new identity should fail at global level.
        let (allowed, _, _, _) = limiter.try_acquire_for("key:identity_new");
        assert!(
            !allowed,
            "global limit should block even though per-identity limit is high"
        );
    }

    #[test]
    fn try_acquire_for_returns_correct_headers_no_per_identity() {
        let limiter = RateLimiter::new(10, 60);
        let (allowed, limit, remaining, reset) = limiter.try_acquire_for("_anonymous");
        assert!(allowed);
        assert_eq!(limit, 10);
        assert_eq!(remaining, 9);
        assert!(reset <= 60);
    }

    #[test]
    fn try_acquire_for_returns_correct_headers_with_per_identity() {
        let limiter = RateLimiter::new(100, 60).with_per_identity(5);
        let (allowed, limit, remaining, _) = limiter.try_acquire_for("key:test");
        assert!(allowed);
        assert_eq!(limit, 5);
        assert_eq!(remaining, 4);
    }

    #[test]
    fn gc_expired_identities_removes_stale_buckets() {
        let limiter = RateLimiter::new(100, 0).with_per_identity(10);
        // Window of 0 seconds means entries expire immediately (2*0 = 0s idle).
        let _ = limiter.try_acquire_for("key:stale_1");
        let _ = limiter.try_acquire_for("key:stale_2");
        assert_eq!(limiter.identities.len(), 2);

        // GC should remove them since window_secs=0 → expired immediately.
        let removed = limiter.gc_expired_identities();
        assert_eq!(removed, 2);
        assert_eq!(limiter.identities.len(), 0);
    }

    #[test]
    fn anonymous_identity_from_no_auth_header() {
        use axum::body::Body;
        use axum::http::Request;

        let req = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .expect("valid request");
        assert_eq!(extract_rate_limit_identity(&req), "_anonymous");
    }

    #[test]
    fn api_key_identity_from_auth_header() {
        use axum::body::Body;
        use axum::http::Request;

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer az_test_key_123456789")
            .body(Body::empty())
            .expect("valid request");
        assert_eq!(extract_rate_limit_identity(&req), "key:az_test_key_1234");
    }

    #[test]
    fn bearer_identity_from_non_api_key_auth() {
        use axum::body::Body;
        use axum::http::Request;

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer some-session-token")
            .body(Body::empty())
            .expect("valid request");
        assert_eq!(extract_rate_limit_identity(&req), "bearer");
    }

    #[test]
    fn non_bearer_auth_treated_as_anonymous() {
        use axum::body::Body;
        use axum::http::Request;

        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .expect("valid request");
        assert_eq!(extract_rate_limit_identity(&req), "_anonymous");
    }

    #[tokio::test]
    async fn rate_limit_middleware_adds_headers_on_success() {
        use axum::body::Body;
        use axum::http::Request;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        let limiter = Arc::new(RateLimiter::new(10, 60));
        let lim = limiter.clone();
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn(move |req: Request<Body>, next: Next| {
                let l = lim.clone();
                async move { rate_limit(req, next, l).await }
            }));

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .expect("valid request");
        let resp = app.oneshot(req).await.expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(X_RATELIMIT_LIMIT).is_some());
        assert!(resp.headers().get(X_RATELIMIT_REMAINING).is_some());
        assert!(resp.headers().get(X_RATELIMIT_RESET).is_some());

        let limit_val = resp
            .headers()
            .get(X_RATELIMIT_LIMIT)
            .expect("limit header")
            .to_str()
            .expect("valid str");
        assert_eq!(limit_val, "10");
    }

    #[tokio::test]
    async fn rate_limit_middleware_returns_429_with_headers() {
        use axum::body::Body;
        use axum::http::Request;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        let limiter = Arc::new(RateLimiter::new(2, 60));
        let app = {
            let lim = limiter.clone();
            Router::new()
                .route("/", get(|| async { "ok" }))
                .layer(from_fn(move |req: Request<Body>, next: Next| {
                    let l = lim.clone();
                    async move { rate_limit(req, next, l).await }
                }))
        };

        // Exhaust the limit.
        for _ in 0..2 {
            let req = Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("valid request");
            let resp = app.clone().oneshot(req).await.expect("request ok");
            assert_eq!(resp.status(), StatusCode::OK);
        }

        // 3rd request should be 429.
        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .expect("valid request");
        let resp = app.oneshot(req).await.expect("request ok");
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().get(X_RATELIMIT_LIMIT).is_some());
        assert_eq!(
            resp.headers()
                .get(X_RATELIMIT_REMAINING)
                .expect("remaining header")
                .to_str()
                .expect("str"),
            "0"
        );
        assert!(resp.headers().get(header::RETRY_AFTER).is_some());
    }

    // --- Config defaults ---

    #[test]
    fn middleware_config_defaults() {
        let config = MiddlewareConfig::default();
        assert_eq!(config.max_body_bytes, 1024 * 1024);
        assert_eq!(config.rate_limit_max, 600);
        assert_eq!(config.rate_limit_per_identity, 0);
        assert_eq!(config.rate_limit_window_secs, 60);
        assert!(config.cors_allowed_origins.is_empty());
        assert!(!config.tls_enabled);
    }

    // --- Correlation ID tests ---

    #[tokio::test]
    async fn correlation_id_generates_uuid_when_absent() {
        use axum::body::Body;
        use axum::http::Request;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn(correlation_id));

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .expect("valid request");

        let response = app.oneshot(req).await.expect("request should succeed");
        let id = response
            .headers()
            .get(X_REQUEST_ID)
            .expect("X-Request-Id should be present");
        let id_str = id.to_str().expect("valid header value");
        // Should be a valid UUID v4.
        assert!(
            uuid::Uuid::parse_str(id_str).is_ok(),
            "expected UUID, got: {id_str}"
        );
    }

    #[tokio::test]
    async fn correlation_id_propagates_existing_header() {
        use axum::body::Body;
        use axum::http::Request;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn(correlation_id));

        let req = Request::builder()
            .uri("/")
            .header(X_REQUEST_ID, "my-custom-id-123")
            .body(Body::empty())
            .expect("valid request");

        let response = app.oneshot(req).await.expect("request should succeed");
        let id = response
            .headers()
            .get(X_REQUEST_ID)
            .expect("X-Request-Id should be present");
        assert_eq!(id.to_str().expect("valid value"), "my-custom-id-123");
    }

    // --- HSTS tests ---

    #[tokio::test]
    async fn hsts_middleware_adds_header() {
        use axum::body::Body;
        use axum::http::Request;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn(hsts_middleware));

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .expect("valid request");

        let response = app.oneshot(req).await.expect("request should succeed");
        let hsts = response
            .headers()
            .get(header::STRICT_TRANSPORT_SECURITY)
            .expect("HSTS header should be present");
        let value = hsts.to_str().expect("valid header value");
        assert!(value.contains("max-age=31536000"));
        assert!(value.contains("includeSubDomains"));
    }
}
