//! Noise Protocol handshake HTTP handlers for the gateway.
//!
//! Implements a 2-round-trip XX handshake over HTTP POST:
//!   1. Client POSTs `→ e` message, server responds with `← e ee s es`
//!   2. Client POSTs `→ s se` message, server responds with session token
//!
//! After the handshake, the client uses the `X-Noise-Session` header to
//! send encrypted requests through the noise middleware.

use crate::privacy_state::NoiseSessionStore;
use crate::state::GatewayState;
use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state for in-progress handshakes, keyed by a client-chosen handshake ID.
pub(crate) type HandshakeMap = Arc<dashmap::DashMap<String, Arc<Mutex<NoiseHandshaker>>>>;

/// Request body for step 1 of the Noise handshake.
#[derive(Debug, Deserialize)]
pub(crate) struct HandshakeStep1Request {
    /// Client-chosen unique handshake identifier.
    pub handshake_id: String,
    /// Base64-encoded first handshake message (→ e).
    pub message: String,
}

/// Response body for step 1 of the Noise handshake.
#[derive(Debug, Serialize)]
pub(crate) struct HandshakeStep1Response {
    /// Base64-encoded server handshake message (← e ee s es).
    pub message: String,
}

/// Request body for step 2 of the Noise handshake.
#[derive(Debug, Deserialize)]
pub(crate) struct HandshakeStep2Request {
    /// Same handshake identifier from step 1.
    pub handshake_id: String,
    /// Base64-encoded final handshake message (→ s se).
    pub message: String,
}

/// Response body for step 2 (handshake complete).
#[derive(Debug, Serialize)]
pub(crate) struct HandshakeStep2Response {
    /// Hex-encoded session ID for use in `X-Noise-Session` header.
    pub session_id: String,
}

/// Error response for handshake failures.
#[derive(Debug, Serialize)]
#[allow(dead_code)] // Available for structured error responses
pub(crate) struct HandshakeError {
    pub error: String,
}

use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Step 1 handler: receive client's `→ e`, respond with server's `← e ee s es`.
pub(crate) async fn noise_handshake_step1(
    State(state): State<GatewayState>,
    Json(req): Json<HandshakeStep1Request>,
) -> impl IntoResponse {
    let (_sessions, keypair, handshakes) = match privacy_components(&state) {
        Some(c) => c,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "noise encryption not enabled"})),
            )
        }
    };

    // Decode client message
    let client_msg = match STANDARD.decode(&req.message) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("invalid base64: {e}")})),
            )
        }
    };

    // Create server-side responder
    let mut responder = match NoiseHandshaker::new_responder("XX", keypair) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("handshake init failed: {e}")})),
            )
        }
    };

    // Read client's → e
    let mut payload_buf = [0u8; 65535];
    if let Err(e) = responder.read_message(&client_msg, &mut payload_buf) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("handshake step1 read failed: {e}")})),
        );
    }

    // Write server's ← e ee s es
    let mut out_buf = [0u8; 65535];
    let len = match responder.write_message(b"", &mut out_buf) {
        Ok(l) => l,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("handshake step1 write failed: {e}")})),
            )
        }
    };

    // Store the in-progress handshake
    handshakes.insert(req.handshake_id, Arc::new(Mutex::new(responder)));

    let response = HandshakeStep1Response {
        message: STANDARD.encode(&out_buf[..len]),
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

/// Step 2 handler: receive client's `→ s se`, finalize handshake, return session ID.
pub(crate) async fn noise_handshake_step2(
    State(state): State<GatewayState>,
    Json(req): Json<HandshakeStep2Request>,
) -> impl IntoResponse {
    let (sessions, _keypair, handshakes) = match privacy_components(&state) {
        Some(c) => c,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "noise encryption not enabled"})),
            )
        }
    };

    // Retrieve the in-progress handshake
    let handshaker_arc = match handshakes.remove(&req.handshake_id) {
        Some((_, h)) => h,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "handshake not found or expired"})),
            )
        }
    };

    let client_msg = match STANDARD.decode(&req.message) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("invalid base64: {e}")})),
            )
        }
    };

    let mut responder = handshaker_arc.lock().await;

    // Read client's → s se
    let mut payload_buf = [0u8; 65535];
    if let Err(e) = responder.read_message(&client_msg, &mut payload_buf) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("handshake step2 read failed: {e}")})),
        );
    }

    // We need to take ownership of the handshaker to call into_transport
    drop(responder);
    let handshaker = Arc::try_unwrap(handshaker_arc)
        .map_err(|_| "handshake still referenced")
        .map(|m| m.into_inner());

    let handshaker = match handshaker {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("failed to finalize handshake: {e}")})),
            )
        }
    };

    if !handshaker.is_finished() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "handshake not yet complete"})),
        );
    }

    // Transition to transport mode
    let session = match handshaker.into_transport() {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("transport transition failed: {e}")})),
            )
        }
    };

    let session_id_hex = session
        .session_id()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    match sessions.insert(session) {
        Ok(_) => {
            crate::gateway_metrics::record_noise_handshake("success");
            crate::gateway_metrics::set_noise_sessions_active(sessions.len() as f64);
            let response = HandshakeStep2Response {
                session_id: session_id_hex,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
        }
        Err(e) => {
            crate::gateway_metrics::record_noise_handshake("failure");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": format!("session store full: {e}")})),
            )
        }
    }
}

/// Extract privacy components from gateway state. Returns None if privacy is not enabled.
fn privacy_components(
    state: &GatewayState,
) -> Option<(&NoiseSessionStore, &NoiseKeypair, &HandshakeMap)> {
    let sessions = state.noise_sessions.as_deref()?;
    let keypair = state.noise_keypair.as_ref()?;
    let handshakes = state.noise_handshakes.as_ref()?;
    Some((sessions, keypair, handshakes))
}
