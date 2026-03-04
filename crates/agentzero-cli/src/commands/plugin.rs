use crate::cli::PluginCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_plugins::package::{
    install_packaged_plugin, list_installed_plugins, package_plugin, remove_installed_plugin,
    PluginManifest,
};
use agentzero_plugins::wasm::{
    WasmExecutionRequest, WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime,
};
use async_trait::async_trait;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub struct PluginCommand;

#[async_trait]
impl AgentZeroCommand for PluginCommand {
    type Options = PluginCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            PluginCommands::New {
                id,
                version,
                entrypoint,
                wasm_file,
                out_dir,
                force,
            } => {
                let out_dir = out_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().expect("cwd should resolve"));
                fs::create_dir_all(&out_dir)?;
                let manifest_path = out_dir.join("manifest.json");
                if manifest_path.exists() && !force {
                    anyhow::bail!(
                        "manifest already exists at {} (use --force to overwrite)",
                        manifest_path.display()
                    );
                }

                let manifest = PluginManifest {
                    id,
                    version,
                    entrypoint,
                    wasm_file,
                    wasm_sha256: "0".repeat(64),
                    capabilities: vec![],
                    hooks: vec![],
                    min_runtime_api: 2,
                    max_runtime_api: 2,
                    allowed_host_calls: vec![],
                };
                manifest.validate()?;
                fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
                println!(
                    "Wrote plugin manifest template: {}",
                    manifest_path.display()
                );
            }
            PluginCommands::Validate { manifest } => {
                let manifest = load_manifest(&manifest)?;
                manifest.validate()?;
                println!("Manifest is valid: {}", manifest.id);
            }
            PluginCommands::Test {
                manifest,
                wasm,
                execute,
            } => {
                let manifest = load_manifest(&manifest)?;
                manifest.validate()?;

                let container = WasmPluginContainer {
                    id: manifest.id.clone(),
                    module_path: PathBuf::from(wasm),
                    entrypoint: manifest.entrypoint.clone(),
                    max_execution_ms: 5_000,
                    max_memory_mb: 64,
                    allow_network: false,
                    allow_fs_write: false,
                };
                let runtime = WasmPluginRuntime::new();
                let policy = WasmIsolationPolicy {
                    max_execution_ms: 5_000,
                    max_module_bytes: 5 * 1024 * 1024,
                    max_memory_mb: 64,
                    allow_network: false,
                    allow_fs_write: false,
                    allow_fs_read: false,
                    allowed_host_calls: manifest.allowed_host_calls.clone(),
                };
                runtime.preflight_with_policy(&container, &policy)?;
                if execute {
                    let result = runtime.execute_with_policy(
                        &container,
                        &WasmExecutionRequest {
                            input: serde_json::json!({}),
                        },
                        &policy,
                    )?;
                    println!(
                        "Plugin test execution succeeded: status_code={}",
                        result.status_code
                    );
                } else {
                    println!("Plugin preflight succeeded");
                }
            }
            PluginCommands::Package {
                manifest,
                wasm,
                out,
            } => {
                let manifest = load_manifest(&manifest)?;
                package_plugin(wasm, manifest, out.as_str())?;
                println!("Packaged plugin archive: {out}");
            }
            PluginCommands::Dev {
                manifest,
                wasm,
                iterations,
                execute,
            } => {
                if iterations == 0 {
                    anyhow::bail!("plugin dev loop requires --iterations >= 1");
                }

                let manifest = load_manifest(&manifest)?;
                manifest.validate()?;
                let policy = WasmIsolationPolicy {
                    max_execution_ms: 5_000,
                    max_module_bytes: 5 * 1024 * 1024,
                    max_memory_mb: 64,
                    allow_network: false,
                    allow_fs_write: false,
                    allow_fs_read: false,
                    allowed_host_calls: manifest.allowed_host_calls.clone(),
                };
                let runtime = WasmPluginRuntime::new();
                let container = WasmPluginContainer {
                    id: manifest.id.clone(),
                    module_path: PathBuf::from(wasm),
                    entrypoint: manifest.entrypoint.clone(),
                    max_execution_ms: 5_000,
                    max_memory_mb: 64,
                    allow_network: false,
                    allow_fs_write: false,
                };

                for idx in 0..iterations {
                    runtime.preflight_with_policy(&container, &policy)?;
                    if execute {
                        let fixture = deterministic_fixture();
                        let result = runtime.execute_with_policy(&container, &fixture, &policy)?;
                        println!(
                            "[{}/{}] dev execution status_code={}",
                            idx + 1,
                            iterations,
                            result.status_code
                        );
                    } else {
                        println!("[{}/{}] dev preflight ok", idx + 1, iterations);
                    }
                }
            }
            PluginCommands::Install {
                package,
                install_dir,
            } => {
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));
                let installed = install_packaged_plugin(package, &install_root)?;
                println!(
                    "Installed plugin {}@{} at {}",
                    installed.manifest.id,
                    installed.manifest.version,
                    installed.install_dir.display()
                );
            }
            PluginCommands::List { json, install_dir } => {
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));
                let installed = list_installed_plugins(&install_root)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&PluginListOutput {
                            install_root: install_root.display().to_string(),
                            plugins: installed,
                        })?
                    );
                } else if installed.is_empty() {
                    println!("No installed plugins");
                } else {
                    println!("Installed plugins ({} total):\n", installed.len());
                    println!("  ID                  VERSION        INSTALL_DIR");
                    println!(
                        "  ------------------- -------------- -------------------------------"
                    );
                    for plugin in installed {
                        println!(
                            "  {:<19} {:<14} {}",
                            plugin.id,
                            plugin.version,
                            plugin.install_dir.display()
                        );
                    }
                }
            }
            PluginCommands::Remove {
                id,
                version,
                install_dir,
            } => {
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));
                let removed = remove_installed_plugin(&install_root, &id, version.as_deref())?;
                if removed == 0 {
                    if let Some(version) = version {
                        println!("No installed plugin found for {id}@{version}");
                    } else {
                        println!("No installed plugin found for {id}");
                    }
                } else if let Some(version) = version {
                    println!("Removed plugin {id}@{version}");
                } else {
                    println!("Removed plugin {id} ({removed} version(s))");
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct PluginListOutput {
    install_root: String,
    plugins: Vec<agentzero_plugins::package::InstalledPluginRecord>,
}

fn load_manifest(path: impl AsRef<Path>) -> anyhow::Result<PluginManifest> {
    let path = path.as_ref();
    let bytes = fs::read(path)?;
    let manifest: PluginManifest = serde_json::from_slice(&bytes)?;
    Ok(manifest)
}

fn deterministic_fixture() -> WasmExecutionRequest {
    WasmExecutionRequest {
        input: serde_json::json!({
            "fixture": "agentzero-plugin-dev",
            "payload": {"stable": true}
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::PluginCommand;
    use crate::cli::PluginCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
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
            "agentzero-plugin-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn plugin_new_and_validate_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        PluginCommand::run(
            &ctx,
            PluginCommands::New {
                id: "my-plugin".to_string(),
                version: "0.1.0".to_string(),
                entrypoint: "run".to_string(),
                wasm_file: "plugin.wasm".to_string(),
                out_dir: Some(dir.to_string_lossy().to_string()),
                force: false,
            },
        )
        .await
        .expect("new should succeed");

        PluginCommand::run(
            &ctx,
            PluginCommands::Validate {
                manifest: dir.join("manifest.json").to_string_lossy().to_string(),
            },
        )
        .await
        .expect("validate should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn plugin_validate_missing_manifest_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = PluginCommand::run(
            &ctx,
            PluginCommands::Validate {
                manifest: dir.join("missing.json").to_string_lossy().to_string(),
            },
        )
        .await
        .expect_err("missing manifest should fail");
        assert!(err.to_string().contains("No such file") || err.to_string().contains("os error"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn plugin_remove_nonexistent_is_handled_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        PluginCommand::run(
            &ctx,
            PluginCommands::Remove {
                id: "missing-plugin".to_string(),
                version: None,
                install_dir: Some(dir.join("plugins").to_string_lossy().to_string()),
            },
        )
        .await
        .expect("remove should succeed for missing plugin");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn plugin_remove_empty_id_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = PluginCommand::run(
            &ctx,
            PluginCommands::Remove {
                id: String::new(),
                version: None,
                install_dir: Some(dir.join("plugins").to_string_lossy().to_string()),
            },
        )
        .await
        .expect_err("empty plugin id should fail");
        assert!(err.to_string().contains("plugin id cannot be empty"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn plugin_dev_preflight_loop_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let manifest_path = dir.join("manifest.json");
        let wasm_path = dir.join("plugin.wasm");
        fs::write(&wasm_path, [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])
            .expect("minimal wasm should be written");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "id": "dev-plugin",
                "version": "0.1.0",
                "entrypoint": "run",
                "wasm_file": "plugin.wasm",
                "wasm_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                "capabilities": [],
                "hooks": [],
                "min_runtime_api": 2,
                "max_runtime_api": 2,
                "allowed_host_calls": []
            }))
            .expect("manifest should serialize"),
        )
        .expect("manifest should write");

        PluginCommand::run(
            &ctx,
            PluginCommands::Dev {
                manifest: manifest_path.to_string_lossy().to_string(),
                wasm: wasm_path.to_string_lossy().to_string(),
                iterations: 2,
                execute: false,
            },
        )
        .await
        .expect("plugin dev preflight loop should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn plugin_dev_zero_iterations_fails_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = PluginCommand::run(
            &ctx,
            PluginCommands::Dev {
                manifest: "manifest.json".to_string(),
                wasm: "plugin.wasm".to_string(),
                iterations: 0,
                execute: false,
            },
        )
        .await
        .expect_err("zero iterations should fail");
        assert!(err.to_string().contains("--iterations >= 1"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
