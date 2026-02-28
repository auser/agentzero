use agentzero_storage::EncryptedJsonStore;
use std::collections::HashSet;
use std::path::Path;

pub(crate) fn load_paired_tokens(path: Option<&Path>) -> anyhow::Result<HashSet<String>> {
    let Some(path) = path else {
        return Ok(HashSet::new());
    };
    let store = store_for(path)?;
    let loaded: Option<Vec<String>> = store.load_optional()?;
    Ok(loaded
        .unwrap_or_default()
        .into_iter()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .collect())
}

pub(crate) fn save_paired_tokens(
    path: Option<&Path>,
    tokens: &HashSet<String>,
) -> anyhow::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let store = store_for(path)?;
    let mut serialized = tokens.iter().cloned().collect::<Vec<_>>();
    serialized.sort_unstable();
    store.save(&serialized)
}

pub(crate) fn clear_paired_tokens(path: Option<&Path>) -> anyhow::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let store = store_for(path)?;
    store.delete()
}

fn store_for(path: &Path) -> anyhow::Result<EncryptedJsonStore> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "failed to resolve paired token file name from path {}",
                path.display()
            )
        })?;
    EncryptedJsonStore::in_config_dir(parent, file_name)
}

#[cfg(test)]
mod tests {
    use super::{clear_paired_tokens, load_paired_tokens, save_paired_tokens};
    use std::collections::HashSet;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_file(name: &str) -> std::path::PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agentzero-gateway-store-{name}-{now}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir.join("tokens.json")
    }

    #[test]
    fn load_paired_tokens_returns_empty_when_file_is_missing_negative_path() {
        let path = unique_temp_file("missing");
        let loaded = load_paired_tokens(Some(&path)).expect("missing token store should be ok");
        assert!(loaded.is_empty());
        fs::remove_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be removed");
    }

    #[test]
    fn save_and_load_paired_tokens_round_trip_success_path() {
        let path = unique_temp_file("roundtrip");
        let mut tokens = HashSet::new();
        tokens.insert("token-1".to_string());
        tokens.insert("token-2".to_string());

        save_paired_tokens(Some(&path), &tokens).expect("save should succeed");
        let loaded = load_paired_tokens(Some(&path)).expect("load should succeed");
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains("token-1"));
        assert!(loaded.contains("token-2"));

        let disk = fs::read_to_string(&path).expect("persisted payload should be readable");
        assert!(!disk.contains("token-1"));
        assert!(!disk.contains("token-2"));

        clear_paired_tokens(Some(&path)).expect("cleanup should succeed");
        fs::remove_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should be removed");
    }
}
