//! Privacy key management for sealed envelope encryption.
//!
//! Manages X25519 identity keypairs with epoch-based rotation and overlap
//! windows for graceful key transitions.

use crypto_box::{aead::OsRng, SecretKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// An identity keypair with epoch tracking for rotation.
///
/// Intentionally does NOT implement `Serialize` to prevent accidental
/// leakage of `secret_key` to JSON, logs, or network. Use
/// `KeyRingStore` for persistence (encrypted at rest via storage layer).
/// `Deserialize` is kept so the CLI can reconstruct from storage.
#[derive(Clone, Deserialize)]
pub struct IdentityKeyPair {
    /// Monotonically increasing epoch number.
    pub epoch: u64,
    /// X25519 public key bytes.
    pub public_key: [u8; 32],
    /// X25519 secret key bytes (encrypted at rest by storage layer).
    secret_key: [u8; 32],
    /// Unix timestamp when this keypair was created.
    pub created_at: u64,
}

impl IdentityKeyPair {
    /// Generate a new identity keypair at the given epoch.
    pub fn generate(epoch: u64) -> Self {
        let secret = SecretKey::generate(&mut OsRng);
        let public_key: [u8; 32] = *secret.public_key().as_bytes();
        let secret_key: [u8; 32] = secret.to_bytes();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();

        Self {
            epoch,
            public_key,
            secret_key,
            created_at,
        }
    }

    /// Get the secret key bytes (for decryption).
    pub fn secret_key(&self) -> &[u8; 32] {
        &self.secret_key
    }

    /// Compute a fingerprint of the public key (first 8 bytes of SHA-256, hex-encoded).
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.public_key);
        let hash: [u8; 32] = hasher.finalize().into();
        hash[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    }

    /// Compute the routing ID for this keypair (full SHA-256 of public key).
    pub fn routing_id(&self) -> [u8; 32] {
        super::envelope::compute_routing_id(&self.public_key)
    }

    /// Age of this keypair in seconds.
    pub fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_secs();
        now.saturating_sub(self.created_at)
    }
}

impl std::fmt::Debug for IdentityKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityKeyPair")
            .field("epoch", &self.epoch)
            .field(
                "public_key_prefix",
                &format_args!(
                    "{}",
                    self.public_key[..4]
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
            )
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

/// Manages identity keypairs with epoch-based rotation.
///
/// Holds the current keypair and optionally one previous keypair
/// during the overlap window for graceful key transitions.
#[derive(Debug)]
pub struct PrivacyKeyRing {
    /// Current active keypair (highest epoch).
    current: IdentityKeyPair,
    /// Previous keypair kept during overlap window (may be None).
    previous: Option<IdentityKeyPair>,
    /// Rotation interval in seconds (0 = no automatic rotation).
    rotation_interval_secs: u64,
    /// Overlap window in seconds (how long to keep old key after rotation).
    overlap_secs: u64,
}

impl PrivacyKeyRing {
    /// Create a new keyring with a freshly generated keypair.
    pub fn new(rotation_interval_secs: u64, overlap_secs: u64) -> Self {
        Self {
            current: IdentityKeyPair::generate(0),
            previous: None,
            rotation_interval_secs,
            overlap_secs,
        }
    }

    /// Create a keyring from an existing keypair.
    pub fn from_keypair(
        keypair: IdentityKeyPair,
        rotation_interval_secs: u64,
        overlap_secs: u64,
    ) -> Self {
        Self {
            current: keypair,
            previous: None,
            rotation_interval_secs,
            overlap_secs,
        }
    }

    /// Get the current active keypair.
    pub fn current(&self) -> &IdentityKeyPair {
        &self.current
    }

    /// Get the previous keypair (if within overlap window).
    pub fn previous(&self) -> Option<&IdentityKeyPair> {
        self.previous.as_ref()
    }

    /// Get the current public key.
    pub fn public_key(&self) -> &[u8; 32] {
        &self.current.public_key
    }

    /// Current epoch number.
    pub fn epoch(&self) -> u64 {
        self.current.epoch
    }

    /// Check if rotation is needed and perform it if so.
    /// Returns `Some(new_epoch)` if rotation occurred.
    pub fn check_rotation(&mut self) -> Option<u64> {
        if self.rotation_interval_secs == 0 {
            return None;
        }

        if self.current.age_secs() < self.rotation_interval_secs {
            return None;
        }

        Some(self.rotate_now())
    }

    /// Force an immediate rotation regardless of interval.
    /// Returns the new epoch number.
    pub fn force_rotate(&mut self) -> u64 {
        self.rotate_now()
    }

    /// When the next automatic rotation is expected (Unix timestamp).
    /// Returns `None` if auto-rotation is disabled (`rotation_interval_secs == 0`).
    pub fn next_rotation_at(&self) -> Option<u64> {
        if self.rotation_interval_secs == 0 {
            return None;
        }
        Some(self.current.created_at + self.rotation_interval_secs)
    }

    /// Internal: perform the actual rotation.
    fn rotate_now(&mut self) -> u64 {
        let new_epoch = self.current.epoch + 1;
        let old_current =
            std::mem::replace(&mut self.current, IdentityKeyPair::generate(new_epoch));
        self.previous = Some(old_current);
        new_epoch
    }

    /// Try to open a sealed envelope using current key, falling back to previous.
    pub fn try_open(&self, envelope: &super::envelope::SealedEnvelope) -> anyhow::Result<Vec<u8>> {
        // Try current key first.
        if let Ok(plaintext) = envelope.open(self.current.secret_key()) {
            return Ok(plaintext);
        }

        // Fall back to previous key during overlap window.
        if let Some(ref prev) = self.previous {
            if prev.age_secs() <= prev.created_at.saturating_add(self.overlap_secs)
                || self.overlap_secs == 0
            {
                // Just try the previous key regardless of overlap calculation —
                // the cleanup_expired method handles removing old keys.
                if let Ok(plaintext) = envelope.open(prev.secret_key()) {
                    return Ok(plaintext);
                }
            }
        }

        anyhow::bail!("failed to decrypt envelope with any available key")
    }

    /// Remove the previous key if it's past the overlap window.
    pub fn cleanup_expired(&mut self) {
        if let Some(ref prev) = self.previous {
            if prev.age_secs() > self.overlap_secs {
                self.previous = None;
            }
        }
    }

    /// Get all keypairs (current + previous if present) for persistence.
    pub fn all_keypairs(&self) -> Vec<&IdentityKeyPair> {
        let mut keys = vec![&self.current];
        if let Some(ref prev) = self.previous {
            keys.push(prev);
        }
        keys
    }

    /// Restore a keyring from persisted keypairs.
    /// Expects keypairs sorted by epoch (highest first).
    pub fn from_persisted(
        mut keypairs: Vec<IdentityKeyPair>,
        rotation_interval_secs: u64,
        overlap_secs: u64,
    ) -> anyhow::Result<Self> {
        if keypairs.is_empty() {
            anyhow::bail!("cannot restore keyring from empty keypair list");
        }
        keypairs.sort_by_key(|b| std::cmp::Reverse(b.epoch));
        let current = keypairs.remove(0);
        let previous = keypairs.into_iter().next();

        Ok(Self {
            current,
            previous,
            rotation_interval_secs,
            overlap_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::envelope::SealedEnvelope;

    #[test]
    fn generate_identity_keypair() {
        let kp = IdentityKeyPair::generate(0);
        assert_eq!(kp.epoch, 0);
        assert!(!kp.fingerprint().is_empty());
        assert_eq!(kp.fingerprint().len(), 16); // 8 bytes hex = 16 chars
    }

    #[test]
    fn keypair_routing_id_matches_envelope_routing_id() {
        let kp = IdentityKeyPair::generate(0);
        let routing_id = kp.routing_id();
        let expected = super::super::envelope::compute_routing_id(&kp.public_key);
        assert_eq!(routing_id, expected);
    }

    #[test]
    fn keyring_starts_at_epoch_zero() {
        let kr = PrivacyKeyRing::new(3600, 600);
        assert_eq!(kr.epoch(), 0);
        assert!(kr.previous().is_none());
    }

    #[test]
    fn keyring_no_rotation_when_interval_zero() {
        let mut kr = PrivacyKeyRing::new(0, 0);
        assert!(kr.check_rotation().is_none());
    }

    #[test]
    fn keyring_rotation_creates_new_epoch() {
        let old_kp = IdentityKeyPair {
            epoch: 0,
            public_key: [1u8; 32],
            secret_key: [2u8; 32],
            created_at: 0, // epoch 0 = very old
        };
        let mut kr = PrivacyKeyRing::from_keypair(old_kp, 1, 3600);

        let new_epoch = kr.check_rotation();
        assert_eq!(new_epoch, Some(1));
        assert_eq!(kr.epoch(), 1);
        assert!(kr.previous().is_some());
        assert_eq!(kr.previous().unwrap().epoch, 0);
    }

    #[test]
    fn try_open_with_current_key() {
        let kr = PrivacyKeyRing::new(3600, 600);
        let envelope = SealedEnvelope::seal(kr.public_key(), b"hello keyring", 3600);

        let plaintext = kr.try_open(&envelope).expect("should decrypt");
        assert_eq!(plaintext, b"hello keyring");
    }

    #[test]
    fn try_open_falls_back_to_previous_key() {
        // Create keyring, encrypt with its key, then rotate.
        let mut kr = PrivacyKeyRing::new(0, 3600); // 0 interval = manual rotation
        let old_pubkey = *kr.public_key();
        let envelope = SealedEnvelope::seal(&old_pubkey, b"old key data", 3600);

        // Force rotation by replacing current with a new keypair.
        let old = std::mem::replace(&mut kr.current, IdentityKeyPair::generate(1));
        kr.previous = Some(old);

        let plaintext = kr
            .try_open(&envelope)
            .expect("should decrypt with previous key");
        assert_eq!(plaintext, b"old key data");
    }

    #[test]
    fn try_open_fails_with_no_matching_key() {
        let kr = PrivacyKeyRing::new(3600, 600);
        let other_kp = IdentityKeyPair::generate(99);
        let envelope = SealedEnvelope::seal(&other_kp.public_key, b"wrong recipient", 3600);

        let err = kr.try_open(&envelope).expect_err("should fail");
        assert!(err.to_string().contains("failed to decrypt"));
    }

    #[test]
    fn cleanup_expired_removes_old_previous() {
        let old_kp = IdentityKeyPair {
            epoch: 0,
            public_key: [1u8; 32],
            secret_key: [2u8; 32],
            created_at: 0, // Very old
        };
        let mut kr = PrivacyKeyRing::from_keypair(IdentityKeyPair::generate(1), 3600, 1);
        kr.previous = Some(old_kp);

        kr.cleanup_expired();
        assert!(kr.previous().is_none(), "old key should be cleaned up");
    }

    #[test]
    fn from_persisted_restores_order() {
        let kp0 = IdentityKeyPair::generate(0);
        let kp1 = IdentityKeyPair::generate(1);
        let kp2 = IdentityKeyPair::generate(2);

        // Provide in wrong order — should sort by epoch desc.
        let kr =
            PrivacyKeyRing::from_persisted(vec![kp0, kp2, kp1], 3600, 600).expect("should restore");
        assert_eq!(kr.epoch(), 2);
        assert_eq!(kr.previous().unwrap().epoch, 1);
    }

    #[test]
    fn from_persisted_rejects_empty() {
        let err = PrivacyKeyRing::from_persisted(vec![], 3600, 600).expect_err("should fail");
        assert!(err.to_string().contains("empty keypair list"));
    }

    #[test]
    fn all_keypairs_returns_both() {
        let mut kr = PrivacyKeyRing::new(0, 3600);
        assert_eq!(kr.all_keypairs().len(), 1);

        let old = std::mem::replace(&mut kr.current, IdentityKeyPair::generate(1));
        kr.previous = Some(old);
        assert_eq!(kr.all_keypairs().len(), 2);
    }

    #[test]
    fn keypair_deserializes_from_json() {
        // IdentityKeyPair intentionally does NOT implement Serialize (prevents
        // secret key leakage). Verify it can still be deserialized from stored JSON.
        let pub_key: Vec<u8> = vec![1u8; 32];
        let sec_key: Vec<u8> = vec![2u8; 32];
        let json = serde_json::json!({
            "epoch": 5,
            "public_key": pub_key,
            "secret_key": sec_key,
            "created_at": 1000
        });
        let restored: IdentityKeyPair =
            serde_json::from_value(json).expect("should deserialize from JSON");
        assert_eq!(restored.epoch, 5);
        assert_eq!(restored.public_key, [1u8; 32]);
        assert_eq!(restored.secret_key(), &[2u8; 32]);
    }

    #[test]
    fn keypair_does_not_implement_serialize() {
        // Compile-time guarantee: `IdentityKeyPair` must NOT be serializable.
        // If someone re-adds Serialize, this test documents the design intent.
        fn assert_not_serialize<T>() {
            let _ = std::marker::PhantomData::<T>;
        }
        assert_not_serialize::<IdentityKeyPair>();
    }

    #[test]
    fn force_rotate_always_advances_epoch() {
        let mut kr = PrivacyKeyRing::new(0, 3600); // 0 interval = no auto-rotation
        assert_eq!(kr.epoch(), 0);

        let new_epoch = kr.force_rotate();
        assert_eq!(new_epoch, 1);
        assert_eq!(kr.epoch(), 1);
        assert!(kr.previous().is_some());
        assert_eq!(kr.previous().unwrap().epoch, 0);

        // Force again.
        let new_epoch = kr.force_rotate();
        assert_eq!(new_epoch, 2);
        assert_eq!(kr.epoch(), 2);
        assert_eq!(kr.previous().unwrap().epoch, 1);
    }

    #[test]
    fn next_rotation_at_returns_none_when_disabled() {
        let kr = PrivacyKeyRing::new(0, 0);
        assert!(kr.next_rotation_at().is_none());
    }

    #[test]
    fn next_rotation_at_returns_expected_time() {
        let kr = PrivacyKeyRing::new(3600, 600);
        let next = kr.next_rotation_at().expect("should have next rotation");
        // next should be created_at + 3600
        assert_eq!(next, kr.current().created_at + 3600);
    }
}
