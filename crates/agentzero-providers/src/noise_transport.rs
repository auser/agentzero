//! HTTP-level Noise Protocol transport for encrypted gateway communication.
//!
//! Bridges the transport-agnostic `NoiseClientHandshake` from `agentzero-core`
//! with actual HTTP calls via `reqwest`. Provides:
//! - `perform_noise_handshake()` — executes the full XX handshake over HTTP
//! - `NoiseHttpTransport` — an `HttpTransport` impl that encrypts/decrypts

use crate::openai::{ChatRequest, HttpTransport, ReqwestTransport};
use crate::transport::{TransportError, TransportErrorKind, TransportResponse};
use agentzero_core::privacy::noise_client::{NoiseClientHandshake, NoiseClientSession};
use async_trait::async_trait;
use std::sync::Mutex;

/// Perform the full Noise XX handshake with a gateway over HTTP.
///
/// 1. POST `/v1/noise/handshake/step1` — sends `→ e`
/// 2. Receives server's `← e, ee, s, es` + session ID
/// 3. POST `/v1/noise/handshake/step2` — sends `→ s, se`
/// 4. Returns a `NoiseClientSession` ready for encrypt/decrypt.
pub async fn perform_noise_handshake(gateway_base_url: &str) -> anyhow::Result<NoiseClientSession> {
    let client = reqwest::Client::new();
    let base = gateway_base_url.trim_end_matches('/');

    // Step 1: → e
    let mut handshake = NoiseClientHandshake::new()?;
    let step1_msg = handshake.step1()?;

    let step1_resp = client
        .post(format!("{base}/v1/noise/handshake/step1"))
        .json(&serde_json::json!({ "client_message": step1_msg }))
        .send()
        .await?;

    if !step1_resp.status().is_success() {
        anyhow::bail!("noise handshake step1 failed: HTTP {}", step1_resp.status());
    }

    let step1_body: serde_json::Value = step1_resp.json().await?;
    let server_message = step1_body["server_message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing server_message in step1 response"))?;
    let session_id = step1_body["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing session_id in step1 response"))?
        .to_string();

    // Process server response and generate step 2
    handshake.process_step1_response(server_message)?;
    let step2_msg = handshake.step2()?;

    // Step 2: → s, se
    let step2_resp = client
        .post(format!("{base}/v1/noise/handshake/step2"))
        .json(&serde_json::json!({
            "session_id": &session_id,
            "client_message": step2_msg,
        }))
        .send()
        .await?;

    if !step2_resp.status().is_success() {
        anyhow::bail!("noise handshake step2 failed: HTTP {}", step2_resp.status());
    }

    // Handshake complete — create transport session
    handshake.finish(session_id)
}

/// Perform a Noise IK handshake with a gateway over HTTP (single round-trip).
///
/// Requires the server's static public key (32 bytes, typically cached from a
/// previous `GET /v1/privacy/info` call).
///
/// 1. POST `/v1/noise/handshake/ik` — sends `→ e, es, s, ss`
/// 2. Receives server's `← e, ee, se` + session ID
/// 3. Returns a `NoiseClientSession` ready for encrypt/decrypt.
#[allow(dead_code)] // Public API for consumers; not yet called within workspace
pub async fn perform_noise_handshake_ik(
    gateway_base_url: &str,
    server_public_key: &[u8; 32],
) -> anyhow::Result<NoiseClientSession> {
    let client = reqwest::Client::new();
    let base = gateway_base_url.trim_end_matches('/');

    let mut handshake = NoiseClientHandshake::new_ik(server_public_key)?;
    let step1_msg = handshake.step1()?;

    let resp = client
        .post(format!("{base}/v1/noise/handshake/ik"))
        .json(&serde_json::json!({ "message": step1_msg }))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("noise IK handshake failed: HTTP {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let server_message = body["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing message in IK response"))?;
    let session_id = body["session_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing session_id in IK response"))?
        .to_string();

    // Process server's ← e, ee, se — handshake should be complete
    handshake.process_step1_response(server_message)?;
    handshake.finish(session_id)
}

/// Auto-select the best handshake pattern and perform it.
///
/// Uses IK when the server's public key is cached (1 round-trip), otherwise
/// falls back to XX (2 round-trips).
#[allow(dead_code)] // Public API for consumers; not yet called within workspace
pub async fn auto_noise_handshake(
    gateway_base_url: &str,
    cached_server_key: Option<&[u8; 32]>,
) -> anyhow::Result<NoiseClientSession> {
    match cached_server_key {
        Some(key) => perform_noise_handshake_ik(gateway_base_url, key).await,
        None => perform_noise_handshake(gateway_base_url).await,
    }
}

/// HTTP transport that wraps requests with Noise encryption.
///
/// Serializes the `ChatRequest` to JSON, encrypts it via the Noise session,
/// sends the encrypted bytes with `X-Noise-Session` header, receives the
/// encrypted response, and decrypts it.
pub(crate) struct NoiseHttpTransport {
    inner: ReqwestTransport,
    session: Mutex<NoiseClientSession>,
}

impl NoiseHttpTransport {
    pub(crate) fn new(session: NoiseClientSession) -> Self {
        Self {
            inner: ReqwestTransport::new(),
            session: Mutex::new(session),
        }
    }
}

#[async_trait]
impl HttpTransport for NoiseHttpTransport {
    async fn send_chat(
        &self,
        url: &str,
        api_key: &str,
        payload: &ChatRequest,
    ) -> Result<TransportResponse, TransportError> {
        // Serialize to JSON then encrypt.
        let json_bytes = serde_json::to_vec(payload).map_err(|e| {
            TransportError::new(
                TransportErrorKind::Other,
                format!("failed to serialize request: {e}"),
            )
        })?;

        let (encrypted, session_id) = {
            let mut session = self.session.lock().map_err(|e| {
                TransportError::new(TransportErrorKind::Other, format!("session lock: {e}"))
            })?;
            session.encrypt_request(&json_bytes).map_err(|e| {
                TransportError::new(TransportErrorKind::Other, format!("noise encrypt: {e}"))
            })?
        };

        // Send encrypted body with noise session header.
        let mut request = self
            .inner
            .client
            .post(url)
            .header("X-Noise-Session", &session_id)
            .header("Content-Type", "application/octet-stream")
            .body(encrypted);
        if !api_key.is_empty() {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|e| {
            TransportError::new(TransportErrorKind::Connect, format!("request failed: {e}"))
        })?;

        let status = response.status().as_u16();
        let mut headers = std::collections::HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(parsed) = value.to_str() {
                headers.insert(name.as_str().to_ascii_lowercase(), parsed.to_string());
            }
        }

        let body_bytes = response.bytes().await.map_err(|e| {
            TransportError::new(TransportErrorKind::Body, format!("read body: {e}"))
        })?;

        // Decrypt the response body.
        let decrypted = {
            let mut session = self.session.lock().map_err(|e| {
                TransportError::new(TransportErrorKind::Other, format!("session lock: {e}"))
            })?;
            session.decrypt_response(&body_bytes).map_err(|e| {
                TransportError::new(TransportErrorKind::Other, format!("noise decrypt: {e}"))
            })?
        };

        let body = String::from_utf8(decrypted).map_err(|e| {
            TransportError::new(TransportErrorKind::Body, format!("invalid utf8: {e}"))
        })?;

        Ok(TransportResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_http_transport_wraps_session() {
        // Verify that NoiseHttpTransport can be constructed from a session.
        // Full integration test requires a running gateway — see gateway integration tests.
        use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let server_kp = NoiseKeypair::generate().unwrap();
        let mut client_hs = NoiseClientHandshake::new().unwrap();
        let step1_b64 = client_hs.step1().unwrap();

        // Simulate server
        let mut server_hs = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();
        let step1_bytes = STANDARD.decode(&step1_b64).unwrap();
        let mut buf = [0u8; 65535];
        server_hs.read_message(&step1_bytes, &mut buf).unwrap();
        let len = server_hs.write_message(b"", &mut buf).unwrap();
        let server_resp = STANDARD.encode(&buf[..len]);

        client_hs.process_step1_response(&server_resp).unwrap();
        let _step2 = client_hs.step2().unwrap();
        let session = client_hs.finish("test-session".to_string()).unwrap();

        let transport = NoiseHttpTransport::new(session);
        assert_eq!(
            transport.session.lock().unwrap().session_id(),
            "test-session"
        );
    }
}
