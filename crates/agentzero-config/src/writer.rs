//! Safe config writer with backup-before-write and validation.
//!
//! Shared by the gateway `PUT /v1/config` handler and the `config_manage` tool.

use crate::model::AgentZeroConfig;
use anyhow::{bail, Context};
use std::path::{Path, PathBuf};

/// Information about a config backup file.
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub timestamp: String,
}

/// Convert a `serde_json::Value` to a `toml::Value`, skipping null entries.
pub fn json_value_to_toml(v: &serde_json::Value) -> Result<toml::Value, String> {
    match v {
        serde_json::Value::Null => Err("null values not supported in TOML".to_string()),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(format!("unsupported number: {n}"))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<toml::Value>, String> = arr
                .iter()
                .filter(|v| !v.is_null())
                .map(json_value_to_toml)
                .collect();
            Ok(toml::Value::Array(items?))
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (k, v) in map {
                if v.is_null() {
                    continue;
                }
                table.insert(k.clone(), json_value_to_toml(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

/// A key-value section to merge into the config.
pub struct ConfigSection {
    pub key: String,
    pub value: serde_json::Value,
}

/// Read the existing TOML config, merge the provided sections, validate, and
/// return the merged TOML string + parsed config.  Does NOT write to disk.
pub fn read_and_merge(
    config_path: &Path,
    sections: &[ConfigSection],
) -> anyhow::Result<(String, AgentZeroConfig)> {
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let mut doc: toml::Table =
        toml::from_str(&content).context("failed to parse existing config")?;

    for section in sections {
        let toml_val = json_value_to_toml(&section.value)
            .map_err(|e| anyhow::anyhow!("invalid value for section '{}': {e}", section.key))?;
        doc.insert(section.key.clone(), toml_val);
    }

    let merged_str = toml::to_string_pretty(&doc).context("failed to serialize config")?;
    let merged_cfg: AgentZeroConfig =
        toml::from_str(&merged_str).context("invalid config after merge")?;
    merged_cfg.validate().context("config validation failed")?;

    Ok((merged_str, merged_cfg))
}

/// Read a specific section from the config, returning it as JSON.
pub fn read_section(
    config_path: &Path,
    section: Option<&str>,
) -> anyhow::Result<serde_json::Value> {
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let doc: toml::Table = toml::from_str(&content).context("failed to parse existing config")?;

    match section {
        Some(key) => {
            let val = doc
                .get(key)
                .with_context(|| format!("section '{key}' not found in config"))?;
            let json_str = serde_json::to_string(val).context("failed to convert to JSON")?;
            serde_json::from_str(&json_str).context("failed to parse JSON")
        }
        None => {
            let json_str = serde_json::to_string(&doc).context("failed to convert to JSON")?;
            serde_json::from_str(&json_str).context("failed to parse JSON")
        }
    }
}

/// Create a timestamped backup of the config file, then write the new content.
/// Returns the backup path.  Prunes old backups beyond `max_backups`.
pub fn write_with_backup(
    config_path: &Path,
    content: &str,
    max_backups: usize,
) -> anyhow::Result<Option<PathBuf>> {
    let backup_path = if config_path.exists() {
        let timestamp = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!("{secs}")
        };
        let backup = config_path.with_extension(format!("toml.bak.{timestamp}"));
        std::fs::copy(config_path, &backup)
            .with_context(|| format!("failed to create backup at {}", backup.display()))?;
        prune_backups(config_path, max_backups);
        Some(backup)
    } else {
        None
    };

    std::fs::write(config_path, content).context("failed to write config file")?;

    Ok(backup_path)
}

/// List available backup files for the given config path.
pub fn list_backups(config_path: &Path) -> anyhow::Result<Vec<BackupInfo>> {
    let parent = config_path
        .parent()
        .context("config path has no parent directory")?;
    let stem = config_path
        .file_name()
        .context("config path has no file name")?
        .to_string_lossy();

    let prefix = format!("{stem}.bak.");
    let mut backups = Vec::new();

    if !parent.exists() {
        return Ok(backups);
    }

    for entry in std::fs::read_dir(parent).context("failed to read config directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(timestamp) = name.strip_prefix(&prefix) {
            backups.push(BackupInfo {
                path: entry.path(),
                timestamp: timestamp.to_string(),
            });
        }
    }

    backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(backups)
}

/// Restore a specific backup to the config path.
pub fn rollback(config_path: &Path, backup_path: &Path) -> anyhow::Result<()> {
    if !backup_path.exists() {
        bail!("backup file does not exist: {}", backup_path.display());
    }

    // Validate the backup is a valid config before restoring.
    let content = std::fs::read_to_string(backup_path).context("failed to read backup file")?;
    let cfg: AgentZeroConfig =
        toml::from_str(&content).context("backup contains invalid config")?;
    cfg.validate().context("backup config validation failed")?;

    std::fs::copy(backup_path, config_path).context("failed to restore backup")?;

    Ok(())
}

/// Compute a simple diff between the current config and proposed sections.
/// Returns lines prefixed with `-` (removed) or `+` (added).
pub fn diff_sections(config_path: &Path, sections: &[ConfigSection]) -> anyhow::Result<String> {
    let current = std::fs::read_to_string(config_path).unwrap_or_default();
    let (merged, _) = read_and_merge(config_path, sections)?;

    let current_lines: Vec<&str> = current.lines().collect();
    let merged_lines: Vec<&str> = merged.lines().collect();

    let mut diff_output = String::new();
    // Simple line-by-line comparison (not a full LCS diff, but sufficient for config changes)
    let max_len = current_lines.len().max(merged_lines.len());
    for i in 0..max_len {
        match (current_lines.get(i), merged_lines.get(i)) {
            (Some(old), Some(new)) if old == new => {
                diff_output.push_str(&format!("  {old}\n"));
            }
            (Some(old), Some(new)) => {
                diff_output.push_str(&format!("- {old}\n"));
                diff_output.push_str(&format!("+ {new}\n"));
            }
            (Some(old), None) => {
                diff_output.push_str(&format!("- {old}\n"));
            }
            (None, Some(new)) => {
                diff_output.push_str(&format!("+ {new}\n"));
            }
            (None, None) => {}
        }
    }

    Ok(diff_output)
}

/// Remove old backup files, keeping only the most recent `max_backups`.
fn prune_backups(config_path: &Path, max_backups: usize) {
    if max_backups == 0 {
        return;
    }
    if let Ok(mut backups) = list_backups(config_path) {
        while backups.len() > max_backups {
            if let Some(oldest) = backups.pop() {
                let _ = std::fs::remove_file(&oldest.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-writer-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn read_and_merge_validates() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("write should succeed");

        // Valid merge (empty sections = default config)
        let result = read_and_merge(&config_path, &[]);
        assert!(result.is_ok(), "empty merge should succeed");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_with_backup_creates_backup() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "# original").expect("write should succeed");

        let backup = write_with_backup(&config_path, "# updated", 5)
            .expect("write_with_backup should succeed");
        assert!(backup.is_some(), "backup should be created");
        assert!(backup.as_ref().expect("backup exists").exists());

        let content = fs::read_to_string(&config_path).expect("read should succeed");
        assert_eq!(content, "# updated");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_backups_finds_files() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        fs::write(&config_path, "").expect("write should succeed");
        fs::write(dir.join("agentzero.toml.bak.20260316T120000Z"), "# bak1")
            .expect("write should succeed");
        fs::write(dir.join("agentzero.toml.bak.20260316T130000Z"), "# bak2")
            .expect("write should succeed");

        let backups = list_backups(&config_path).expect("list should succeed");
        assert_eq!(backups.len(), 2);
        // Most recent first
        assert_eq!(backups[0].timestamp, "20260316T130000Z");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn json_value_to_toml_converts_correctly() {
        let json = serde_json::json!({
            "enabled": true,
            "count": 42,
            "name": "test",
            "items": [1, 2, 3]
        });
        let toml_val = json_value_to_toml(&json).expect("conversion should succeed");
        assert!(matches!(toml_val, toml::Value::Table(_)));
    }
}
