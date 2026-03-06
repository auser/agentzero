//! Sealed envelope encryption for zero-knowledge packet routing.
//!
//! Double-envelope encryption where relays can route by `routing_id` but
//! cannot read content. Uses X25519 key exchange + XSalsa20Poly1305 AEAD
//! via the `crypto_box` crate with ephemeral sender keys for anonymity.

use crypto_box::{
    aead::{Aead, AeadCore, OsRng},
    PublicKey, SalsaBox, SecretKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Sealed envelope format version.
const ENVELOPE_VERSION: u8 = 1;

/// A sealed envelope that can be routed by `routing_id` without revealing content.
///
/// The relay sees only `routing_id` (a SHA-256 hash of the recipient's public key)
/// and the `ephemeral_pubkey`. It cannot decrypt the payload without the
/// recipient's secret key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedEnvelope {
    /// Format version for forward compatibility.
    pub version: u8,
    /// SHA-256(recipient_pubkey) — opaque routing identifier.
    pub routing_id: [u8; 32],
    /// Sender's ephemeral X25519 public key (needed for decryption).
    pub ephemeral_pubkey: [u8; 32],
    /// XSalsa20Poly1305 nonce.
    pub nonce: [u8; 24],
    /// Encrypted payload (XSalsa20Poly1305 ciphertext + tag).
    pub encrypted_payload: Vec<u8>,
    /// Unix epoch timestamp (replay protection).
    pub timestamp: u64,
    /// Envelope expiry in seconds from timestamp.
    pub ttl_secs: u32,
}

impl SealedEnvelope {
    /// Seal plaintext for a recipient identified by their X25519 public key.
    ///
    /// Generates an ephemeral keypair for anonymous sender behavior. The
    /// recipient decrypts using their secret key + the ephemeral public key
    /// included in the envelope header.
    pub fn seal(recipient_pubkey: &[u8; 32], plaintext: &[u8], ttl_secs: u32) -> Self {
        let recipient_pk = PublicKey::from(*recipient_pubkey);

        // Generate ephemeral sender keypair (anonymous — not linked to any identity).
        let ephemeral_secret = SecretKey::generate(&mut OsRng);
        let ephemeral_pubkey: [u8; 32] = *ephemeral_secret.public_key().as_bytes();

        // Create crypto_box from ephemeral secret + recipient public.
        let salsa_box = SalsaBox::new(&recipient_pk, &ephemeral_secret);
        let nonce = SalsaBox::generate_nonce(&mut OsRng);
        let encrypted_payload = salsa_box
            .encrypt(&nonce, plaintext)
            .expect("encryption should not fail with valid keys");

        let routing_id = compute_routing_id(recipient_pubkey);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();

        let mut nonce_bytes = [0u8; 24];
        nonce_bytes.copy_from_slice(&nonce);

        Self {
            version: ENVELOPE_VERSION,
            routing_id,
            ephemeral_pubkey,
            nonce: nonce_bytes,
            encrypted_payload,
            timestamp,
            ttl_secs,
        }
    }

    /// Open a sealed envelope using the recipient's secret key.
    ///
    /// Reconstructs the shared secret from (recipient_secret, ephemeral_pubkey)
    /// and decrypts the payload.
    pub fn open(&self, recipient_secret: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
        if self.version != ENVELOPE_VERSION {
            anyhow::bail!(
                "unsupported sealed envelope version {} (expected {})",
                self.version,
                ENVELOPE_VERSION,
            );
        }

        if self.is_expired() {
            anyhow::bail!("sealed envelope has expired");
        }

        let recipient_sk = SecretKey::from(*recipient_secret);
        let ephemeral_pk = PublicKey::from(self.ephemeral_pubkey);

        let salsa_box = SalsaBox::new(&ephemeral_pk, &recipient_sk);
        let nonce = crypto_box::Nonce::from(self.nonce);

        salsa_box
            .decrypt(&nonce, self.encrypted_payload.as_ref())
            .map_err(|_| anyhow::anyhow!("failed to decrypt sealed envelope"))
    }

    /// Check if this envelope has expired based on its timestamp and TTL.
    pub fn is_expired(&self) -> bool {
        if self.ttl_secs == 0 {
            return false; // 0 means no expiry
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();
        now > self.timestamp.saturating_add(self.ttl_secs as u64)
    }

    /// Serialize the envelope to JSON bytes.
    pub fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| anyhow::anyhow!("failed to serialize envelope: {e}"))
    }

    /// Deserialize an envelope from JSON bytes.
    pub fn from_json(data: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(data)
            .map_err(|e| anyhow::anyhow!("failed to deserialize envelope: {e}"))
    }
}

/// Generate an X25519 keypair for sealed envelope operations.
///
/// Returns `(public_key, secret_key)` as raw 32-byte arrays.
pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let secret = SecretKey::generate(&mut OsRng);
    let public: [u8; 32] = *secret.public_key().as_bytes();
    let secret_bytes: [u8; 32] = secret.to_bytes();
    (public, secret_bytes)
}

/// Compute the routing ID for a public key (SHA-256 hash).
pub fn compute_routing_id(pubkey: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(pubkey);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_box::SecretKey;

    fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let secret = SecretKey::generate(&mut OsRng);
        let public: [u8; 32] = *secret.public_key().as_bytes();
        let secret_bytes: [u8; 32] = secret.to_bytes();
        (secret_bytes, public)
    }

    #[test]
    fn seal_and_open_round_trip() {
        let (secret, public) = generate_keypair();
        let plaintext = b"hello zero-knowledge world";

        let envelope = SealedEnvelope::seal(&public, plaintext, 3600);
        assert_eq!(envelope.version, 1);
        assert_eq!(envelope.routing_id, compute_routing_id(&public));

        let decrypted = envelope.open(&secret).expect("should decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let (_secret, public) = generate_keypair();
        let (wrong_secret, _wrong_public) = generate_keypair();

        let envelope = SealedEnvelope::seal(&public, b"secret data", 3600);
        let err = envelope
            .open(&wrong_secret)
            .expect_err("wrong key should fail");
        assert!(err.to_string().contains("failed to decrypt"));
    }

    #[test]
    fn expired_envelope_rejected() {
        let (secret, public) = generate_keypair();
        let mut envelope = SealedEnvelope::seal(&public, b"expires fast", 1);
        // Backdate the timestamp so it's already expired.
        envelope.timestamp = envelope.timestamp.saturating_sub(10);

        assert!(envelope.is_expired());
        let err = envelope.open(&secret).expect_err("expired should fail");
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn zero_ttl_means_no_expiry() {
        let (secret, public) = generate_keypair();
        let mut envelope = SealedEnvelope::seal(&public, b"forever", 0);
        // Even with old timestamp, 0 TTL means no expiry.
        envelope.timestamp = 0;

        assert!(!envelope.is_expired());
        let decrypted = envelope.open(&secret).expect("should decrypt");
        assert_eq!(decrypted, b"forever");
    }

    #[test]
    fn routing_id_is_deterministic() {
        let pubkey = [42u8; 32];
        let id1 = compute_routing_id(&pubkey);
        let id2 = compute_routing_id(&pubkey);
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_keys_produce_different_routing_ids() {
        let id1 = compute_routing_id(&[1u8; 32]);
        let id2 = compute_routing_id(&[2u8; 32]);
        assert_ne!(id1, id2);
    }

    #[test]
    fn json_serialization_round_trip() {
        let (_secret, public) = generate_keypair();
        let envelope = SealedEnvelope::seal(&public, b"serializable", 3600);

        let json = envelope.to_json().expect("should serialize");
        let restored = SealedEnvelope::from_json(&json).expect("should deserialize");

        assert_eq!(restored.version, envelope.version);
        assert_eq!(restored.routing_id, envelope.routing_id);
        assert_eq!(restored.ephemeral_pubkey, envelope.ephemeral_pubkey);
        assert_eq!(restored.nonce, envelope.nonce);
        assert_eq!(restored.encrypted_payload, envelope.encrypted_payload);
        assert_eq!(restored.timestamp, envelope.timestamp);
        assert_eq!(restored.ttl_secs, envelope.ttl_secs);
    }

    #[test]
    fn unsupported_version_rejected() {
        let (secret, public) = generate_keypair();
        let mut envelope = SealedEnvelope::seal(&public, b"v2", 3600);
        envelope.version = 99;

        let err = envelope.open(&secret).expect_err("bad version should fail");
        assert!(err
            .to_string()
            .contains("unsupported sealed envelope version"));
    }

    #[test]
    fn large_payload_seal_and_open() {
        let (secret, public) = generate_keypair();
        let plaintext = vec![0xABu8; 1024 * 64]; // 64 KB

        let envelope = SealedEnvelope::seal(&public, &plaintext, 3600);
        let decrypted = envelope
            .open(&secret)
            .expect("should decrypt large payload");
        assert_eq!(decrypted, plaintext);
    }
}
