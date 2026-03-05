//! Privacy relay mode for zero-knowledge sealed envelope routing.
//!
//! When relay mode is enabled, the gateway routes sealed envelopes by
//! `routing_id` without reading their content. Metadata headers are stripped
//! and optional timing jitter prevents traffic analysis.

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine as _;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::state::GatewayState;

/// In-memory mailbox for sealed envelopes, keyed by routing_id.
pub struct RelayMailbox {
    mailboxes: DashMap<[u8; 32], VecDeque<StoredEnvelope>>,
    /// Seen nonces for replay protection. Each nonce is tracked along with
    /// its receive timestamp so expired entries can be garbage-collected.
    seen_nonces: DashMap<[u8; 24], u64>,
    max_mailbox_size: usize,
    default_ttl_secs: u32,
}

/// An envelope stored in the relay mailbox with metadata for GC.
struct StoredEnvelope {
    payload: Vec<u8>,
    received_at: u64,
    ttl_secs: u32,
}

impl StoredEnvelope {
    fn is_expired(&self) -> bool {
        if self.ttl_secs == 0 {
            return false;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();
        now > self.received_at.saturating_add(self.ttl_secs as u64)
    }
}

impl RelayMailbox {
    pub fn new(max_mailbox_size: usize, default_ttl_secs: u32) -> Arc<Self> {
        Arc::new(Self {
            mailboxes: DashMap::new(),
            seen_nonces: DashMap::new(),
            max_mailbox_size,
            default_ttl_secs,
        })
    }

    /// Submit an envelope to a routing_id mailbox.
    ///
    /// Returns `Err("duplicate nonce")` if the nonce was already seen
    /// (replay protection).
    fn submit(
        &self,
        routing_id: [u8; 32],
        payload: Vec<u8>,
        nonce: Option<[u8; 24]>,
        ttl_secs: Option<u32>,
    ) -> Result<(), &'static str> {
        // Replay protection: reject duplicate nonces.
        if let Some(n) = nonce {
            if self.seen_nonces.contains_key(&n) {
                return Err("duplicate nonce");
            }
        }

        let mut mailbox = self.mailboxes.entry(routing_id).or_default();
        if mailbox.len() >= self.max_mailbox_size {
            return Err("mailbox full");
        }
        let ttl = ttl_secs.unwrap_or(self.default_ttl_secs);
        let received_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();

        // Record nonce after successful mailbox check.
        if let Some(n) = nonce {
            self.seen_nonces.insert(n, received_at);
        }

        mailbox.push_back(StoredEnvelope {
            payload,
            received_at,
            ttl_secs: ttl,
        });
        Ok(())
    }

    /// Drain all non-expired envelopes for a routing_id.
    fn poll(&self, routing_id: &[u8; 32]) -> Vec<Vec<u8>> {
        let mut entry = match self.mailboxes.get_mut(routing_id) {
            Some(e) => e,
            None => return vec![],
        };
        let mailbox = entry.value_mut();
        let mut result = Vec::new();
        while let Some(env) = mailbox.pop_front() {
            if !env.is_expired() {
                result.push(env.payload);
            }
        }
        result
    }

    /// Garbage-collect expired envelopes and stale nonces from all mailboxes.
    #[allow(dead_code)] // Used by background GC task at runtime
    pub fn gc_expired(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();
        let ttl = self.default_ttl_secs as u64;

        self.mailboxes.retain(|_, mailbox| {
            mailbox.retain(|env| !env.is_expired());
            !mailbox.is_empty()
        });

        // GC seen nonces that are older than default TTL.
        self.seen_nonces
            .retain(|_, received_at| now < received_at.saturating_add(ttl));
    }

    /// Total number of envelopes across all mailboxes.
    pub fn total_envelopes(&self) -> usize {
        self.mailboxes.iter().map(|e| e.value().len()).sum()
    }

    /// Number of active mailboxes.
    #[allow(dead_code)] // Used by monitoring/diagnostics at runtime
    pub fn mailbox_count(&self) -> usize {
        self.mailboxes.len()
    }
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct SubmitRequest {
    /// Hex-encoded routing_id (64 chars).
    routing_id: String,
    /// Base64-encoded sealed envelope payload.
    payload: String,
    /// Optional TTL override in seconds.
    ttl_secs: Option<u32>,
    /// Optional base64-encoded nonce (24 bytes) for replay protection.
    nonce: Option<String>,
}

#[derive(Serialize)]
struct SubmitResponse {
    ok: bool,
}

#[derive(Serialize)]
struct PollResponse {
    /// Base64-encoded envelope payloads.
    envelopes: Vec<String>,
}

/// POST /v1/relay/submit — accept a sealed envelope for relay.
pub(crate) async fn relay_submit(
    State(state): State<GatewayState>,
    Json(req): Json<SubmitRequest>,
) -> Response {
    let mailbox = match state.relay_mailbox {
        Some(ref mb) => mb,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "relay not enabled").into_response(),
    };

    let routing_id = match parse_hex_id(&req.routing_id) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "invalid routing_id hex").into_response(),
    };

    let payload = match base64::engine::general_purpose::STANDARD.decode(&req.payload) {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid base64 payload").into_response(),
    };

    // Parse optional nonce for replay protection.
    let nonce: Option<[u8; 24]> = if let Some(ref nonce_b64) = req.nonce {
        match base64::engine::general_purpose::STANDARD.decode(nonce_b64) {
            Ok(bytes) if bytes.len() == 24 => {
                let mut arr = [0u8; 24];
                arr.copy_from_slice(&bytes);
                Some(arr)
            }
            Ok(_) => return (StatusCode::BAD_REQUEST, "nonce must be 24 bytes").into_response(),
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid base64 nonce").into_response(),
        }
    } else {
        None
    };

    match mailbox.submit(routing_id, payload, nonce, req.ttl_secs) {
        Ok(()) => {
            crate::gateway_metrics::record_relay_submit();
            crate::gateway_metrics::set_relay_envelopes(mailbox.total_envelopes() as f64);
            Json(SubmitResponse { ok: true }).into_response()
        }
        Err("duplicate nonce") => {
            (StatusCode::CONFLICT, "duplicate nonce (replay rejected)").into_response()
        }
        Err(msg) => (StatusCode::TOO_MANY_REQUESTS, msg).into_response(),
    }
}

/// GET /v1/relay/poll/:routing_id — drain mailbox for a routing_id.
pub(crate) async fn relay_poll(
    State(state): State<GatewayState>,
    Path(routing_id_hex): Path<String>,
) -> Response {
    let mailbox = match state.relay_mailbox {
        Some(ref mb) => mb,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "relay not enabled").into_response(),
    };

    let routing_id = match parse_hex_id(&routing_id_hex) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "invalid routing_id hex").into_response(),
    };

    let envelopes: Vec<String> = mailbox
        .poll(&routing_id)
        .into_iter()
        .map(|p| base64::engine::general_purpose::STANDARD.encode(p))
        .collect();

    Json(PollResponse { envelopes }).into_response()
}

fn parse_hex_id(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Metadata stripping middleware
// ---------------------------------------------------------------------------

/// Middleware that strips identifying headers from relay requests.
pub(crate) async fn strip_metadata_headers(
    mut request: axum::extract::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let headers = request.headers_mut();
    headers.remove("x-forwarded-for");
    headers.remove("x-real-ip");
    headers.remove("x-forwarded-host");
    headers.remove("via");
    // Don't strip User-Agent entirely — replace with generic value.
    headers.insert("user-agent", "agentzero-relay/1.0".parse().unwrap());
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_id_valid() {
        let hex = "a".repeat(64);
        let id = parse_hex_id(&hex).expect("should parse");
        assert_eq!(id, [0xaa; 32]);
    }

    #[test]
    fn parse_hex_id_invalid_length() {
        assert!(parse_hex_id("abcd").is_none());
    }

    #[test]
    fn parse_hex_id_invalid_chars() {
        let hex = "g".repeat(64);
        assert!(parse_hex_id(&hex).is_none());
    }

    #[test]
    fn mailbox_submit_and_poll() {
        let mb = RelayMailbox::new(10, 3600);
        let routing_id = [1u8; 32];

        mb.submit(routing_id, b"envelope1".to_vec(), None, None)
            .unwrap();
        mb.submit(routing_id, b"envelope2".to_vec(), None, None)
            .unwrap();
        assert_eq!(mb.total_envelopes(), 2);

        let polled = mb.poll(&routing_id);
        assert_eq!(polled.len(), 2);
        assert_eq!(polled[0], b"envelope1");
        assert_eq!(polled[1], b"envelope2");

        // After poll, mailbox should be empty.
        let polled2 = mb.poll(&routing_id);
        assert!(polled2.is_empty());
    }

    #[test]
    fn mailbox_rejects_when_full() {
        let mb = RelayMailbox::new(1, 3600);
        let routing_id = [2u8; 32];

        mb.submit(routing_id, b"first".to_vec(), None, None)
            .unwrap();
        let err = mb.submit(routing_id, b"second".to_vec(), None, None);
        assert_eq!(err, Err("mailbox full"));
    }

    #[test]
    fn replay_protection_rejects_duplicate_nonce() {
        let mb = RelayMailbox::new(10, 3600);
        let routing_id = [5u8; 32];
        let nonce = [42u8; 24];

        mb.submit(routing_id, b"first".to_vec(), Some(nonce), None)
            .unwrap();
        let err = mb
            .submit(routing_id, b"replay".to_vec(), Some(nonce), None)
            .unwrap_err();
        assert_eq!(err, "duplicate nonce");
    }

    #[test]
    fn replay_protection_allows_different_nonces() {
        let mb = RelayMailbox::new(10, 3600);
        let routing_id = [6u8; 32];

        mb.submit(routing_id, b"a".to_vec(), Some([1u8; 24]), None)
            .unwrap();
        mb.submit(routing_id, b"b".to_vec(), Some([2u8; 24]), None)
            .unwrap();
        assert_eq!(mb.total_envelopes(), 2);
    }

    #[test]
    fn gc_cleans_up_stale_nonces() {
        let mb = RelayMailbox::new(10, 1); // 1-second TTL
        let nonce = [7u8; 24];

        // Insert a nonce with a backdated timestamp.
        mb.seen_nonces.insert(nonce, 0); // epoch = very old
        assert!(mb.seen_nonces.contains_key(&nonce));

        mb.gc_expired();
        assert!(!mb.seen_nonces.contains_key(&nonce));
    }

    #[test]
    fn mailbox_gc_expired_removes_old_envelopes() {
        let mb = RelayMailbox::new(10, 1); // 1-second TTL
        let routing_id = [3u8; 32];

        // Insert with timestamp backdated to be expired.
        {
            let mut mailbox = mb.mailboxes.entry(routing_id).or_default();
            mailbox.push_back(StoredEnvelope {
                payload: b"old".to_vec(),
                received_at: 0, // epoch = very old
                ttl_secs: 1,
            });
        }

        assert_eq!(mb.total_envelopes(), 1);
        mb.gc_expired();
        assert_eq!(mb.total_envelopes(), 0);
        assert_eq!(mb.mailbox_count(), 0);
    }

    #[test]
    fn poll_skips_expired_envelopes() {
        let mb = RelayMailbox::new(10, 3600);
        let routing_id = [4u8; 32];

        {
            let mut mailbox = mb.mailboxes.entry(routing_id).or_default();
            // Expired envelope.
            mailbox.push_back(StoredEnvelope {
                payload: b"expired".to_vec(),
                received_at: 0,
                ttl_secs: 1,
            });
            // Fresh envelope.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            mailbox.push_back(StoredEnvelope {
                payload: b"fresh".to_vec(),
                received_at: now,
                ttl_secs: 3600,
            });
        }

        let polled = mb.poll(&routing_id);
        assert_eq!(polled.len(), 1);
        assert_eq!(polled[0], b"fresh");
    }

    #[test]
    fn poll_empty_mailbox_returns_empty() {
        let mb = RelayMailbox::new(10, 3600);
        let polled = mb.poll(&[99u8; 32]);
        assert!(polled.is_empty());
    }
}
