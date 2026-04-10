//! Structured audit logging for security-relevant gateway events.
//!
//! All audit events are emitted as structured tracing events at the `INFO` level
//! under the `audit` target. When a JSON tracing subscriber is configured (e.g.,
//! the gateway's default JSONL logger), these events are machine-parseable and
//! queryable via log aggregation tools.
//!
//! # Event types
//!
//! - `auth_failure` — Failed authentication attempt.
//! - `scope_denied` — Authenticated but insufficient scope.
//! - `pair_success` — Successful pairing code exchange.
//! - `pair_failure` — Invalid pairing code attempt.
//! - `api_key_created` — New API key created.
//! - `api_key_revoked` — API key revoked.
//! - `estop` — Emergency stop triggered.
//! - `rate_limited` — Request rejected by rate limiter.

/// Audit event types for structured logging.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AuditEvent {
    AuthFailure,
    ScopeDenied,
    PairSuccess,
    PairFailure,
    ApiKeyCreated,
    ApiKeyRevoked,
    Estop,
    RateLimited,
    AdminAction,
}

impl AuditEvent {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AuthFailure => "auth_failure",
            Self::ScopeDenied => "scope_denied",
            Self::PairSuccess => "pair_success",
            Self::PairFailure => "pair_failure",
            Self::ApiKeyCreated => "api_key_created",
            Self::ApiKeyRevoked => "api_key_revoked",
            Self::Estop => "estop",
            Self::RateLimited => "rate_limited",
            Self::AdminAction => "admin_action",
        }
    }
}

/// Emit a structured audit event. All audit events go to the `audit` tracing target
/// at INFO level with consistent field names for machine parsing.
///
/// # Fields
/// - `audit_event` — Event type string (e.g., "auth_failure").
/// - `reason` — Human-readable reason or detail.
/// - `identity` — Identity string (key_id, "bearer", "paired", or empty).
/// - `path` — Request path (when available).
pub(crate) fn audit(event: AuditEvent, reason: &str, identity: &str, path: &str) {
    tracing::info!(
        target: "audit",
        audit_event = event.as_str(),
        reason = reason,
        identity = identity,
        path = path,
        "audit: {}",
        event.as_str(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_roundtrip_all_variants() {
        // Verify all variants have distinct string representations.
        let events = [
            AuditEvent::AuthFailure,
            AuditEvent::ScopeDenied,
            AuditEvent::PairSuccess,
            AuditEvent::PairFailure,
            AuditEvent::ApiKeyCreated,
            AuditEvent::ApiKeyRevoked,
            AuditEvent::Estop,
            AuditEvent::RateLimited,
        ];
        let strings: Vec<&str> = events.iter().map(|e| e.as_str()).collect();
        // All unique.
        let unique: std::collections::HashSet<&str> = strings.iter().copied().collect();
        assert_eq!(
            strings.len(),
            unique.len(),
            "all event types must be unique"
        );
    }

    #[test]
    fn audit_does_not_panic_without_subscriber() {
        // Tracing macros are no-ops without a subscriber — verify no panic.
        audit(AuditEvent::AuthFailure, "invalid token", "", "/v1/ping");
        audit(
            AuditEvent::ApiKeyCreated,
            "new key",
            "key_abc123",
            "/internal",
        );
    }

    #[test]
    fn audit_event_as_str_returns_snake_case() {
        assert_eq!(AuditEvent::AuthFailure.as_str(), "auth_failure");
        assert_eq!(AuditEvent::ScopeDenied.as_str(), "scope_denied");
        assert_eq!(AuditEvent::RateLimited.as_str(), "rate_limited");
    }
}
