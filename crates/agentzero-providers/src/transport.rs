use anyhow::anyhow;
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub(crate) const MAX_ATTEMPTS: usize = 3;
const BASE_BACKOFF_MS: u64 = 100;

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

pub(crate) fn map_status_error(status: StatusCode, body: &str) -> anyhow::Error {
    let message = extract_error_message(body).unwrap_or_else(|| "no error message".to_string());
    match status {
        StatusCode::UNAUTHORIZED => anyhow!("provider auth error (401): {message}"),
        StatusCode::TOO_MANY_REQUESTS => anyhow!("provider rate limited (429): {message}"),
        s if s.is_server_error() => anyhow!("provider server error ({}): {message}", s.as_u16()),
        _ => anyhow!("provider request failed ({}): {message}", status.as_u16()),
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
}
