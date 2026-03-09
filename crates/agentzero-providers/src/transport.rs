use anyhow::anyhow;
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub(crate) const MAX_ATTEMPTS: usize = 3;
const BASE_BACKOFF_MS: u64 = 100;

// ---------------------------------------------------------------------------
// Transport config
// ---------------------------------------------------------------------------

/// Per-provider transport configuration. Loaded from `[provider.transport]`
/// in `agentzero.toml`.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Request timeout in milliseconds. Default: 30 000 (30s).
    pub timeout_ms: u64,
    /// Maximum retry attempts for transient failures. Default: 3.
    pub max_retries: usize,
    /// Circuit breaker: consecutive failures before opening. Default: 5.
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker: how long the circuit stays open before a probe. Default: 30s.
    pub circuit_breaker_reset_ms: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_retries: MAX_ATTEMPTS,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Circuit breaker
// ---------------------------------------------------------------------------

/// Atomic circuit breaker states.
const CB_CLOSED: u8 = 0;
const CB_OPEN: u8 = 1;
const CB_HALF_OPEN: u8 = 2;

/// A lightweight circuit breaker that prevents cascading failures when a
/// provider is unreachable or consistently returning errors.
///
/// State machine: Closed → Open (after N failures) → Half-Open (after
/// reset timeout) → Closed (on success) or Open (on failure).
pub struct CircuitBreaker {
    state: AtomicU8,
    consecutive_failures: AtomicU64,
    threshold: u32,
    reset_duration: Duration,
    last_failure: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_duration: Duration) -> Self {
        Self {
            state: AtomicU8::new(CB_CLOSED),
            consecutive_failures: AtomicU64::new(0),
            threshold,
            reset_duration,
            last_failure: Mutex::new(None),
        }
    }

    pub fn from_config(config: &TransportConfig) -> Self {
        Self::new(
            config.circuit_breaker_threshold,
            Duration::from_millis(config.circuit_breaker_reset_ms),
        )
    }

    /// Check if a request is allowed. Returns `Ok(())` if the circuit is
    /// closed or half-open (probe). Returns an error if the circuit is open.
    pub async fn check(&self) -> anyhow::Result<()> {
        let state = self.state.load(Ordering::SeqCst);
        match state {
            CB_CLOSED => Ok(()),
            CB_HALF_OPEN => {
                info!("circuit breaker half-open, allowing probe request");
                Ok(())
            }
            CB_OPEN => {
                let last = self.last_failure.lock().await;
                if let Some(instant) = *last {
                    if instant.elapsed() >= self.reset_duration {
                        drop(last);
                        self.state.store(CB_HALF_OPEN, Ordering::SeqCst);
                        info!("circuit breaker transitioning open → half-open");
                        return Ok(());
                    }
                }
                let remaining = last
                    .map(|i| self.reset_duration.saturating_sub(i.elapsed()))
                    .unwrap_or(self.reset_duration);
                Err(anyhow!(
                    "circuit breaker open: provider unavailable (retry in {:.1}s)",
                    remaining.as_secs_f64()
                ))
            }
            _ => Ok(()),
        }
    }

    /// Record a successful request. Resets the breaker to closed.
    pub fn record_success(&self) {
        let previous = self.state.swap(CB_CLOSED, Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);
        if previous != CB_CLOSED {
            info!("circuit breaker closed after successful request");
        }
    }

    /// Record a failed request. If consecutive failures exceed the threshold,
    /// the circuit opens.
    pub async fn record_failure(&self) {
        let count = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure.lock().await = Some(Instant::now());
        if count >= self.threshold as u64 {
            let previous = self.state.swap(CB_OPEN, Ordering::SeqCst);
            if previous != CB_OPEN {
                warn!(
                    consecutive_failures = count,
                    "circuit breaker opened after {} consecutive failures", count
                );
            }
        }
    }

    /// Current state as a string, useful for health checks.
    pub fn state_label(&self) -> &'static str {
        match self.state.load(Ordering::SeqCst) {
            CB_CLOSED => "closed",
            CB_OPEN => "open",
            CB_HALF_OPEN => "half-open",
            _ => "unknown",
        }
    }

    /// Current consecutive failure count.
    pub fn failure_count(&self) -> u64 {
        self.consecutive_failures.load(Ordering::SeqCst)
    }
}

/// Snapshot of circuit breaker state for monitoring and health checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitBreakerStatus {
    pub state: &'static str,
    pub failure_count: u64,
}

impl CircuitBreaker {
    /// Take a snapshot of the current circuit breaker state.
    pub fn status(&self) -> CircuitBreakerStatus {
        CircuitBreakerStatus {
            state: self.state_label(),
            failure_count: self.failure_count(),
        }
    }
}

// ---------------------------------------------------------------------------
// Transport error types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportErrorKind {
    Timeout,
    Connect,
    Request,
    Body,
    Other,
}

#[derive(Debug)]
pub(crate) struct TransportError {
    pub kind: TransportErrorKind,
    pub message: String,
}

impl TransportError {
    pub fn new(kind: TransportErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

pub(crate) struct TransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

// ---------------------------------------------------------------------------
// reqwest helpers
// ---------------------------------------------------------------------------

pub(crate) fn map_reqwest_error(err: reqwest::Error) -> TransportError {
    let kind = if err.is_timeout() {
        TransportErrorKind::Timeout
    } else if err.is_connect() {
        TransportErrorKind::Connect
    } else if err.is_request() {
        TransportErrorKind::Request
    } else if err.is_body() {
        TransportErrorKind::Body
    } else {
        TransportErrorKind::Other
    };
    TransportError::new(kind, err.to_string())
}

pub(crate) fn read_response_body(err: reqwest::Error) -> TransportError {
    TransportError::new(
        if err.is_body() {
            TransportErrorKind::Body
        } else {
            TransportErrorKind::Other
        },
        format!("failed reading response body: {err}"),
    )
}

pub(crate) fn collect_headers(response: &reqwest::Response) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(parsed) = value.to_str() {
            headers.insert(name.as_str().to_ascii_lowercase(), parsed.to_string());
        }
    }
    headers
}

// ---------------------------------------------------------------------------
// Retry helpers
// ---------------------------------------------------------------------------

pub(crate) fn jittered_backoff(attempt_index: usize) -> Duration {
    let exp = BASE_BACKOFF_MS.saturating_mul(1_u64 << attempt_index.min(10));
    let jitter = ((attempt_index as u64).saturating_mul(37)) % 53;
    Duration::from_millis(exp.saturating_add(jitter))
}

pub(crate) fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

pub(crate) fn should_retry_transport(kind: TransportErrorKind) -> bool {
    matches!(
        kind,
        TransportErrorKind::Timeout
            | TransportErrorKind::Connect
            | TransportErrorKind::Request
            | TransportErrorKind::Body
    )
}

pub(crate) fn parse_retry_after(headers: &HashMap<String, String>) -> Option<Duration> {
    let retry_after = headers.get("retry-after")?.trim();
    let seconds = retry_after.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

pub(crate) fn extract_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

/// Extract the Anthropic/OpenAI error type field (e.g. "invalid_request_error",
/// "authentication_error", "overloaded_error").
pub(crate) fn extract_error_type(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub(crate) fn map_status_error(status: StatusCode, body: &str) -> anyhow::Error {
    let message = extract_error_message(body).unwrap_or_else(|| "no error message".to_string());
    let error_type = extract_error_type(body);
    let type_suffix = error_type
        .as_deref()
        .map(|t| format!(" [{t}]"))
        .unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED => {
            anyhow!("provider auth error (401){type_suffix}: {message}")
        }
        StatusCode::FORBIDDEN => {
            anyhow!("provider forbidden (403){type_suffix}: {message}")
        }
        StatusCode::NOT_FOUND => {
            anyhow!("provider endpoint not found (404){type_suffix}: {message}")
        }
        StatusCode::TOO_MANY_REQUESTS => {
            anyhow!("provider rate limited (429){type_suffix}: {message}")
        }
        s if s.is_server_error() => {
            anyhow!(
                "provider server error ({}){type_suffix}: {message}",
                s.as_u16()
            )
        }
        _ => anyhow!(
            "provider request failed ({}){type_suffix}: {message}",
            status.as_u16()
        ),
    }
}

// ---------------------------------------------------------------------------
// Request/response logging helpers
// ---------------------------------------------------------------------------

pub(crate) fn log_request(provider: &str, url: &str, model: &str) {
    info!(
        provider = provider,
        url = url,
        model = model,
        "sending provider request"
    );
}

pub(crate) fn log_response(provider: &str, status: u16, body_len: usize, elapsed: Duration) {
    info!(
        provider = provider,
        status = status,
        body_bytes = body_len,
        elapsed_ms = elapsed.as_millis() as u64,
        "received provider response"
    );
}

pub(crate) fn log_retry(provider: &str, attempt: usize, reason: &str) {
    warn!(
        provider = provider,
        attempt = attempt + 1,
        reason = reason,
        "retrying provider request"
    );
}

// ---------------------------------------------------------------------------
// Health check probing
// ---------------------------------------------------------------------------

/// Result of probing a provider's health endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthProbeResult {
    /// Provider responded with a successful status.
    Healthy { latency_ms: u64 },
    /// Provider responded but with an error status.
    Unhealthy { status: u16, message: String },
    /// Provider did not respond (timeout, connection error, etc.).
    Unreachable { reason: String },
}

/// Probe a provider's health by sending a lightweight request to its base URL.
/// For OpenAI-compatible providers, this tries `GET /v1/models`.
/// For Anthropic, this tries a minimal messages request that returns quickly.
pub async fn health_probe(base_url: &str, api_key: &str, provider_kind: &str) -> HealthProbeResult {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let url = if provider_kind == "anthropic" {
        format!("{}/v1/messages", base_url.trim_end_matches('/'))
    } else {
        format!("{}/v1/models", base_url.trim_end_matches('/'))
    };

    let start = Instant::now();
    let request = if provider_kind == "anthropic" {
        let req = client.post(&url);
        // OAuth tokens (`sk-ant-oat` prefix or non `sk-ant-`) use Bearer auth.
        let req = if api_key.starts_with("sk-ant-oat")
            || (!api_key.is_empty() && !api_key.starts_with("sk-ant-"))
        {
            req.bearer_auth(api_key)
        } else {
            req.header("x-api-key", api_key)
        };
        req.header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(r#"{"model":"claude-haiku-4-20250414","max_tokens":1,"messages":[{"role":"user","content":"ping"}]}"#)
    } else {
        let mut req = client.get(&url);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        req
    };

    match request.send().await {
        Ok(response) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let status = response.status().as_u16();
            if response.status().is_success() || status == 401 {
                // 401 means the endpoint exists but the key is wrong/missing.
                // We treat this as "healthy" from a connectivity standpoint.
                HealthProbeResult::Healthy { latency_ms }
            } else {
                let body = response.text().await.unwrap_or_default();
                let message = extract_error_message(&body)
                    .unwrap_or_else(|| body.chars().take(200).collect());
                HealthProbeResult::Unhealthy { status, message }
            }
        }
        Err(e) => HealthProbeResult::Unreachable {
            reason: format!("{e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_produces_specific_categories() {
        let auth = map_status_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"bad key"}}"#,
        );
        assert!(auth.to_string().contains("auth error (401)"));

        let rate = map_status_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"slow down"}}"#,
        );
        assert!(rate.to_string().contains("rate limited (429)"));

        let server = map_status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"boom"}}"#,
        );
        assert!(server.to_string().contains("server error (500)"));
    }

    #[test]
    fn status_mapping_includes_error_type_when_present() {
        let err = map_status_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"type":"invalid_request_error","message":"bad input"}}"#,
        );
        let msg = err.to_string();
        assert!(msg.contains("[invalid_request_error]"));
        assert!(msg.contains("bad input"));
    }

    #[test]
    fn status_mapping_forbidden_and_not_found() {
        let forbidden = map_status_error(
            StatusCode::FORBIDDEN,
            r#"{"error":{"message":"access denied"}}"#,
        );
        assert!(forbidden.to_string().contains("forbidden (403)"));

        let not_found = map_status_error(
            StatusCode::NOT_FOUND,
            r#"{"error":{"message":"model not found"}}"#,
        );
        assert!(not_found.to_string().contains("not found (404)"));
    }

    #[test]
    fn retry_policy_only_retries_transient_statuses() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry_status(StatusCode::BAD_GATEWAY));
        assert!(!should_retry_status(StatusCode::UNAUTHORIZED));
        assert!(!should_retry_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn backoff_increases_with_attempts() {
        let first = jittered_backoff(0);
        let second = jittered_backoff(1);
        let third = jittered_backoff(2);
        assert!(second > first);
        assert!(third > second);
    }

    #[test]
    fn extract_error_type_parses_anthropic_format() {
        let body = r#"{"error":{"type":"overloaded_error","message":"overloaded"}}"#;
        assert_eq!(
            extract_error_type(body),
            Some("overloaded_error".to_string())
        );
    }

    #[test]
    fn extract_error_type_returns_none_for_missing_type() {
        let body = r#"{"error":{"message":"no type field"}}"#;
        assert_eq!(extract_error_type(body), None);
    }

    // --- Circuit breaker tests ---

    #[tokio::test]
    async fn circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.state_label(), "closed");
        assert!(cb.check().await.is_ok());
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold_failures() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));

        cb.record_failure().await;
        assert_eq!(cb.state_label(), "closed");
        cb.record_failure().await;
        assert_eq!(cb.state_label(), "closed");
        cb.record_failure().await;
        assert_eq!(cb.state_label(), "open");

        let result = cb.check().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("circuit breaker open"));
    }

    #[tokio::test]
    async fn circuit_breaker_resets_on_success() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(30));

        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.state_label(), "open");

        cb.record_success();
        assert_eq!(cb.state_label(), "closed");
        assert_eq!(cb.failure_count(), 0);
        assert!(cb.check().await.is_ok());
    }

    #[tokio::test]
    async fn circuit_breaker_transitions_to_half_open_after_reset() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));

        cb.record_failure().await;
        assert_eq!(cb.state_label(), "open");

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(cb.check().await.is_ok());
        assert_eq!(cb.state_label(), "half-open");
    }

    #[tokio::test]
    async fn circuit_breaker_half_open_closes_on_success() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));

        cb.record_failure().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.check().await.expect("should transition to half-open");

        cb.record_success();
        assert_eq!(cb.state_label(), "closed");
    }

    #[tokio::test]
    async fn circuit_breaker_half_open_reopens_on_failure() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));

        cb.record_failure().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        cb.check().await.expect("should transition to half-open");
        assert_eq!(cb.state_label(), "half-open");

        cb.record_failure().await;
        assert_eq!(cb.state_label(), "open");
    }

    #[test]
    fn transport_config_default_values() {
        let config = TransportConfig::default();
        assert_eq!(config.timeout_ms, 30_000);
        assert_eq!(config.max_retries, MAX_ATTEMPTS);
        assert_eq!(config.circuit_breaker_threshold, 5);
        assert_eq!(config.circuit_breaker_reset_ms, 30_000);
    }

    // --- Health probe ---

    #[tokio::test]
    async fn health_probe_unreachable_for_invalid_host() {
        let result = health_probe("http://192.0.2.1:1", "", "openai").await;
        assert!(matches!(result, HealthProbeResult::Unreachable { .. }));
    }

    #[test]
    fn health_probe_result_variants() {
        let healthy = HealthProbeResult::Healthy { latency_ms: 42 };
        assert_eq!(healthy, HealthProbeResult::Healthy { latency_ms: 42 });

        let unhealthy = HealthProbeResult::Unhealthy {
            status: 500,
            message: "internal error".to_string(),
        };
        assert!(matches!(
            unhealthy,
            HealthProbeResult::Unhealthy { status: 500, .. }
        ));
    }

    // --- Circuit breaker status snapshot ---

    #[tokio::test]
    async fn circuit_breaker_status_reflects_state() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(30));
        let status = cb.status();
        assert_eq!(status.state, "closed");
        assert_eq!(status.failure_count, 0);

        cb.record_failure().await;
        let status = cb.status();
        assert_eq!(status.state, "closed");
        assert_eq!(status.failure_count, 1);

        cb.record_failure().await;
        let status = cb.status();
        assert_eq!(status.state, "open");
        assert_eq!(status.failure_count, 2);
    }

    #[tokio::test]
    async fn circuit_breaker_status_after_recovery() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        cb.record_failure().await;
        assert_eq!(cb.status().state, "open");

        cb.record_success();
        let status = cb.status();
        assert_eq!(status.state, "closed");
        assert_eq!(status.failure_count, 0);
    }

    #[test]
    fn state_label_correct_for_all_states() {
        let cb = CircuitBreaker::new(100, Duration::from_secs(30));
        assert_eq!(cb.state_label(), "closed");

        // Manually set to open via atomic.
        cb.state.store(CB_OPEN, Ordering::SeqCst);
        assert_eq!(cb.state_label(), "open");

        cb.state.store(CB_HALF_OPEN, Ordering::SeqCst);
        assert_eq!(cb.state_label(), "half-open");

        // Unknown state value.
        cb.state.store(255, Ordering::SeqCst);
        assert_eq!(cb.state_label(), "unknown");
    }

    #[test]
    fn log_helpers_accept_expected_field_types() {
        // Verify the logging helpers don't panic with representative values.
        log_request(
            "test-provider",
            "http://localhost/v1/messages",
            "test-model",
        );
        log_response("test-provider", 200, 1024, Duration::from_millis(42));
        log_retry("test-provider", 0, "rate limited");
    }
}
