//! Encryption at rest for AgentZero.
//!
//! Provides AES-256-GCM encryption with Argon2id key derivation.
//! Used to protect audit logs, session files, and any sensitive
//! data persisted to disk.
//!
//! Design:
//! - Key derived from passphrase using Argon2id (memory-hard)
//! - Each encryption uses a random 96-bit nonce
//! - Ciphertext format: salt (16) || nonce (12) || ciphertext (variable) || tag (16)
//! - No raw secrets ever written to disk in plaintext

use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, KeyInit};
use argon2::Argon2;
use thiserror::Error;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32; // AES-256

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("key derivation failed: {0}")]
    KeyDerivationFailed(String),
    #[error("invalid ciphertext: {0}")]
    InvalidCiphertext(String),
}

/// Derive an AES-256 key from a passphrase and salt using Argon2id.
fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    Ok(key)
}

/// Encrypt plaintext with a passphrase.
///
/// Returns: salt (16) || nonce (12) || ciphertext || tag (16)
pub fn encrypt(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    let salt: [u8; SALT_LEN] = rand::random();
    let key_bytes = derive_key(passphrase.as_bytes(), &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    let mut output = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt ciphertext with a passphrase.
///
/// Expects format: salt (16) || nonce (12) || ciphertext || tag (16)
pub fn decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    let min_len = SALT_LEN + NONCE_LEN + 16; // at least salt + nonce + tag
    if data.len() < min_len {
        return Err(CryptoError::InvalidCiphertext(format!(
            "data too short: {} bytes, need at least {min_len}",
            data.len()
        )));
    }

    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let key_bytes = derive_key(passphrase.as_bytes(), salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);

    cipher.decrypt(nonce, ciphertext).map_err(|_| {
        CryptoError::DecryptionFailed(
            "decryption failed — wrong passphrase or corrupted data".into(),
        )
    })
}

/// Encrypt a string and return base64-encoded ciphertext.
pub fn encrypt_string(plaintext: &str, passphrase: &str) -> Result<String, CryptoError> {
    let encrypted = encrypt(plaintext.as_bytes(), passphrase)?;
    Ok(base64_encode(&encrypted))
}

/// Decrypt base64-encoded ciphertext to a string.
pub fn decrypt_string(encoded: &str, passphrase: &str) -> Result<String, CryptoError> {
    let data = base64_decode(encoded)?;
    let plaintext = decrypt(&data, passphrase)?;
    String::from_utf8(plaintext)
        .map_err(|e| CryptoError::DecryptionFailed(format!("invalid UTF-8: {e}")))
}

// Simple base64 without pulling in a full crate
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(input: &str) -> Result<Vec<u8>, CryptoError> {
    let input = input.trim_end_matches('=');
    let mut result = Vec::with_capacity(input.len() * 3 / 4);

    let decode_char = |c: u8| -> Result<u32, CryptoError> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(CryptoError::InvalidCiphertext(format!(
                "invalid base64 character: {c}"
            ))),
        }
    };

    let bytes = input.as_bytes();
    let chunks = bytes.chunks(4);
    for chunk in chunks {
        let len = chunk.len();
        let b0 = decode_char(chunk[0])?;
        let b1 = if len > 1 { decode_char(chunk[1])? } else { 0 };
        let b2 = if len > 2 { decode_char(chunk[2])? } else { 0 };
        let b3 = if len > 3 { decode_char(chunk[3])? } else { 0 };
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        result.push(((triple >> 16) & 0xFF) as u8);
        if len > 2 {
            result.push(((triple >> 8) & 0xFF) as u8);
        }
        if len > 3 {
            result.push((triple & 0xFF) as u8);
        }
    }

    Ok(result)
}

/// Encrypt a file at the given path in place.
pub fn encrypt_file(path: &std::path::Path, passphrase: &str) -> Result<(), CryptoError> {
    let plaintext = std::fs::read(path)
        .map_err(|e| CryptoError::EncryptionFailed(format!("failed to read file: {e}")))?;
    let encrypted = encrypt(&plaintext, passphrase)?;
    std::fs::write(path, encrypted)
        .map_err(|e| CryptoError::EncryptionFailed(format!("failed to write file: {e}")))?;
    Ok(())
}

/// Decrypt a file at the given path and return its contents.
pub fn decrypt_file(path: &std::path::Path, passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    let data = std::fs::read(path)
        .map_err(|e| CryptoError::DecryptionFailed(format!("failed to read file: {e}")))?;
    decrypt(&data, passphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let plaintext = b"hello world, this is secret data";
        let passphrase = "test-passphrase-123";

        let encrypted = encrypt(plaintext, passphrase).expect("encryption should succeed");
        assert_ne!(encrypted, plaintext);
        assert!(encrypted.len() > plaintext.len()); // salt + nonce + tag overhead

        let decrypted = decrypt(&encrypted, passphrase).expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let plaintext = b"secret";
        let encrypted = encrypt(plaintext, "correct").expect("encryption should succeed");
        let result = decrypt(&encrypted, "wrong");
        assert!(result.is_err());
    }

    #[test]
    fn each_encryption_is_unique() {
        let plaintext = b"same data";
        let passphrase = "same-pass";
        let e1 = encrypt(plaintext, passphrase).expect("should succeed");
        let e2 = encrypt(plaintext, passphrase).expect("should succeed");
        assert_ne!(e1, e2); // random salt + nonce
    }

    #[test]
    fn empty_plaintext() {
        let encrypted = encrypt(b"", "pass").expect("should succeed");
        let decrypted = decrypt(&encrypted, "pass").expect("should succeed");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn short_ciphertext_rejected() {
        let result = decrypt(&[0u8; 10], "pass");
        assert!(result.is_err());
    }

    #[test]
    fn string_roundtrip() {
        let plaintext = "sensitive audit log entry with PII";
        let passphrase = "vault-key";
        let encoded = encrypt_string(plaintext, passphrase).expect("should succeed");
        let decoded = decrypt_string(&encoded, passphrase).expect("should succeed");
        assert_eq!(decoded, plaintext);
    }

    #[test]
    fn base64_roundtrip() {
        let data = vec![0u8, 1, 2, 255, 254, 253, 100, 200];
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).expect("should decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn file_encrypt_decrypt() {
        let dir = std::env::temp_dir().join(format!(
            "agentzero-crypto-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("should create dir");
        let file = dir.join("test.enc");

        let plaintext = b"this is a secret audit log";
        std::fs::write(&file, plaintext).expect("should write");

        encrypt_file(&file, "my-passphrase").expect("encryption should succeed");
        let on_disk = std::fs::read(&file).expect("should read");
        assert_ne!(on_disk, plaintext);

        let decrypted = decrypt_file(&file, "my-passphrase").expect("decryption should succeed");
        assert_eq!(decrypted, plaintext);

        std::fs::remove_dir_all(&dir).ok();
    }
}
