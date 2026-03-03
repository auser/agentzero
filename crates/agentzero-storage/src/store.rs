use agentzero_crypto::{decrypt_json, encrypt_json, StorageKey};
use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct EncryptedJsonStore {
    path: PathBuf,
    key: StorageKey,
}

impl EncryptedJsonStore {
    pub fn in_default_data_dir(file_name: &str) -> anyhow::Result<Self> {
        let data_dir = agentzero_common::paths::default_data_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "failed to determine default data directory; set {}",
                agentzero_common::paths::ENV_DATA_DIR
            )
        })?;
        Self::in_config_dir(&data_dir, file_name)
    }

    pub fn in_config_dir(config_dir: &Path, file_name: &str) -> anyhow::Result<Self> {
        let key = StorageKey::from_config_dir(config_dir)?;
        Ok(Self {
            path: config_dir.join(file_name),
            key,
        })
    }

    pub fn new(path: PathBuf, key: StorageKey) -> Self {
        Self { path, key }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_optional<T>(&self) -> anyhow::Result<Option<T>>
    where
        T: DeserializeOwned + Serialize,
    {
        if !self.path.exists() {
            return Ok(None);
        }

        let raw = fs::read(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;

        match decrypt_json(self.key.as_bytes(), &raw) {
            Ok(decrypted) => {
                let value = serde_json::from_slice(&decrypted)
                    .with_context(|| format!("failed to parse {}", self.path.display()))?;
                Ok(Some(value))
            }
            Err(_) => {
                let plaintext: T = serde_json::from_slice(&raw).with_context(|| {
                    format!(
                        "failed to decrypt or parse legacy plaintext {}",
                        self.path.display()
                    )
                })?;
                self.save(&plaintext)?;
                Ok(Some(plaintext))
            }
        }
    }

    pub fn load_or_default<T>(&self) -> anyhow::Result<T>
    where
        T: DeserializeOwned + Serialize + Default,
    {
        Ok(self.load_optional()?.unwrap_or_default())
    }

    pub fn save<T>(&self, value: &T) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let serialized = serde_json::to_vec(value).context("failed to serialize JSON payload")?;
        let encrypted = encrypt_json(self.key.as_bytes(), &serialized)?;
        let temp_path = self.temporary_path();
        fs::write(&temp_path, encrypted)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        self.enforce_private_permissions(&temp_path)?;
        fs::rename(&temp_path, &self.path).with_context(|| {
            format!(
                "failed to atomically replace {} with {}",
                self.path.display(),
                temp_path.display()
            )
        })?;
        self.enforce_private_permissions(&self.path)?;
        Ok(())
    }

    pub fn delete(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .with_context(|| format!("failed to remove {}", self.path.display()))?;
        }
        Ok(())
    }

    fn temporary_path(&self) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("agentzero-store.json");
        self.path
            .with_file_name(format!(".{file_name}.tmp.{stamp}"))
    }

    fn enforce_private_permissions(&self, _path: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(_path, fs::Permissions::from_mode(0o600))
                .with_context(|| format!("failed to chmod {}", _path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EncryptedJsonStore;
    use agentzero_crypto::StorageKey;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
    struct TestData {
        value: String,
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-store-{}-{now}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn save_and_load_round_trip_success_path() {
        let dir = unique_temp_dir();
        let store =
            EncryptedJsonStore::in_config_dir(&dir, "test.json").expect("store should construct");
        let data = TestData {
            value: "secret".to_string(),
        };

        store.save(&data).expect("save should succeed");
        let loaded: TestData = store
            .load_optional()
            .expect("load should succeed")
            .expect("value should exist");
        assert_eq!(loaded, data);

        let disk = fs::read_to_string(store.path()).expect("stored payload should be readable");
        assert!(!disk.contains("secret"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn load_legacy_plaintext_migrates_to_encrypted_success_path() {
        let dir = unique_temp_dir();
        let store =
            EncryptedJsonStore::in_config_dir(&dir, "legacy.json").expect("store should construct");
        let plaintext = r#"{"value":"plain"}"#;
        fs::write(store.path(), plaintext).expect("legacy plaintext should be written");

        let loaded: TestData = store
            .load_optional()
            .expect("load should succeed")
            .expect("value should exist");
        assert_eq!(
            loaded,
            TestData {
                value: "plain".to_string()
            }
        );

        let disk = fs::read_to_string(store.path()).expect("migrated payload should be readable");
        assert!(!disk.contains("\"plain\""));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn load_optional_returns_none_for_missing_file_negative_path() {
        let dir = unique_temp_dir();
        let key = StorageKey::from_config_dir(&dir).expect("key should load");
        let store = EncryptedJsonStore::new(dir.join("missing.json"), key);

        let loaded: Option<TestData> = store.load_optional().expect("load should succeed");
        assert!(loaded.is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
