use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

const ENVELOPE_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Envelope {
    v: u8,
    nonce: String,
    ciphertext: String,
}

pub(crate) fn encrypt_json(key_bytes: [u8; 32], plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
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

pub(crate) fn decrypt_json(key_bytes: [u8; 32], payload: &[u8]) -> anyhow::Result<Vec<u8>> {
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
}
