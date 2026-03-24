use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::RngCore;
use std::fs;
use std::path::Path;

const KEY_FILE_NAME: &str = ".agentzero-data.key";
const KEY_ENV_NAME: &str = "AGENTZERO_DATA_KEY";

#[derive(Debug, Clone, Copy)]
pub struct StorageKey {
    bytes: [u8; 32],
}

impl StorageKey {
    pub fn from_config_dir(config_dir: &Path) -> anyhow::Result<Self> {
        if let Ok(raw) = std::env::var(KEY_ENV_NAME) {
            return parse_storage_key(raw.trim())
                .context("failed to parse AGENTZERO_DATA_KEY as base64 or 64-char hex");
        }

        load_or_create_key_file(config_dir)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(self) -> [u8; 32] {
        self.bytes
    }
}

fn load_or_create_key_file(config_dir: &Path) -> anyhow::Result<StorageKey> {
    let key_path = config_dir.join(KEY_FILE_NAME);
    if key_path.exists() {
        let raw = fs::read_to_string(&key_path)
            .with_context(|| format!("failed to read key file {}", key_path.display()))?;
        return parse_storage_key(raw.trim())
            .with_context(|| format!("failed to parse key file {}", key_path.display()));
    }

    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut key = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    let encoded = STANDARD.encode(key);
    fs::write(&key_path, encoded)
        .with_context(|| format!("failed to write key file {}", key_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to chmod key file {}", key_path.display()))?;
    }
    Ok(StorageKey { bytes: key })
}

fn parse_storage_key(raw: &str) -> anyhow::Result<StorageKey> {
    if raw.is_empty() {
        return Err(anyhow!("storage key must not be empty"));
    }

    if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut bytes = [0_u8; 32];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            let start = idx * 2;
            let value = u8::from_str_radix(&raw[start..start + 2], 16)
                .context("failed to decode hex storage key")?;
            *slot = value;
        }
        return Ok(StorageKey { bytes });
    }

    let decoded = STANDARD
        .decode(raw.as_bytes())
        .context("failed to decode base64 storage key")?;
    if decoded.len() != 32 {
        return Err(anyhow!(
            "storage key must be 32 bytes (got {} bytes)",
            decoded.len()
        ));
    }
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&decoded);
    Ok(StorageKey { bytes })
}

#[cfg(test)]
pub fn key_file_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(KEY_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::{key_file_path, StorageKey};
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir() -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("agentzero-crypto-{}-{now}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn loads_existing_key_file_success_path() {
        let dir = unique_temp_dir();
        let encoded = STANDARD.encode([42_u8; 32]);
        fs::write(key_file_path(&dir), encoded).expect("key file should be written");

        let key = StorageKey::from_config_dir(&dir).expect("key should load");
        assert_eq!(key.as_bytes(), [42_u8; 32]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_invalid_key_file_negative_path() {
        let dir = unique_temp_dir();
        fs::write(key_file_path(&dir), "bad-key").expect("invalid key file should be written");
        let err =
            StorageKey::from_config_dir(&dir).expect_err("invalid key file should return error");
        assert!(err.to_string().contains("failed to parse key file"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn valid_64_char_hex_accepted() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let dir = unique_temp_dir();
        fs::write(key_file_path(&dir), hex_key).expect("write");
        let key = StorageKey::from_config_dir(&dir).expect("hex key should be accepted");
        assert_eq!(key.as_bytes()[0], 0x01);
        assert_eq!(key.as_bytes()[1], 0x23);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_key_string_rejected() {
        let dir = unique_temp_dir();
        fs::write(key_file_path(&dir), "").expect("write");
        let err = StorageKey::from_config_dir(&dir).expect_err("empty should fail");
        let chain = format!("{:#}", err);
        assert!(
            chain.contains("empty"),
            "error chain should mention empty: {chain}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn wrong_length_base64_rejected() {
        let dir = unique_temp_dir();
        // 16 bytes encoded as base64 (not 32 bytes).
        let short = STANDARD.encode([1_u8; 16]);
        fs::write(key_file_path(&dir), short).expect("write");
        let err = StorageKey::from_config_dir(&dir).expect_err("wrong length should fail");
        let chain = format!("{:#}", err);
        assert!(
            chain.contains("32 bytes"),
            "error chain should mention 32 bytes: {chain}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn from_config_dir_creates_key_file_if_missing() {
        // Skip if AGENTZERO_DATA_KEY env var is set (from_config_dir reads
        // the env var first, bypassing file creation — causes flaky failures
        // when running in parallel with tests that set the env var).
        if std::env::var(super::KEY_ENV_NAME).is_ok() {
            eprintln!("skipping: {} env var is set", super::KEY_ENV_NAME);
            return;
        }

        let dir = unique_temp_dir();
        let kf = key_file_path(&dir);
        assert!(!kf.exists());

        let key = StorageKey::from_config_dir(&dir).expect("should create key");
        assert!(kf.exists(), "key file should have been created");
        // Key should be 32 bytes.
        assert_eq!(key.as_bytes().len(), 32);

        // Re-load should return the same key.
        let key2 = StorageKey::from_config_dir(&dir).expect("reload");
        assert_eq!(key.as_bytes(), key2.as_bytes());
        fs::remove_dir_all(dir).ok();
    }
}
