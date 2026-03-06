//! Persistent storage for privacy keyring identity keypairs.
//!
//! Wraps `EncryptedJsonStore` to persist `IdentityKeyPair`s encrypted at rest
//! using the existing `StorageKey` infrastructure.

use crate::EncryptedJsonStore;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

const KEYRING_FILE: &str = "privacy-keyring.enc";

/// Tuple of `(epoch, public_key, secret_key, created_at)` for a single persisted keypair.
pub type KeyPairTuple = (u64, [u8; 32], [u8; 32], u64);

/// Persisted form of the privacy keyring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedKeyRing {
    keypairs: Vec<PersistedKeyPair>,
}

/// Serializable keypair (mirrors `IdentityKeyPair` from agentzero-core).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedKeyPair {
    epoch: u64,
    public_key: [u8; 32],
    secret_key: [u8; 32],
    created_at: u64,
}

/// Encrypted keyring store for privacy identity keypairs.
pub struct KeyRingStore {
    store: EncryptedJsonStore,
}

impl KeyRingStore {
    /// Open (or create) a keyring store in the given data directory.
    pub fn in_data_dir(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, KEYRING_FILE)
            .context("failed to initialize keyring store")?;
        Ok(Self { store })
    }

    /// Open a keyring store using the default data directory.
    pub fn in_default_data_dir() -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_default_data_dir(KEYRING_FILE)
            .context("failed to initialize keyring store in default dir")?;
        Ok(Self { store })
    }

    /// Save a list of keypairs (replaces any existing keyring).
    pub fn save_keypairs(&self, keypairs: &[KeyPairTuple]) -> anyhow::Result<()> {
        let persisted = PersistedKeyRing {
            keypairs: keypairs
                .iter()
                .map(
                    |(epoch, public_key, secret_key, created_at)| PersistedKeyPair {
                        epoch: *epoch,
                        public_key: *public_key,
                        secret_key: *secret_key,
                        created_at: *created_at,
                    },
                )
                .collect(),
        };
        self.store
            .save(&persisted)
            .context("failed to save keyring")
    }

    /// Load all persisted keypairs. Returns `(epoch, public_key, secret_key, created_at)` tuples.
    /// Returns an empty list if no keyring file exists.
    pub fn load_keypairs(&self) -> anyhow::Result<Vec<KeyPairTuple>> {
        let persisted: Option<PersistedKeyRing> = self
            .store
            .load_optional()
            .context("failed to load keyring")?;

        Ok(persisted
            .map(|kr| {
                kr.keypairs
                    .into_iter()
                    .map(|kp| (kp.epoch, kp.public_key, kp.secret_key, kp.created_at))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// List all stored epoch numbers.
    pub fn list_epochs(&self) -> anyhow::Result<Vec<u64>> {
        Ok(self
            .load_keypairs()?
            .iter()
            .map(|(epoch, _, _, _)| *epoch)
            .collect())
    }

    /// Remove all keypairs with epoch strictly less than `min_epoch`.
    pub fn prune_before_epoch(&self, min_epoch: u64) -> anyhow::Result<usize> {
        let keypairs = self.load_keypairs()?;
        let before_count = keypairs.len();
        let remaining: Vec<_> = keypairs
            .into_iter()
            .filter(|(epoch, _, _, _)| *epoch >= min_epoch)
            .collect();
        let pruned = before_count - remaining.len();
        if pruned > 0 {
            self.save_keypairs(&remaining)?;
        }
        Ok(pruned)
    }

    /// Delete the keyring file entirely.
    pub fn delete(&self) -> anyhow::Result<()> {
        self.store.delete().context("failed to delete keyring")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir() -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-keyring-{}-{now}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let keypairs = vec![
            (0u64, [1u8; 32], [2u8; 32], 1000u64),
            (1, [3u8; 32], [4u8; 32], 2000),
        ];
        store.save_keypairs(&keypairs).expect("save should succeed");

        let loaded = store.load_keypairs().expect("load should succeed");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].0, 0); // epoch
        assert_eq!(loaded[0].1, [1u8; 32]); // public_key
        assert_eq!(loaded[1].0, 1);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn load_returns_empty_when_no_file() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let loaded = store.load_keypairs().expect("load should succeed");
        assert!(loaded.is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn list_epochs_returns_all_epochs() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let keypairs = vec![
            (5u64, [1u8; 32], [2u8; 32], 1000u64),
            (10, [3u8; 32], [4u8; 32], 2000),
            (15, [5u8; 32], [6u8; 32], 3000),
        ];
        store.save_keypairs(&keypairs).expect("save should succeed");

        let epochs = store.list_epochs().expect("list should succeed");
        assert_eq!(epochs, vec![5, 10, 15]);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn prune_before_epoch_removes_old_keys() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let keypairs = vec![
            (0u64, [1u8; 32], [2u8; 32], 1000u64),
            (1, [3u8; 32], [4u8; 32], 2000),
            (2, [5u8; 32], [6u8; 32], 3000),
        ];
        store.save_keypairs(&keypairs).expect("save should succeed");

        let pruned = store.prune_before_epoch(2).expect("prune should succeed");
        assert_eq!(pruned, 2);

        let remaining = store.load_keypairs().expect("load should succeed");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0, 2);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn prune_noop_when_nothing_to_prune() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let keypairs = vec![(5u64, [1u8; 32], [2u8; 32], 1000u64)];
        store.save_keypairs(&keypairs).expect("save should succeed");

        let pruned = store.prune_before_epoch(0).expect("prune should succeed");
        assert_eq!(pruned, 0);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn delete_removes_keyring_file() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        store
            .save_keypairs(&[(0, [1u8; 32], [2u8; 32], 1000)])
            .expect("save should succeed");

        store.delete().expect("delete should succeed");

        let loaded = store.load_keypairs().expect("load should succeed");
        assert!(loaded.is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keyring_data_is_encrypted_on_disk() {
        let dir = unique_temp_dir();
        let store = KeyRingStore::in_data_dir(&dir).expect("store should open");

        let keypairs = vec![(42u64, [0xABu8; 32], [0xCDu8; 32], 9999u64)];
        store.save_keypairs(&keypairs).expect("save should succeed");

        // Read raw file — should NOT contain plaintext JSON keys.
        let raw = fs::read_to_string(dir.join(KEYRING_FILE)).expect("file should exist");
        assert!(!raw.contains("\"epoch\""), "raw file should be encrypted");
        assert!(
            !raw.contains("\"keypairs\""),
            "keypairs key should not be visible in encrypted output"
        );
        assert!(
            !raw.contains("\"public_key\""),
            "public_key key should not be visible in encrypted output"
        );

        fs::remove_dir_all(dir).ok();
    }
}
