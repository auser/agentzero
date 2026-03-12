use axum::{
    body::Body,
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
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
    /// Rate limit: max requests per window. 0 = unlimited. Default: 0.
    pub rate_limit_max: u64,
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
            rate_limit_window_secs: 60,
            cors_allowed_origins: vec![],
            tls_enabled: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Rate limiter (sliding window counter)
// ---------------------------------------------------------------------------

/// Simple atomic rate limiter using a sliding window counter.
/// Designed for single-process use (not distributed).
#[derive(Debug)]
pub struct RateLimiter {
    max_requests: u64,
    window_secs: u64,
    counter: AtomicU64,
    window_start: std::sync::Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            counter: AtomicU64::new(0),
            window_start: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Try to acquire a permit. Returns `true` if allowed, `false` if rate limited.
    pub fn try_acquire(&self) -> bool {
        if self.max_requests == 0 {
            return true; // unlimited
        }

        let mut start = self.window_start.lock().expect("rate limiter lock");
        let elapsed = start.elapsed().as_secs();
        if elapsed >= self.window_secs {
            // Window expired — reset.
            *start = Instant::now();
            self.counter.store(1, Ordering::Relaxed);
            return true;
        }
        drop(start);

        let count = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        count <= self.max_requests
    }

    /// Current request count in the window.
    pub fn current_count(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Remaining requests in the current window.
    pub fn remaining(&self) -> u64 {
        if self.max_requests == 0 {
            return u64::MAX;
        }
        self.max_requests
            .saturating_sub(self.counter.load(Ordering::Relaxed))
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

/// Middleware that enforces a global request rate limit.
pub async fn rate_limit(request: Request<Body>, next: Next, limiter: Arc<RateLimiter>) -> Response {
    if !limiter.try_acquire() {
        let remaining = limiter.remaining();
        let path = request.uri().path().to_string();
        crate::audit::audit(
            crate::audit::AuditEvent::RateLimited,
            "rate limit exceeded",
            "",
            &path,
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "60")],
            format!("rate limit exceeded ({} remaining in window)", remaining),
        )
            .into_response();
    }
    next.run(request).await
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
            "GET, POST, PUT, DELETE, OPTIONS",
        )
        .header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Authorization, Content-Type",
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

    // --- Config defaults ---

    #[test]
    fn middleware_config_defaults() {
        let config = MiddlewareConfig::default();
        assert_eq!(config.max_body_bytes, 1024 * 1024);
        assert_eq!(config.rate_limit_max, 600);
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
