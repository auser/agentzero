//! Client-side Noise Protocol handshake for encrypted gateway communication.
//!
//! Provides a transport-agnostic `NoiseClientHandshake` that produces the
//! handshake messages (as byte buffers) and a `NoiseClientSession` for
//! encrypting/decrypting after handshake completes. The actual HTTP transport
//! lives in `agentzero-infra` where `reqwest` is available.

use super::noise::{NoiseHandshaker, NoiseKeypair, NoiseSession};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

/// Privacy capabilities returned by `GET /v1/privacy/info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyInfo {
    pub noise_enabled: bool,
    pub handshake_pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_fingerprint: Option<String>,
    pub sealed_envelopes_enabled: bool,
    pub relay_mode: bool,
    /// Handshake patterns supported by the server. `["XX"]` is always
    /// supported; `"IK"` is available when `public_key` is present.
    #[serde(default)]
    pub supported_patterns: Vec<String>,
}

/// Transport-agnostic Noise handshake orchestrator.
///
/// Generates handshake messages for the caller to send over any transport
/// (HTTP, WebSocket, etc). Usage:
///
/// ```ignore
/// let mut hs = NoiseClientHandshake::new()?;
/// let step1_msg = hs.step1()?;           // Send to server
/// hs.process_step1_response(server_msg)?; // Process server response
/// let step2_msg = hs.step2()?;           // Send to server
/// let session = hs.finish(session_id)?;   // Get encrypted session
/// ```
pub struct NoiseClientHandshake {
    handshaker: NoiseHandshaker,
}

impl NoiseClientHandshake {
    /// Create a new client XX handshake with a fresh keypair.
    pub fn new() -> anyhow::Result<Self> {
        let client_kp = NoiseKeypair::generate()?;
        let handshaker = NoiseHandshaker::new_initiator("XX", &client_kp)?;
        Ok(Self { handshaker })
    }

    /// Create a new client IK handshake. Requires the server's static public
    /// key (32 bytes). Completes in 1 HTTP round-trip instead of 2.
    pub fn new_ik(server_public_key: &[u8; 32]) -> anyhow::Result<Self> {
        let client_kp = NoiseKeypair::generate()?;
        let handshaker = NoiseHandshaker::new_ik_initiator(&client_kp, server_public_key)?;
        Ok(Self { handshaker })
    }

    /// Generate the step 1 message (→ e). Returns base64-encoded bytes.
    pub fn step1(&mut self) -> anyhow::Result<String> {
        let mut buf = [0u8; 65535];
        let len = self.handshaker.write_message(b"", &mut buf)?;
        Ok(STANDARD.encode(&buf[..len]))
    }

    /// Process the server's step 1 response (← e, ee, s, es).
    /// Takes base64-encoded server message.
    pub fn process_step1_response(&mut self, server_message_b64: &str) -> anyhow::Result<()> {
        let server_msg = STANDARD.decode(server_message_b64)?;
        let mut payload_buf = [0u8; 65535];
        self.handshaker
            .read_message(&server_msg, &mut payload_buf)?;
        Ok(())
    }

    /// Generate the step 2 message (→ s, se). Returns base64-encoded bytes.
    pub fn step2(&mut self) -> anyhow::Result<String> {
        let mut buf = [0u8; 65535];
        let len = self.handshaker.write_message(b"", &mut buf)?;
        Ok(STANDARD.encode(&buf[..len]))
    }

    /// Finish the handshake and create an encrypted session.
    pub fn finish(self, session_id_hex: String) -> anyhow::Result<NoiseClientSession> {
        let session = self.handshaker.into_transport()?;
        Ok(NoiseClientSession {
            session,
            session_id_hex,
        })
    }
}

/// An established Noise session for client-side encrypt/decrypt.
pub struct NoiseClientSession {
    session: NoiseSession,
    session_id_hex: String,
}

impl NoiseClientSession {
    /// Encrypt a request body. Returns `(encrypted_body, session_id_header)`.
    pub fn encrypt_request(&mut self, body: &[u8]) -> anyhow::Result<(Vec<u8>, String)> {
        let encrypted = self.session.encrypt(body)?;
        Ok((encrypted, self.session_id_hex.clone()))
    }

    /// Decrypt a response body.
    pub fn decrypt_response(&mut self, body: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.session.decrypt(body)
    }

    /// The hex session ID for `X-Noise-Session` header.
    pub fn session_id(&self) -> &str {
        &self.session_id_hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::noise::{NoiseHandshaker, NoiseKeypair};

    #[test]
    fn privacy_info_deserializes() {
        let json = serde_json::json!({
            "noise_enabled": true,
            "handshake_pattern": "XX",
            "public_key": "YWJj",
            "key_epoch": 5,
            "key_fingerprint": "a1b2c3d4e5f6a7b8",
            "sealed_envelopes_enabled": false,
            "relay_mode": false,
        });
        let info: PrivacyInfo = serde_json::from_value(json).expect("should deserialize");
        assert!(info.noise_enabled);
        assert_eq!(info.handshake_pattern, "XX");
        assert_eq!(info.key_epoch, Some(5));
        // supported_patterns defaults to empty vec when not in JSON
        assert!(info.supported_patterns.is_empty());
    }

    #[test]
    fn privacy_info_deserializes_minimal() {
        let json = serde_json::json!({
            "noise_enabled": false,
            "handshake_pattern": "XX",
            "sealed_envelopes_enabled": false,
            "relay_mode": false,
        });
        let info: PrivacyInfo = serde_json::from_value(json).expect("should deserialize");
        assert!(!info.noise_enabled);
        assert!(info.public_key.is_none());
    }

    #[test]
    fn privacy_info_serializes_with_optional_fields() {
        let info = PrivacyInfo {
            noise_enabled: true,
            handshake_pattern: "XX".to_string(),
            public_key: Some("abc123".to_string()),
            key_epoch: Some(42),
            key_fingerprint: Some("deadbeef".to_string()),
            sealed_envelopes_enabled: true,
            relay_mode: false,
            supported_patterns: vec!["XX".to_string(), "IK".to_string()],
        };
        let json = serde_json::to_value(&info).expect("should serialize");
        assert_eq!(json["noise_enabled"], true);
        assert_eq!(json["public_key"], "abc123");
        assert_eq!(json["key_epoch"], 42);
        assert_eq!(json["supported_patterns"], serde_json::json!(["XX", "IK"]));
    }

    #[test]
    fn privacy_info_serializes_without_optional_fields() {
        let info = PrivacyInfo {
            noise_enabled: false,
            handshake_pattern: "XX".to_string(),
            public_key: None,
            key_epoch: None,
            key_fingerprint: None,
            sealed_envelopes_enabled: false,
            relay_mode: false,
            supported_patterns: vec!["XX".to_string()],
        };
        let json = serde_json::to_value(&info).expect("should serialize");
        assert!(json.get("public_key").is_none());
        assert!(json.get("key_epoch").is_none());
    }

    #[test]
    fn client_handshake_with_server_round_trip() {
        // Simulate a full client/server handshake without HTTP.
        let server_kp = NoiseKeypair::generate().unwrap();

        // Client side
        let mut client_hs = NoiseClientHandshake::new().unwrap();
        let step1_b64 = client_hs.step1().unwrap();

        // Server side: process step 1
        let mut server_hs = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();
        let step1_bytes = STANDARD.decode(&step1_b64).unwrap();
        let mut payload_buf = [0u8; 65535];
        server_hs
            .read_message(&step1_bytes, &mut payload_buf)
            .unwrap();

        let mut buf = [0u8; 65535];
        let len = server_hs.write_message(b"", &mut buf).unwrap();
        let server_resp_b64 = STANDARD.encode(&buf[..len]);

        // Client: process step 1 response
        client_hs.process_step1_response(&server_resp_b64).unwrap();
        let step2_b64 = client_hs.step2().unwrap();

        // Server: process step 2
        let step2_bytes = STANDARD.decode(&step2_b64).unwrap();
        server_hs
            .read_message(&step2_bytes, &mut payload_buf)
            .unwrap();
        assert!(server_hs.is_finished());

        let mut server_session = server_hs.into_transport().unwrap();

        // Client: finish
        let session_id = "aabbccdd".to_string();
        let mut client_session = client_hs.finish(session_id).unwrap();

        // Test encrypt/decrypt round-trip
        let (encrypted, sid) = client_session.encrypt_request(b"hello server").unwrap();
        assert_eq!(sid, "aabbccdd");
        let decrypted = server_session.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"hello server");

        // Server → Client
        let encrypted = server_session.encrypt(b"hello client").unwrap();
        let decrypted = client_session.decrypt_response(&encrypted).unwrap();
        assert_eq!(decrypted, b"hello client");
    }

    #[test]
    fn step1_produces_base64() {
        let mut hs = NoiseClientHandshake::new().unwrap();
        let msg = hs.step1().unwrap();
        // Should be valid base64
        assert!(STANDARD.decode(&msg).is_ok());
        assert!(!msg.is_empty());
    }

    #[test]
    fn ik_client_handshake_round_trip() {
        // IK handshake: client knows server's static key beforehand.
        let server_kp = NoiseKeypair::generate().unwrap();

        // Client creates IK handshake with server's public key
        let mut client_hs = NoiseClientHandshake::new_ik(&server_kp.public).unwrap();
        let step1_b64 = client_hs.step1().unwrap();

        // Server side: IK responder processes step1 (→ e, es, s, ss)
        let mut server_hs = NoiseHandshaker::new_responder("IK", &server_kp).unwrap();
        let step1_bytes = STANDARD.decode(&step1_b64).unwrap();
        let mut payload_buf = [0u8; 65535];
        server_hs
            .read_message(&step1_bytes, &mut payload_buf)
            .unwrap();

        // Server responds (← e, ee, se)
        let mut buf = [0u8; 65535];
        let len = server_hs.write_message(b"", &mut buf).unwrap();
        let server_resp_b64 = STANDARD.encode(&buf[..len]);

        // Server is done after 2 messages in IK
        assert!(server_hs.is_finished());
        let mut server_session = server_hs.into_transport().unwrap();

        // Client processes response and finishes
        client_hs.process_step1_response(&server_resp_b64).unwrap();
        assert!(client_hs.handshaker.is_finished());
        let mut client_session = client_hs.finish("ik-session-1".to_string()).unwrap();

        // Verify encrypt/decrypt
        let (encrypted, sid) = client_session.encrypt_request(b"ik hello").unwrap();
        assert_eq!(sid, "ik-session-1");
        let decrypted = server_session.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"ik hello");

        let encrypted = server_session.encrypt(b"ik reply").unwrap();
        let decrypted = client_session.decrypt_response(&encrypted).unwrap();
        assert_eq!(decrypted, b"ik reply");
    }

    #[test]
    fn ik_wrong_server_key_fails() {
        let server_kp = NoiseKeypair::generate().unwrap();
        let wrong_kp = NoiseKeypair::generate().unwrap();

        // Client uses wrong server public key
        let mut client_hs = NoiseClientHandshake::new_ik(&wrong_kp.public).unwrap();
        let step1_b64 = client_hs.step1().unwrap();

        // Server (with the real key) tries to process — should fail
        let mut server_hs = NoiseHandshaker::new_responder("IK", &server_kp).unwrap();
        let step1_bytes = STANDARD.decode(&step1_b64).unwrap();
        let mut payload_buf = [0u8; 65535];
        let result = server_hs.read_message(&step1_bytes, &mut payload_buf);
        assert!(result.is_err(), "IK with wrong key should fail");
    }

    #[test]
    fn privacy_info_supported_patterns_includes_ik() {
        let json = serde_json::json!({
            "noise_enabled": true,
            "handshake_pattern": "XX",
            "public_key": "YWJj",
            "sealed_envelopes_enabled": false,
            "relay_mode": false,
            "supported_patterns": ["XX", "IK"],
        });
        let info: PrivacyInfo = serde_json::from_value(json).expect("should deserialize");
        assert_eq!(info.supported_patterns, vec!["XX", "IK"]);
    }
}
