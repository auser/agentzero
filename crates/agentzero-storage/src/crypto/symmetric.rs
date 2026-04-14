use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

const ENVELOPE_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Envelope {
    v: u8,
    nonce: String,
    ciphertext: String,
}

pub fn encrypt_json(key_bytes: [u8; 32], plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| anyhow!("failed to encrypt payload"))?;

    let envelope = Envelope {
        v: ENVELOPE_VERSION,
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };

    serde_json::to_vec_pretty(&envelope).context("failed to serialize encrypted envelope")
}

pub fn decrypt_json(key_bytes: [u8; 32], payload: &[u8]) -> anyhow::Result<Vec<u8>> {
    let envelope: Envelope =
        serde_json::from_slice(payload).context("failed to parse encrypted envelope")?;
    if envelope.v != ENVELOPE_VERSION {
        return Err(anyhow!(
            "unsupported encrypted envelope version {}",
            envelope.v
        ));
    }

    let nonce = STANDARD
        .decode(envelope.nonce.as_bytes())
        .context("failed to decode encrypted nonce")?;
    if nonce.len() != 24 {
        return Err(anyhow!("invalid encrypted nonce length"));
    }
    let ciphertext = STANDARD
        .decode(envelope.ciphertext.as_bytes())
        .context("failed to decode encrypted payload")?;

    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key_bytes));
    cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("failed to decrypt payload"))
}

#[cfg(test)]
mod tests {
    use super::{decrypt_json, encrypt_json};

    #[test]
    fn encrypt_and_decrypt_round_trip_success_path() {
        let key = [7_u8; 32];
        let plaintext = br#"{"secret":"token"}"#;
        let encrypted = encrypt_json(key, plaintext).expect("encryption should succeed");
        let decrypted = decrypt_json(key, &encrypted).expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_fails_with_wrong_key_negative_path() {
        let plaintext = br#"{"secret":"token"}"#;
        let encrypted =
            encrypt_json([1_u8; 32], plaintext).expect("encryption should succeed for setup");
        let err =
            decrypt_json([2_u8; 32], &encrypted).expect_err("wrong key should fail decryption");
        assert!(err.to_string().contains("failed to decrypt"));
    }

    #[test]
    fn empty_plaintext_round_trip() {
        let key = [42_u8; 32];
        let encrypted = encrypt_json(key, b"").expect("encrypt empty");
        let decrypted = decrypt_json(key, &encrypted).expect("decrypt empty");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn truncated_ciphertext_fails() {
        let key = [3_u8; 32];
        let encrypted = encrypt_json(key, b"data").expect("encrypt");
        // Truncate the JSON envelope.
        let truncated = &encrypted[..encrypted.len() / 2];
        assert!(decrypt_json(key, truncated).is_err());
    }

    #[test]
    fn invalid_json_envelope_fails() {
        let key = [4_u8; 32];
        let err = decrypt_json(key, b"not-json").expect_err("should fail");
        assert!(err.to_string().contains("failed to parse"));
    }

    #[test]
    fn two_encryptions_produce_different_ciphertexts() {
        let key = [5_u8; 32];
        let plaintext = b"same data";
        let a = encrypt_json(key, plaintext).expect("encrypt a");
        let b = encrypt_json(key, plaintext).expect("encrypt b");
        assert_ne!(a, b, "random nonce should produce different ciphertexts");
        // Both should decrypt to the same plaintext.
        assert_eq!(
            decrypt_json(key, &a).unwrap(),
            decrypt_json(key, &b).unwrap()
        );
    }

    // ── Property-based tests ──────────────────────────────────────────
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// encrypt → decrypt is always the identity for any plaintext
            /// and any valid key.
            #[test]
            fn round_trip_for_arbitrary_plaintext(
                plaintext in proptest::collection::vec(any::<u8>(), 0..1024),
                key in proptest::collection::vec(any::<u8>(), 32..=32)
            ) {
                let key_arr: [u8; 32] = key.try_into().expect("key must be 32 bytes");
                let encrypted = encrypt_json(key_arr, &plaintext).expect("encryption must succeed");
                let decrypted = decrypt_json(key_arr, &encrypted).expect("decryption must succeed");
                prop_assert_eq!(plaintext, decrypted, "round-trip failed");
            }

            /// A different key must always fail to decrypt.
            #[test]
            fn wrong_key_always_fails(
                plaintext in proptest::collection::vec(any::<u8>(), 1..256),
                key1 in proptest::collection::vec(any::<u8>(), 32..=32),
                key2 in proptest::collection::vec(any::<u8>(), 32..=32)
            ) {
                let k1: [u8; 32] = key1.try_into().expect("key1");
                let k2: [u8; 32] = key2.try_into().expect("key2");
                prop_assume!(k1 != k2);
                let encrypted = encrypt_json(k1, &plaintext).expect("encrypt");
                prop_assert!(decrypt_json(k2, &encrypted).is_err(), "wrong key should fail");
            }

            /// Same plaintext + same key always produces different ciphertext
            /// (nonce randomization).
            #[test]
            fn nonce_randomization(
                plaintext in proptest::collection::vec(any::<u8>(), 1..128),
                key in proptest::collection::vec(any::<u8>(), 32..=32)
            ) {
                let k: [u8; 32] = key.try_into().expect("key");
                let a = encrypt_json(k, &plaintext).expect("encrypt a");
                let b = encrypt_json(k, &plaintext).expect("encrypt b");
                prop_assert_ne!(a, b, "same input must produce different ciphertexts");
            }
        }
    }
}
