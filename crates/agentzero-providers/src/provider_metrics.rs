//! Per-provider metrics for request counts, latency, errors, and token usage.
//!
//! Uses the `metrics` crate for lazy metric registration. Metrics are only
//! emitted when a Prometheus recorder is installed (by the gateway).
//!
//! When the `metrics` feature is disabled, all functions are no-ops — call
//! sites in `anthropic.rs`, `openai.rs`, and `pipeline.rs` compile
//! unconditionally without any `#[cfg]` noise.

#[cfg(feature = "metrics")]
use metrics::{counter, histogram};

/// Metric name constants.
pub const PROVIDER_REQUESTS_TOTAL: &str = "agentzero_provider_requests_total";
pub const PROVIDER_REQUEST_DURATION: &str = "agentzero_provider_request_duration_seconds";
pub const PROVIDER_ERRORS_TOTAL: &str = "agentzero_provider_errors_total";
pub const PROVIDER_TOKENS_TOTAL: &str = "agentzero_provider_tokens_total";

/// Record a successful provider request.
#[cfg(feature = "metrics")]
pub fn record_provider_success(provider: &str, model: &str, duration_secs: f64) {
    let labels = [
        ("provider", provider.to_string()),
        ("model", model.to_string()),
        ("status", "success".to_string()),
    ];
    counter!(PROVIDER_REQUESTS_TOTAL, &labels).increment(1);
    histogram!(
        PROVIDER_REQUEST_DURATION,
        &[
            ("provider", provider.to_string()),
            ("model", model.to_string()),
        ]
    )
    .record(duration_secs);
}

/// Record a failed provider request.
#[cfg(feature = "metrics")]
pub fn record_provider_error(provider: &str, model: &str, error_type: &str, duration_secs: f64) {
    let labels = [
        ("provider", provider.to_string()),
        ("model", model.to_string()),
        ("status", "error".to_string()),
    ];
    counter!(PROVIDER_REQUESTS_TOTAL, &labels).increment(1);
    counter!(
        PROVIDER_ERRORS_TOTAL,
        &[
            ("provider", provider.to_string()),
            ("model", model.to_string()),
            ("error_type", error_type.to_string()),
        ]
    )
    .increment(1);
    histogram!(
        PROVIDER_REQUEST_DURATION,
        &[
            ("provider", provider.to_string()),
            ("model", model.to_string()),
        ]
    )
    .record(duration_secs);
}

/// Record token usage from a provider response.
#[cfg(feature = "metrics")]
pub fn record_token_usage(provider: &str, model: &str, input_tokens: u32, output_tokens: u32) {
    if input_tokens > 0 {
        counter!(
            PROVIDER_TOKENS_TOTAL,
            &[
                ("provider", provider.to_string()),
                ("model", model.to_string()),
                ("type", "input".to_string()),
            ]
        )
        .increment(u64::from(input_tokens));
    }
    if output_tokens > 0 {
        counter!(
            PROVIDER_TOKENS_TOTAL,
            &[
                ("provider", provider.to_string()),
                ("model", model.to_string()),
                ("type", "output".to_string()),
            ]
        )
        .increment(u64::from(output_tokens));
    }
}

// ── No-op stubs when `metrics` feature is disabled ───────────────────────────

#[cfg(not(feature = "metrics"))]
#[inline(always)]
pub fn record_provider_success(_provider: &str, _model: &str, _duration_secs: f64) {}

#[cfg(not(feature = "metrics"))]
#[inline(always)]
pub fn record_provider_error(
    _provider: &str,
    _model: &str,
    _error_type: &str,
    _duration_secs: f64,
) {
}

#[cfg(not(feature = "metrics"))]
#[inline(always)]
pub fn record_token_usage(_provider: &str, _model: &str, _input_tokens: u32, _output_tokens: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_success_does_not_panic_without_recorder() {
        record_provider_success("anthropic", "claude-sonnet-4-6", 0.5);
    }

    #[test]
    fn record_error_does_not_panic_without_recorder() {
        record_provider_error("openai", "gpt-4o", "timeout", 2.0);
    }

    #[test]
    fn record_tokens_does_not_panic_without_recorder() {
        record_token_usage("anthropic", "claude-sonnet-4-6", 100, 200);
    }

    #[test]
    fn record_zero_tokens_is_noop() {
        record_token_usage("anthropic", "claude-sonnet-4-6", 0, 0);
    }
}
