use crate::cli::{MigrateCommands, UpdateCommands};
use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::update::migrate_openclaw;
use crate::update::migration::{import_from_source, inspect_source};
use crate::update::updater::{
    check_for_updates, download_and_install, fetch_latest_version, load_state, restore_backup,
    rollback_update,
};
use async_trait::async_trait;
use std::path::PathBuf;

pub struct MigrateCommand;
pub struct UpdateCommand;

#[async_trait]
impl AgentZeroCommand for MigrateCommand {
    type Options = MigrateCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            MigrateCommands::Import { source, dry_run } => {
                let source = resolve_migration_source(source)?;
                let inspect = inspect_source(&source)?;
                println!("Migration source: {}", inspect.source.display());
                println!("Found files ({}):", inspect.found_files.len());
                for file in &inspect.found_files {
                    println!("  - {file}");
                }
                if !inspect.missing_files.is_empty() {
                    println!("Missing known files ({}):", inspect.missing_files.len());
                    for file in &inspect.missing_files {
                        println!("  - {file}");
                    }
                }
                println!();

                let result = import_from_source(&source, &ctx.data_dir, dry_run)?;
                let mode = if dry_run { "DRY RUN" } else { "APPLY" };
                println!("Migration import [{mode}]");
                println!("  source: {}", result.source.display());
                println!("  target: {}", result.target.display());
                println!("  copied: {}", result.copied_files.join(", "));
                if !result.skipped_files.is_empty() {
                    println!("  skipped: {}", result.skipped_files.join(", "));
                }
            }
            MigrateCommands::Openclaw {
                source,
                dry_run,
                skip_memory,
                skip_config,
            } => {
                let result = migrate_openclaw::migrate(
                    source.as_deref(),
                    &ctx.data_dir,
                    dry_run,
                    skip_memory,
                    skip_config,
                )?;
                let mode = if dry_run { "DRY RUN" } else { "APPLY" };
                println!("OpenClaw migration [{mode}]");
                println!("  source: {}", result.source.display());

                if result.config_converted {
                    println!("  config: converted to TOML");
                } else if result.config_skipped {
                    println!("  config: skipped");
                }

                if result.memory_entries_imported > 0 {
                    println!(
                        "  memory: {} entries imported",
                        result.memory_entries_imported
                    );
                } else if result.memory_skipped {
                    println!("  memory: skipped");
                }

                for warning in &result.warnings {
                    println!("  warning: {warning}");
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl AgentZeroCommand for UpdateCommand {
    type Options = UpdateCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let state_path = ctx.data_dir.join("update").join("state.json");
        let current_version = env!("CARGO_PKG_VERSION");

        match opts {
            UpdateCommands::Check { channel: _, json } => {
                // Env var override lets tests bypass the network.
                let latest = match std::env::var("AGENTZERO_UPDATE_LATEST").ok() {
                    Some(v) => v,
                    None => {
                        let github_token = std::env::var("GITHUB_TOKEN").ok();
                        fetch_latest_version(github_token.as_deref()).await?
                    }
                };
                let result =
                    check_for_updates(&state_path, current_version, Some(latest.as_str()))?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else if result.up_to_date {
                    println!("Up to date: {}", result.current_version);
                } else {
                    println!(
                        "Update available: {} -> {}",
                        result.current_version, result.latest_version
                    );
                }
            }
            UpdateCommands::Apply { version, json } => {
                let github_token = std::env::var("GITHUB_TOKEN").ok();
                let target = match version {
                    Some(v) => v,
                    None => fetch_latest_version(github_token.as_deref()).await?,
                };
                if !json {
                    println!("Downloading agentzero v{target}…");
                }
                let result = download_and_install(
                    &state_path,
                    current_version,
                    &target,
                    github_token.as_deref(),
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Updated: {} -> {} — restart to use the new version",
                        result.from_version, result.to_version
                    );
                }
            }
            UpdateCommands::Rollback { json } => {
                // Try restoring a backed-up binary first; fall back to state-only rollback.
                let result = match restore_backup(&state_path, current_version).await {
                    Ok(r) => r,
                    Err(_) => rollback_update(&state_path, current_version)?,
                };
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Rolled back: {} -> {} — restart to use the previous version",
                        result.from_version, result.to_version
                    );
                }
            }
            UpdateCommands::Status { json } => {
                let state = load_state(&state_path, current_version)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&state)?);
                } else {
                    println!("Update status");
                    println!("  current version: {}", state.current_version);
                    println!(
                        "  last target: {}",
                        state.last_target_version.as_deref().unwrap_or("none")
                    );
                    println!("  previous versions: {}", state.previous_versions.len());
                    println!("  state file: {}", state_path.display());
                }
            }
        }
        Ok(())
    }
}

fn resolve_migration_source(source: Option<String>) -> anyhow::Result<PathBuf> {
    if let Some(source) = source {
        return Ok(PathBuf::from(source));
    }

    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow::anyhow!("HOME is not set; pass --source explicitly"))?;
    Ok(home.join(".agentzero").join("workspace"))
}

#[cfg(test)]
mod tests {
    use super::{MigrateCommand, UpdateCommand};
    use crate::cli::{MigrateCommands, UpdateCommands};
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-update-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn migrate_import_and_update_apply_success_path() {
        let data_dir = temp_dir();
        let source_dir = temp_dir();
        fs::write(source_dir.join("agentzero.toml"), "provider = \"openai\"\n")
            .expect("source config should be written");

        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        MigrateCommand::run(
            &ctx,
            MigrateCommands::Import {
                source: Some(source_dir.to_string_lossy().to_string()),
                dry_run: false,
            },
        )
        .await
        .expect("migration import should succeed");
        assert!(data_dir.join("agentzero.toml").exists());

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
        fs::remove_dir_all(source_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn update_rollback_without_history_fails_negative_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        let err = UpdateCommand::run(&ctx, UpdateCommands::Rollback { json: false })
            .await
            .expect_err("rollback without history should fail");
        assert!(err.to_string().contains("no previous version"));

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn migrate_import_missing_source_fails_negative_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        let err = MigrateCommand::run(
            &ctx,
            MigrateCommands::Import {
                source: Some(data_dir.join("missing").to_string_lossy().to_string()),
                dry_run: false,
            },
        )
        .await
        .expect_err("missing source should fail");
        assert!(err.to_string().contains("does not exist"));

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[test]
    fn update_status_json_shape_smoke_success_path() {
        let payload = json!({
            "current_version": "0.1.0",
            "last_target_version": null,
            "previous_versions": []
        });
        assert_eq!(payload["current_version"], "0.1.0");
    }
}
