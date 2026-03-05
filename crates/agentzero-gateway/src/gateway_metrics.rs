use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Metric name constants.
pub(crate) const REQUESTS_TOTAL: &str = "agentzero_gateway_requests_total";
pub(crate) const REQUEST_DURATION: &str = "agentzero_gateway_request_duration_seconds";
pub(crate) const ACTIVE_CONNECTIONS: &str = "agentzero_gateway_active_connections";
pub(crate) const WS_CONNECTIONS_TOTAL: &str = "agentzero_gateway_ws_connections_total";
pub(crate) const ERRORS_TOTAL: &str = "agentzero_gateway_errors_total";

/// Build a Prometheus recorder and return the render handle.
pub(crate) fn init_prometheus() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    builder
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

/// Record a completed HTTP request.
pub(crate) fn record_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    let labels = [
        ("method", method.to_string()),
        ("path", path.to_string()),
        ("status", status.to_string()),
    ];
    counter!(REQUESTS_TOTAL, &labels).increment(1);
    histogram!(
        REQUEST_DURATION,
        &[("method", method.to_string()), ("path", path.to_string())]
    )
    .record(duration_secs);
}

/// Record a gateway error by type.
pub(crate) fn record_error(error_type: &str) {
    counter!(ERRORS_TOTAL, "error_type" => error_type.to_string()).increment(1);
}

/// Increment the active connections gauge.
pub(crate) fn inc_active_connections() {
    gauge!(ACTIVE_CONNECTIONS).increment(1.0);
}

/// Decrement the active connections gauge.
pub(crate) fn dec_active_connections() {
    gauge!(ACTIVE_CONNECTIONS).decrement(1.0);
}

/// Record a new WebSocket connection.
pub(crate) fn record_ws_connection() {
    counter!(WS_CONNECTIONS_TOTAL).increment(1);
}

// ---------------------------------------------------------------------------
// Privacy metrics (gated behind `privacy` feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "privacy")]
pub(crate) const NOISE_SESSIONS_ACTIVE: &str = "agentzero_noise_sessions_active";
#[cfg(feature = "privacy")]
pub(crate) const NOISE_HANDSHAKES_TOTAL: &str = "agentzero_noise_handshakes_total";
#[cfg(feature = "privacy")]
pub(crate) const RELAY_MAILBOX_ENVELOPES: &str = "agentzero_relay_mailbox_envelopes";
#[cfg(feature = "privacy")]
pub(crate) const RELAY_SUBMIT_TOTAL: &str = "agentzero_relay_submit_total";
#[cfg(feature = "privacy")]
pub(crate) const KEY_ROTATION_TOTAL: &str = "agentzero_key_rotation_total";
#[cfg(feature = "privacy")]
pub(crate) const PRIVACY_ENCRYPT_DURATION: &str = "agentzero_privacy_encrypt_duration_seconds";

/// Record a Noise Protocol handshake result.
#[cfg(feature = "privacy")]
pub(crate) fn record_noise_handshake(result: &str) {
    counter!(NOISE_HANDSHAKES_TOTAL, "result" => result.to_string()).increment(1);
}

/// Set the active Noise sessions gauge.
#[cfg(feature = "privacy")]
pub(crate) fn set_noise_sessions_active(count: f64) {
    gauge!(NOISE_SESSIONS_ACTIVE).set(count);
}

/// Set the relay mailbox envelope count gauge.
#[cfg(feature = "privacy")]
pub(crate) fn set_relay_envelopes(count: f64) {
    gauge!(RELAY_MAILBOX_ENVELOPES).set(count);
}

/// Record a relay envelope submission.
#[cfg(feature = "privacy")]
pub(crate) fn record_relay_submit() {
    counter!(RELAY_SUBMIT_TOTAL).increment(1);
}

/// Record a key rotation event.
#[cfg(feature = "privacy")]
pub(crate) fn record_key_rotation(epoch: u64) {
    counter!(KEY_ROTATION_TOTAL, "epoch" => epoch.to_string()).increment(1);
}

/// Record encryption/decryption duration.
#[cfg(feature = "privacy")]
pub(crate) fn record_encrypt_duration(duration_secs: f64) {
    histogram!(PRIVACY_ENCRYPT_DURATION).record(duration_secs);
}
