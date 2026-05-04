//! Encrypted secret vault for AgentZero.
//!
//! Stores secret values encrypted at rest per ADR 0009. The model only
//! sees handles (`handle://vault/<provider>/<name>`), never raw values.
//! Tools receive raw material only at execution time after policy approval.

use std::path::{Path, PathBuf};

use crate::crypto;
use crate::secret::SecretHandle;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault not initialized: {0}")]
    NotInitialized(String),
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("vault IO error: {0}")]
    IoError(String),
    #[error("vault crypto error: {0}")]
    CryptoError(String),
}

/// Encrypted on-disk secret vault.
///
/// Each secret is stored as an individual encrypted file:
/// `.agentzero/vault/<provider>/<name>.enc`
pub struct Vault {
    root: PathBuf,
    passphrase: String,
}

impl Vault {
    /// Open or create a vault at the given directory.
    pub fn open(root: &Path, passphrase: &str) -> Result<Self, VaultError> {
        std::fs::create_dir_all(root)
            .map_err(|e| VaultError::NotInitialized(format!("cannot create vault dir: {e}")))?;
        Ok(Self {
            root: root.to_path_buf(),
            passphrase: passphrase.to_string(),
        })
    }

    /// Store a secret value under a handle.
    pub fn put(&self, handle: &SecretHandle, value: &str) -> Result<(), VaultError> {
        let provider_dir = self.root.join(handle.provider());
        std::fs::create_dir_all(&provider_dir)
            .map_err(|e| VaultError::IoError(format!("cannot create provider dir: {e}")))?;

        let file_path = provider_dir.join(format!("{}.enc", handle.name()));
        let encrypted = crypto::encrypt(value.as_bytes(), &self.passphrase)
            .map_err(|e| VaultError::CryptoError(e.to_string()))?;

        std::fs::write(&file_path, encrypted)
            .map_err(|e| VaultError::IoError(format!("cannot write secret: {e}")))?;
        Ok(())
    }

    /// Retrieve and decrypt a secret value by handle.
    pub fn get(&self, handle: &SecretHandle) -> Result<String, VaultError> {
        let file_path = self
            .root
            .join(handle.provider())
            .join(format!("{}.enc", handle.name()));

        if !file_path.exists() {
            return Err(VaultError::NotFound(handle.uri()));
        }

        let data = std::fs::read(&file_path)
            .map_err(|e| VaultError::IoError(format!("cannot read secret: {e}")))?;

        let decrypted = crypto::decrypt(&data, &self.passphrase)
            .map_err(|e| VaultError::CryptoError(e.to_string()))?;

        String::from_utf8(decrypted).map_err(|e| VaultError::CryptoError(e.to_string()))
    }

    /// Remove a secret.
    pub fn remove(&self, handle: &SecretHandle) -> Result<(), VaultError> {
        let file_path = self
            .root
            .join(handle.provider())
            .join(format!("{}.enc", handle.name()));

        if file_path.exists() {
            std::fs::remove_file(&file_path)
                .map_err(|e| VaultError::IoError(format!("cannot remove secret: {e}")))?;
        }
        Ok(())
    }

    /// List all stored handles.
    pub fn list(&self) -> Result<Vec<SecretHandle>, VaultError> {
        let mut handles = Vec::new();

        let entries = std::fs::read_dir(&self.root)
            .map_err(|e| VaultError::IoError(format!("cannot read vault: {e}")))?;

        for entry in entries.flatten() {
            let provider_path = entry.path();
            if !provider_path.is_dir() {
                continue;
            }
            let provider = entry.file_name().to_string_lossy().to_string();

            let secrets = std::fs::read_dir(&provider_path)
                .map_err(|e| VaultError::IoError(format!("cannot read provider dir: {e}")))?;

            for secret_entry in secrets.flatten() {
                let name = secret_entry.file_name().to_string_lossy().to_string();
                if let Some(stripped) = name.strip_suffix(".enc") {
                    handles.push(SecretHandle::new(&provider, stripped));
                }
            }
        }

        handles.sort_by_key(|h| h.uri());
        Ok(handles)
    }
}

/// Resolve a handle to its raw value for tool execution.
///
/// This is the only path where raw secret material is exposed.
/// Callers MUST have policy approval before calling this.
pub fn resolve_for_execution(
    vault: &Vault,
    handle: &SecretHandle,
) -> Result<crate::secret::ResolvedSecret, VaultError> {
    let value = vault.get(handle)?;
    Ok(crate::secret::ResolvedSecret::new(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-vault-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn put_and_get_secret() {
        let dir = temp_dir("put-get");
        let vault = Vault::open(&dir, "test-pass").expect("should open");
        let handle = SecretHandle::new("github", "token");

        vault.put(&handle, "ghp_secret123").expect("should store");
        let value = vault.get(&handle).expect("should retrieve");
        assert_eq!(value, "ghp_secret123");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn secret_encrypted_on_disk() {
        let dir = temp_dir("encrypted-disk");
        let vault = Vault::open(&dir, "pass").expect("should open");
        let handle = SecretHandle::new("aws", "key");

        vault
            .put(&handle, "AKIAIOSFODNN7EXAMPLE")
            .expect("should store");

        let file_path = dir.join("aws/key.enc");
        let raw = std::fs::read(&file_path).expect("should read file");
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(!raw_str.contains("AKIAIOSFODNN7EXAMPLE"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_passphrase_fails() {
        let dir = temp_dir("wrong-pass");
        let vault = Vault::open(&dir, "correct").expect("should open");
        let handle = SecretHandle::new("test", "secret");
        vault.put(&handle, "value").expect("should store");

        let bad_vault = Vault::open(&dir, "wrong").expect("should open");
        assert!(bad_vault.get(&handle).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_secrets() {
        let dir = temp_dir("list");
        let vault = Vault::open(&dir, "pass").expect("should open");

        vault
            .put(&SecretHandle::new("github", "token"), "abc")
            .expect("should store");
        vault
            .put(&SecretHandle::new("aws", "key"), "def")
            .expect("should store");
        vault
            .put(&SecretHandle::new("github", "webhook"), "ghi")
            .expect("should store");

        let handles = vault.list().expect("should list");
        assert_eq!(handles.len(), 3);
        // Sorted by URI
        assert_eq!(handles[0].provider(), "aws");
        assert_eq!(handles[1].provider(), "github");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_secret() {
        let dir = temp_dir("remove");
        let vault = Vault::open(&dir, "pass").expect("should open");
        let handle = SecretHandle::new("test", "key");

        vault.put(&handle, "value").expect("should store");
        assert!(vault.get(&handle).is_ok());

        vault.remove(&handle).expect("should remove");
        assert!(vault.get(&handle).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn not_found_error() {
        let dir = temp_dir("not-found");
        let vault = Vault::open(&dir, "pass").expect("should open");
        let handle = SecretHandle::new("nonexistent", "key");
        assert!(vault.get(&handle).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_for_execution_works() {
        let dir = temp_dir("resolve");
        let vault = Vault::open(&dir, "pass").expect("should open");
        let handle = SecretHandle::new("github", "token");
        vault.put(&handle, "secret-val").expect("should store");

        let resolved = resolve_for_execution(&vault, &handle).expect("should resolve");
        assert_eq!(resolved.expose(), "secret-val");

        std::fs::remove_dir_all(&dir).ok();
    }
}
