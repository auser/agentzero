//! Noise Protocol framework for E2E encrypted gateway communication.
//!
//! Uses the XX handshake pattern (mutual authentication with ephemeral keys)
//! via the `snow` crate. Provides forward secrecy through ephemeral DH.

use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, TransportState};
use std::time::Instant;

/// Noise protocol pattern string for the XX handshake.
const NOISE_PATTERN_XX: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// Noise protocol pattern string for the IK handshake (known server key).
const NOISE_PATTERN_IK: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

/// Maximum message size for Noise transport (64 KB per Noise spec).
const MAX_NOISE_MSG_LEN: usize = 65535;

/// An X25519 static keypair for Noise Protocol handshakes.
#[derive(Clone)]
pub struct NoiseKeypair {
    pub public: [u8; 32],
    private: Vec<u8>,
}

impl NoiseKeypair {
    /// Generate a new random X25519 keypair.
    pub fn generate() -> anyhow::Result<Self> {
        let builder = Builder::new(NOISE_PATTERN_XX.parse()?);
        let keypair = builder.generate_keypair()?;
        let mut public = [0u8; 32];
        public.copy_from_slice(&keypair.public);
        Ok(Self {
            public,
            private: keypair.private,
        })
    }

    /// Compute a routing ID (SHA-256 hash of the public key).
    pub fn routing_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.public);
        hasher.finalize().into()
    }
}

impl std::fmt::Debug for NoiseKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseKeypair")
            .field("public", &hex::encode(self.public))
            .finish_non_exhaustive()
    }
}

/// Encode bytes as hex. Inline to avoid adding a `hex` dependency.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Handshake role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

/// Wraps a `snow::HandshakeState` for step-by-step handshake execution.
pub struct NoiseHandshaker {
    state: HandshakeState,
    role: Role,
}

impl NoiseHandshaker {
    /// Create a new initiator (client) handshaker.
    pub fn new_initiator(pattern: &str, local_keypair: &NoiseKeypair) -> anyhow::Result<Self> {
        let pattern_str = pattern_string(pattern)?;
        let builder = Builder::new(pattern_str.parse()?).local_private_key(&local_keypair.private);
        let state = builder.build_initiator()?;
        Ok(Self {
            state,
            role: Role::Initiator,
        })
    }

    /// Create a new responder (server) handshaker.
    pub fn new_responder(pattern: &str, local_keypair: &NoiseKeypair) -> anyhow::Result<Self> {
        let pattern_str = pattern_string(pattern)?;
        let builder = Builder::new(pattern_str.parse()?).local_private_key(&local_keypair.private);
        let state = builder.build_responder()?;
        Ok(Self {
            state,
            role: Role::Responder,
        })
    }

    /// Create an IK initiator that knows the remote server's static public key.
    pub fn new_ik_initiator(
        local_keypair: &NoiseKeypair,
        remote_public: &[u8; 32],
    ) -> anyhow::Result<Self> {
        let builder = Builder::new(NOISE_PATTERN_IK.parse()?)
            .local_private_key(&local_keypair.private)
            .remote_public_key(remote_public);
        let state = builder.build_initiator()?;
        Ok(Self {
            state,
            role: Role::Initiator,
        })
    }

    /// Whether the handshake is complete and ready for transport mode.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Write the next handshake message into `out`. Returns the number of
    /// bytes written.
    pub fn write_message(&mut self, payload: &[u8], out: &mut [u8]) -> anyhow::Result<usize> {
        let len = self.state.write_message(payload, out)?;
        Ok(len)
    }

    /// Read an incoming handshake message. Returns the decrypted payload length.
    pub fn read_message(&mut self, msg: &[u8], out: &mut [u8]) -> anyhow::Result<usize> {
        let len = self.state.read_message(msg, out)?;
        Ok(len)
    }

    /// Transition to transport mode after the handshake completes.
    /// Returns a `NoiseSession` that can encrypt/decrypt application data.
    pub fn into_transport(self) -> anyhow::Result<NoiseSession> {
        if !self.state.is_handshake_finished() {
            anyhow::bail!("handshake is not yet complete");
        }
        let remote_static = self.state.get_remote_static().map(|s| {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(s);
            arr
        });
        let transport = self.state.into_transport_mode()?;

        // Derive a session ID from the handshake hash.
        let mut session_id = [0u8; 32];
        // Use a deterministic derivation: SHA-256 of "agentzero-noise-session" + remote key
        let mut hasher = Sha256::new();
        hasher.update(b"agentzero-noise-session");
        if let Some(ref rpk) = remote_static {
            hasher.update(rpk);
        }
        session_id.copy_from_slice(&hasher.finalize());

        Ok(NoiseSession {
            session_id,
            transport,
            remote_static,
            created_at: Instant::now(),
            last_activity: Instant::now(),
        })
    }

    /// The role of this handshaker.
    pub fn role(&self) -> Role {
        self.role
    }
}

/// An established Noise transport session for encrypting/decrypting messages.
pub struct NoiseSession {
    session_id: [u8; 32],
    transport: TransportState,
    remote_static: Option<[u8; 32]>,
    created_at: Instant,
    last_activity: Instant,
}

impl NoiseSession {
    /// Unique identifier for this session.
    pub fn session_id(&self) -> &[u8; 32] {
        &self.session_id
    }

    /// The remote peer's static public key, if available.
    pub fn remote_static(&self) -> Option<&[u8; 32]> {
        self.remote_static.as_ref()
    }

    /// When this session was established.
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// When this session last sent or received data.
    pub fn last_activity(&self) -> Instant {
        self.last_activity
    }

    /// Whether the session has been inactive longer than `timeout_secs`.
    pub fn is_expired(&self, timeout_secs: u64) -> bool {
        self.last_activity.elapsed().as_secs() >= timeout_secs
    }

    /// Encrypt a plaintext message. Returns the ciphertext.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        if plaintext.len() > MAX_NOISE_MSG_LEN - 16 {
            anyhow::bail!(
                "plaintext too large for Noise transport ({} bytes, max {})",
                plaintext.len(),
                MAX_NOISE_MSG_LEN - 16
            );
        }
        let mut buf = vec![0u8; plaintext.len() + 16]; // 16 bytes for AEAD tag
        let len = self.transport.write_message(plaintext, &mut buf)?;
        buf.truncate(len);
        self.last_activity = Instant::now();
        Ok(buf)
    }

    /// Decrypt a ciphertext message. Returns the plaintext.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        if ciphertext.len() > MAX_NOISE_MSG_LEN {
            anyhow::bail!(
                "ciphertext too large for Noise transport ({} bytes, max {})",
                ciphertext.len(),
                MAX_NOISE_MSG_LEN
            );
        }
        let mut buf = vec![0u8; ciphertext.len()];
        let len = self.transport.read_message(ciphertext, &mut buf)?;
        buf.truncate(len);
        self.last_activity = Instant::now();
        Ok(buf)
    }
}

/// Resolve a handshake pattern name to the full Noise pattern string.
fn pattern_string(pattern: &str) -> anyhow::Result<&'static str> {
    match pattern {
        "XX" => Ok(NOISE_PATTERN_XX),
        "IK" => Ok(NOISE_PATTERN_IK),
        other => anyhow::bail!("unsupported Noise pattern: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_generation_produces_valid_keys() {
        let kp = NoiseKeypair::generate().unwrap();
        // X25519 public key should be 32 bytes and not all zeros
        assert_eq!(kp.public.len(), 32);
        assert_ne!(kp.public, [0u8; 32]);
    }

    #[test]
    fn routing_id_is_sha256_of_public_key() {
        let kp = NoiseKeypair::generate().unwrap();
        let id = kp.routing_id();
        // Manually compute SHA-256
        let mut hasher = Sha256::new();
        hasher.update(kp.public);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(id, expected);
    }

    #[test]
    fn xx_handshake_round_trip() {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();

        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();

        let mut buf = [0u8; MAX_NOISE_MSG_LEN];
        let mut payload_buf = [0u8; MAX_NOISE_MSG_LEN];

        // Message 1: Client → Server (→ e)
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();

        // Message 2: Server → Client (← e, ee, s, es)
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut payload_buf).unwrap();

        // Message 3: Client → Server (→ s, se)
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();

        assert!(client.is_finished());
        assert!(server.is_finished());

        // Transition to transport mode
        let mut client_session = client.into_transport().unwrap();
        let mut server_session = server.into_transport().unwrap();

        // Encrypt/decrypt round-trip
        let plaintext = b"hello from client";
        let ciphertext = client_session.encrypt(plaintext).unwrap();
        let decrypted = server_session.decrypt(&ciphertext).unwrap();
        assert_eq!(&decrypted, plaintext);

        // Server → Client
        let plaintext2 = b"hello from server";
        let ciphertext2 = server_session.encrypt(plaintext2).unwrap();
        let decrypted2 = client_session.decrypt(&ciphertext2).unwrap();
        assert_eq!(&decrypted2, plaintext2);
    }

    #[test]
    fn ik_handshake_round_trip() {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();

        let mut client = NoiseHandshaker::new_ik_initiator(&client_kp, &server_kp.public).unwrap();
        let mut server = NoiseHandshaker::new_responder("IK", &server_kp).unwrap();

        let mut buf = [0u8; MAX_NOISE_MSG_LEN];
        let mut payload_buf = [0u8; MAX_NOISE_MSG_LEN];

        // Message 1: Client → Server (→ e, es, s, ss)
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();

        // Message 2: Server → Client (← e, ee, se)
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut payload_buf).unwrap();

        assert!(client.is_finished());
        assert!(server.is_finished());

        let mut client_session = client.into_transport().unwrap();
        let mut server_session = server.into_transport().unwrap();

        let msg = b"IK pattern works";
        let enc = client_session.encrypt(msg).unwrap();
        let dec = server_session.decrypt(&enc).unwrap();
        assert_eq!(&dec, msg);
    }

    #[test]
    fn session_expiry_check() {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();

        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();

        let mut buf = [0u8; MAX_NOISE_MSG_LEN];
        let mut payload_buf = [0u8; MAX_NOISE_MSG_LEN];

        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut payload_buf).unwrap();
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();

        let session = client.into_transport().unwrap();
        // Just created — should not be expired with any reasonable timeout
        assert!(!session.is_expired(3600));
        // Expired with 0-second timeout
        assert!(session.is_expired(0));
    }

    #[test]
    fn into_transport_fails_before_handshake_complete() {
        let kp = NoiseKeypair::generate().unwrap();
        let handshaker = NoiseHandshaker::new_initiator("XX", &kp).unwrap();
        let result = handshaker.into_transport();
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_rejects_oversized_plaintext() {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();

        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();

        let mut buf = [0u8; MAX_NOISE_MSG_LEN];
        let mut payload_buf = [0u8; MAX_NOISE_MSG_LEN];

        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut payload_buf).unwrap();
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut payload_buf).unwrap();

        let mut session = client.into_transport().unwrap();
        // Try to encrypt a message larger than MAX_NOISE_MSG_LEN - 16
        let oversized = vec![0u8; MAX_NOISE_MSG_LEN];
        let result = session.encrypt(&oversized);
        assert!(result.is_err());
    }

    #[test]
    fn wrong_session_cannot_decrypt() {
        let kp1 = NoiseKeypair::generate().unwrap();
        let kp2 = NoiseKeypair::generate().unwrap();
        let kp3 = NoiseKeypair::generate().unwrap();

        // Session A: kp1 ↔ kp2
        let (mut session_a_client, mut _session_a_server) = {
            let mut c = NoiseHandshaker::new_initiator("XX", &kp1).unwrap();
            let mut s = NoiseHandshaker::new_responder("XX", &kp2).unwrap();
            let mut buf = [0u8; MAX_NOISE_MSG_LEN];
            let mut pb = [0u8; MAX_NOISE_MSG_LEN];
            let len = c.write_message(b"", &mut buf).unwrap();
            s.read_message(&buf[..len], &mut pb).unwrap();
            let len = s.write_message(b"", &mut buf).unwrap();
            c.read_message(&buf[..len], &mut pb).unwrap();
            let len = c.write_message(b"", &mut buf).unwrap();
            s.read_message(&buf[..len], &mut pb).unwrap();
            (c.into_transport().unwrap(), s.into_transport().unwrap())
        };

        // Session B: kp1 ↔ kp3 (different server)
        let (mut _session_b_client, mut session_b_server) = {
            let mut c = NoiseHandshaker::new_initiator("XX", &kp1).unwrap();
            let mut s = NoiseHandshaker::new_responder("XX", &kp3).unwrap();
            let mut buf = [0u8; MAX_NOISE_MSG_LEN];
            let mut pb = [0u8; MAX_NOISE_MSG_LEN];
            let len = c.write_message(b"", &mut buf).unwrap();
            s.read_message(&buf[..len], &mut pb).unwrap();
            let len = s.write_message(b"", &mut buf).unwrap();
            c.read_message(&buf[..len], &mut pb).unwrap();
            let len = c.write_message(b"", &mut buf).unwrap();
            s.read_message(&buf[..len], &mut pb).unwrap();
            (c.into_transport().unwrap(), s.into_transport().unwrap())
        };

        // Encrypt with session A, try to decrypt with session B
        let ciphertext = session_a_client.encrypt(b"secret").unwrap();
        let result = session_b_server.decrypt(&ciphertext);
        assert!(result.is_err(), "different session should not decrypt");
    }

    #[test]
    fn unsupported_pattern_fails() {
        let kp = NoiseKeypair::generate().unwrap();
        let result = NoiseHandshaker::new_initiator("NK", &kp);
        assert!(result.is_err());
    }

    #[test]
    fn session_has_remote_static_key() {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();

        let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
        let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();

        let mut buf = [0u8; MAX_NOISE_MSG_LEN];
        let mut pb = [0u8; MAX_NOISE_MSG_LEN];

        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut pb).unwrap();
        let len = server.write_message(b"", &mut buf).unwrap();
        client.read_message(&buf[..len], &mut pb).unwrap();
        let len = client.write_message(b"", &mut buf).unwrap();
        server.read_message(&buf[..len], &mut pb).unwrap();

        let client_session = client.into_transport().unwrap();
        let server_session = server.into_transport().unwrap();

        // Client should know server's static key
        assert_eq!(client_session.remote_static(), Some(&server_kp.public));
        // Server should know client's static key
        assert_eq!(server_session.remote_static(), Some(&client_kp.public));
    }
}
