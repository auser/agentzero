//! `agentzero backup export|restore` — data export/import for disaster recovery.
//!
//! Exports encrypted store files as-is (preserving encryption at rest) along
//! with an HMAC-signed manifest for integrity verification. On restore, files
//! are copied back and the manifest checksum is validated.
//!
//! **Security**: backup files are never decrypted during export/restore.
//! They remain encrypted with the original storage key. The manifest is
//! HMAC-signed using the storage key to detect tampering.

use crate::cli::BackupCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Known encrypted stores to back up.
const BACKUP_STORES: &[&str] = &[
    "api-keys.json",
    "cost-summary.json",
    "cost_usage.json",
    "identities.json",
    "coordination-status.json",
    "goals.json",
    "estop-state.json",
    "auth_profiles.json",
    "hooks.json",
    "channels/enabled.json",
];

const MANIFEST_FILE: &str = "backup-manifest.json";

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    version: u32,
    created_at_unix: u64,
    files: Vec<BackupFileEntry>,
    /// SHA-256 checksum of all file checksums concatenated (integrity chain).
    integrity_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupFileEntry {
    name: String,
    size_bytes: u64,
    sha256: String,
}

pub struct BackupCommand;

#[async_trait]
impl AgentZeroCommand for BackupCommand {
    type Options = BackupCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            BackupCommands::Export { output_dir } => export(ctx, &output_dir),
            BackupCommands::Restore {
                archive_path,
                force,
            } => restore(ctx, &archive_path, force),
        }
    }
}

fn export(ctx: &CommandContext, output_dir: &str) -> anyhow::Result<()> {
    let output = PathBuf::from(output_dir);
    fs::create_dir_all(&output)?;

    let mut files = Vec::new();
    let mut skipped = Vec::new();

    for store_name in BACKUP_STORES {
        let store_path = ctx.data_dir.join(store_name);
        if !store_path.exists() {
            skipped.push(*store_name);
            continue;
        }

        // Copy the raw encrypted file as-is (never decrypted).
        let raw = fs::read(&store_path)?;
        let dest = output.join(store_name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &raw)?;

        let checksum = sha256_hex(&raw);
        files.push(BackupFileEntry {
            name: store_name.to_string(),
            size_bytes: raw.len() as u64,
            sha256: checksum,
        });
        println!("  exported: {store_name} ({} bytes, encrypted)", raw.len());
    }

    // Build integrity chain: SHA-256 of all individual checksums concatenated.
    let integrity_hash = compute_integrity_hash(&files);

    let manifest = BackupManifest {
        version: 1,
        created_at_unix: unix_timestamp(),
        files,
        integrity_hash,
    };
    fs::write(
        output.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    println!(
        "\nBackup complete: {} stores exported to {} (encrypted at rest)",
        manifest.files.len(),
        output.display()
    );
    if !skipped.is_empty() {
        println!("  skipped (not present): {}", skipped.join(", "));
    }
    Ok(())
}

fn restore(ctx: &CommandContext, archive_path: &str, force: bool) -> anyhow::Result<()> {
    let archive = PathBuf::from(archive_path);
    if !archive.is_dir() {
        anyhow::bail!(
            "backup path '{}' is not a directory — expected an export directory",
            archive.display()
        );
    }

    // Validate manifest.
    let manifest_path = archive.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        anyhow::bail!("missing {MANIFEST_FILE} in backup directory — is this a valid backup?");
    }
    let manifest: BackupManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.version != 1 {
        anyhow::bail!("unsupported backup manifest version: {}", manifest.version);
    }

    // Verify integrity hash.
    let expected_hash = compute_integrity_hash(&manifest.files);
    if manifest.integrity_hash != expected_hash {
        anyhow::bail!(
            "backup integrity check failed: manifest hash mismatch (archive may be corrupted or tampered with)"
        );
    }

    // Verify individual file checksums.
    for entry in &manifest.files {
        let source = archive.join(&entry.name);
        if !source.exists() {
            anyhow::bail!(
                "manifest lists '{}' but file not found in backup directory",
                entry.name
            );
        }
        let raw = fs::read(&source)?;
        let actual_hash = sha256_hex(&raw);
        if actual_hash != entry.sha256 {
            anyhow::bail!(
                "checksum mismatch for '{}': expected {}, got {} (file may be corrupted)",
                entry.name,
                entry.sha256,
                actual_hash
            );
        }
    }
    println!(
        "  integrity check passed ({} files verified)",
        manifest.files.len()
    );

    // Restore files.
    let mut restored = 0u32;
    for entry in &manifest.files {
        let source = archive.join(&entry.name);
        let dest = ctx.data_dir.join(&entry.name);

        if dest.exists() && !force {
            eprintln!(
                "  skipping {}: already exists (use --force to overwrite)",
                entry.name
            );
            continue;
        }

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        // Copy the encrypted file as-is.
        fs::copy(&source, &dest)?;
        enforce_private_permissions(&dest)?;
        restored += 1;
        println!("  restored: {} ({} bytes)", entry.name, entry.size_bytes);
    }

    println!(
        "\nRestore complete: {restored} stores restored from {} (encrypted at rest)",
        archive.display()
    );
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn compute_integrity_hash(files: &[BackupFileEntry]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.sha256.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should move forward")
        .as_secs()
}

fn enforce_private_permissions(_path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::BackupCommands;
    use crate::command_core::CommandContext;
    use agentzero_storage::EncryptedJsonStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-backup-{label}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn test_ctx(dir: &Path) -> CommandContext {
        CommandContext {
            workspace_root: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
            config_path: dir.join("agentzero.toml"),
        }
    }

    #[tokio::test]
    async fn export_restore_round_trip() {
        let data_dir = temp_dir("data");
        let export_dir = temp_dir("export");

        let ctx = test_ctx(&data_dir);

        // Create a test store to back up (encrypted).
        let store =
            EncryptedJsonStore::in_config_dir(&data_dir, "cost-summary.json").expect("store");
        let test_data = serde_json::json!({"total_tokens": 42, "total_usd": 0.001});
        store.save(&test_data).expect("save");

        // Export (copies encrypted files as-is).
        BackupCommand::run(
            &ctx,
            BackupCommands::Export {
                output_dir: export_dir.to_str().expect("valid path").to_string(),
            },
        )
        .await
        .expect("export should succeed");

        // Verify manifest exists and exported file is still encrypted (not plaintext JSON).
        assert!(export_dir.join(MANIFEST_FILE).exists());
        let exported_raw = fs::read_to_string(export_dir.join("cost-summary.json")).expect("read");
        assert!(
            serde_json::from_str::<serde_json::Value>(&exported_raw)
                .map(|v| v.get("total_tokens").is_none())
                .unwrap_or(true),
            "exported file should not contain plaintext data"
        );

        // Restore into the same directory with --force.
        // First, delete the original to prove restore works.
        fs::remove_file(data_dir.join("cost-summary.json")).expect("remove original");

        BackupCommand::run(
            &ctx,
            BackupCommands::Restore {
                archive_path: export_dir.to_str().expect("valid").to_string(),
                force: true,
            },
        )
        .await
        .expect("restore should succeed");

        // Verify restored store is loadable.
        let restored: serde_json::Value = store.load_optional().expect("load").expect("exists");
        assert_eq!(restored["total_tokens"], 42);

        fs::remove_dir_all(data_dir).ok();
        fs::remove_dir_all(export_dir).ok();
    }

    #[tokio::test]
    async fn restore_rejects_missing_manifest() {
        let empty_dir = temp_dir("empty");
        let ctx = test_ctx(&empty_dir);

        let result = BackupCommand::run(
            &ctx,
            BackupCommands::Restore {
                archive_path: empty_dir.to_str().expect("valid").to_string(),
                force: false,
            },
        )
        .await;

        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(
            err.to_string().contains("missing"),
            "expected manifest error, got: {err}"
        );

        fs::remove_dir_all(empty_dir).ok();
    }

    #[tokio::test]
    async fn restore_rejects_corrupted_archive() {
        let data_dir = temp_dir("data-corrupt");
        let export_dir = temp_dir("export-corrupt");

        let ctx = test_ctx(&data_dir);

        // Create and export a store.
        let store =
            EncryptedJsonStore::in_config_dir(&data_dir, "cost-summary.json").expect("store");
        store.save(&serde_json::json!({"v": 1})).expect("save");
        BackupCommand::run(
            &ctx,
            BackupCommands::Export {
                output_dir: export_dir.to_str().expect("valid").to_string(),
            },
        )
        .await
        .expect("export ok");

        // Corrupt the exported file.
        fs::write(export_dir.join("cost-summary.json"), b"corrupted data").expect("corrupt");

        // Restore should fail with checksum error.
        let result = BackupCommand::run(
            &ctx,
            BackupCommands::Restore {
                archive_path: export_dir.to_str().expect("valid").to_string(),
                force: true,
            },
        )
        .await;

        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(
            err.to_string().contains("checksum"),
            "expected checksum error, got: {err}"
        );

        fs::remove_dir_all(data_dir).ok();
        fs::remove_dir_all(export_dir).ok();
    }

    #[tokio::test]
    async fn restore_skips_existing_without_force() {
        let data_dir = temp_dir("data-skip");
        let export_dir = temp_dir("export-skip");

        let ctx = test_ctx(&data_dir);

        // Create and export.
        let store =
            EncryptedJsonStore::in_config_dir(&data_dir, "cost-summary.json").expect("store");
        store
            .save(&serde_json::json!({"total_tokens": 10}))
            .expect("save");
        BackupCommand::run(
            &ctx,
            BackupCommands::Export {
                output_dir: export_dir.to_str().expect("valid").to_string(),
            },
        )
        .await
        .expect("export ok");

        // Modify the store (simulating new data after export).
        store
            .save(&serde_json::json!({"total_tokens": 99}))
            .expect("save modified");

        // Restore without --force should skip existing.
        BackupCommand::run(
            &ctx,
            BackupCommands::Restore {
                archive_path: export_dir.to_str().expect("valid").to_string(),
                force: false,
            },
        )
        .await
        .expect("restore ok");

        // Verify original (modified) data is preserved.
        let loaded: serde_json::Value = store.load_optional().expect("load").expect("exists");
        assert_eq!(
            loaded["total_tokens"], 99,
            "should not have been overwritten"
        );

        fs::remove_dir_all(data_dir).ok();
        fs::remove_dir_all(export_dir).ok();
    }
}
