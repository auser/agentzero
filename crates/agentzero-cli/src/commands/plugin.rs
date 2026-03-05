use crate::cli::PluginCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_plugins::package::{
    check_outdated, generate_registry_entry, install_from_url, install_packaged_plugin,
    list_installed_plugins, load_registry_index, package_plugin, remove_installed_plugin,
    PluginManifest, PluginState, RegistryEntryParams,
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
                scaffold,
            } => {
                let out_dir = out_dir
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().expect("cwd should resolve"));

                if scaffold.as_deref() == Some("rust") {
                    scaffold_rust_plugin(&id, &version, &out_dir, force)?;
                } else if scaffold.is_some() {
                    anyhow::bail!("unsupported scaffold type (supported: rust)");
                } else {
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
                        description: None,
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
                url,
                sha256,
                install_dir,
            } => {
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));

                let is_url_install = url.is_some();
                let installed = if let Some(url) = url {
                    install_from_url(&url, &install_root, sha256.as_deref())?
                } else if let Some(package) = package {
                    install_packaged_plugin(package, &install_root)?
                } else {
                    anyhow::bail!("either --package or --url is required");
                };

                // Record install in state
                let mut state = PluginState::load(&ctx.data_dir);
                let source = if is_url_install { "url" } else { "local" };
                state.record_install(&installed.manifest.id, &installed.manifest.version, source);
                state.save(&ctx.data_dir)?;

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
                } else {
                    // Remove from state
                    let mut state = PluginState::load(&ctx.data_dir);
                    state.remove(&id);
                    state.save(&ctx.data_dir)?;

                    if let Some(version) = version {
                        println!("Removed plugin {id}@{version}");
                    } else {
                        println!("Removed plugin {id} ({removed} version(s))");
                    }
                }
            }
            PluginCommands::Enable { id } => {
                let mut state = PluginState::load(&ctx.data_dir);
                state.enable(&id)?;
                state.save(&ctx.data_dir)?;
                println!("Enabled plugin {id}");
            }
            PluginCommands::Disable { id } => {
                let mut state = PluginState::load(&ctx.data_dir);
                state.disable(&id)?;
                state.save(&ctx.data_dir)?;
                println!("Disabled plugin {id}");
            }
            PluginCommands::Info { id, install_dir } => {
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));
                let installed = list_installed_plugins(&install_root)?;
                let matching: Vec<_> = installed.iter().filter(|p| p.id == id).collect();

                if matching.is_empty() {
                    println!("No installed plugin found with id '{id}'");
                } else {
                    let state = PluginState::load(&ctx.data_dir);
                    let enabled = state.is_enabled(&id);
                    let state_entry = state.plugins.get(&id);

                    println!("Plugin: {id}");
                    println!("  Enabled:  {enabled}");
                    if let Some(entry) = state_entry {
                        println!("  Source:   {}", entry.source);
                        println!("  Installed at: {}", entry.installed_at);
                    }
                    println!("  Versions:");
                    for p in &matching {
                        // Load manifest for details
                        let manifest_path = &p.manifest_path;
                        if let Ok(manifest) = load_manifest(manifest_path) {
                            println!("    {}:", p.version);
                            println!("      Entrypoint:     {}", manifest.entrypoint);
                            println!("      WASM file:      {}", manifest.wasm_file);
                            println!("      SHA256:         {}", manifest.wasm_sha256);
                            println!(
                                "      API range:      {}..={}",
                                manifest.min_runtime_api, manifest.max_runtime_api
                            );
                            if !manifest.capabilities.is_empty() {
                                println!(
                                    "      Capabilities:   {}",
                                    manifest.capabilities.join(", ")
                                );
                            }
                            if !manifest.allowed_host_calls.is_empty() {
                                println!(
                                    "      Host calls:     {}",
                                    manifest.allowed_host_calls.join(", ")
                                );
                            }
                        } else {
                            println!("    {}: (manifest unreadable)", p.version);
                        }
                    }
                }
            }
            PluginCommands::Search {
                query,
                registry_url,
            } => {
                let index = load_registry_index(&ctx.data_dir, registry_url.as_deref())?;
                let results = index.search(&query);
                if results.is_empty() {
                    println!("No plugins found matching '{query}'");
                } else {
                    println!("Found {} plugin(s):\n", results.len());
                    println!("  ID                  LATEST   CATEGORY      DESCRIPTION");
                    println!(
                        "  ------------------- -------- ------------- ----------------------------"
                    );
                    for entry in results {
                        println!(
                            "  {:<19} {:<8} {:<13} {}",
                            entry.id, entry.latest, entry.category, entry.description
                        );
                    }
                }
            }
            PluginCommands::Outdated { registry_url } => {
                let index = load_registry_index(&ctx.data_dir, registry_url.as_deref())?;
                let state = PluginState::load(&ctx.data_dir);
                let outdated = check_outdated(&state, &index);

                if outdated.is_empty() {
                    println!("All plugins are up to date");
                } else {
                    println!("{} plugin(s) have updates:\n", outdated.len());
                    println!("  ID                  INSTALLED  LATEST");
                    println!("  ------------------- ---------- ----------");
                    for (id, installed, latest) in &outdated {
                        println!("  {:<19} {:<10} {}", id, installed, latest);
                    }
                }
            }
            PluginCommands::Update {
                id,
                registry_url,
                install_dir,
            } => {
                let index = load_registry_index(&ctx.data_dir, registry_url.as_deref())?;
                let state = PluginState::load(&ctx.data_dir);
                let install_root = install_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ctx.data_dir.join("plugins"));

                let outdated = check_outdated(&state, &index);
                let to_update: Vec<_> = if let Some(ref target_id) = id {
                    outdated
                        .into_iter()
                        .filter(|(i, _, _)| i == target_id)
                        .collect()
                } else {
                    outdated
                };

                if to_update.is_empty() {
                    if let Some(target_id) = id {
                        println!("Plugin '{target_id}' is up to date (or not installed)");
                    } else {
                        println!("All plugins are up to date");
                    }
                } else {
                    for (id, installed_ver, latest_ver) in &to_update {
                        let entry = index.get(id).unwrap();
                        let version_entry = entry.latest_version().unwrap();
                        println!(
                            "Updating {id}: {installed_ver} → {latest_ver} from {}",
                            version_entry.download_url
                        );
                        match install_from_url(
                            &version_entry.download_url,
                            &install_root,
                            Some(&version_entry.sha256),
                        ) {
                            Ok(installed) => {
                                let mut state = PluginState::load(&ctx.data_dir);
                                state.record_install(
                                    &installed.manifest.id,
                                    &installed.manifest.version,
                                    "registry",
                                );
                                state.save(&ctx.data_dir)?;
                                println!("  Updated to {}", installed.manifest.version);
                            }
                            Err(e) => {
                                println!("  Failed to update {id}: {e}");
                            }
                        }
                    }
                }
            }
            PluginCommands::Publish {
                manifest,
                download_url,
                sha256,
                description,
                category,
                author,
                repository,
                out,
            } => {
                let manifest = load_manifest(&manifest)?;
                let entry = generate_registry_entry(&RegistryEntryParams {
                    manifest: &manifest,
                    description: &description,
                    category: &category,
                    author: &author,
                    repository: &repository,
                    download_url: &download_url,
                    wasm_sha256: &sha256,
                });
                let json = serde_json::to_string_pretty(&entry)?;

                if let Some(out_path) = out {
                    fs::write(&out_path, &json)?;
                    println!("Registry entry written to {out_path}");
                } else {
                    println!("{json}");
                }
                println!("\nTo publish, add this entry to the registry index and open a PR.");
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

fn scaffold_rust_plugin(
    id: &str,
    version: &str,
    out_dir: &Path,
    force: bool,
) -> anyhow::Result<()> {
    let project_dir = out_dir.join(id);
    if project_dir.exists() && !force {
        anyhow::bail!(
            "directory already exists: {} (use --force to overwrite)",
            project_dir.display()
        );
    }
    fs::create_dir_all(project_dir.join("src"))?;
    fs::create_dir_all(project_dir.join(".cargo"))?;

    // Cargo.toml
    let crate_name = id.replace('-', "_");
    fs::write(
        project_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{id}"
version = "{version}"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
agentzero-plugin-sdk = "0.3.0"
serde_json = "1"
"#
        ),
    )?;

    // .cargo/config.toml
    fs::write(
        project_dir.join(".cargo/config.toml"),
        r#"[build]
target = "wasm32-wasip1"
"#,
    )?;

    // src/lib.rs
    fs::write(
        project_dir.join("src/lib.rs"),
        format!(
            r#"use agentzero_plugin_sdk::prelude::*;

declare_tool!("{crate_name}", execute);

fn execute(input: ToolInput) -> ToolOutput {{
    let req: serde_json::Value = match serde_json::from_str(&input.input) {{
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {{e}}")),
    }};

    ToolOutput::success(format!("Hello from {id}! Input: {{req}}"))
}}
"#
        ),
    )?;

    // manifest.json
    let manifest = PluginManifest {
        id: id.to_string(),
        version: version.to_string(),
        description: Some(format!("{id} plugin")),
        entrypoint: "az_tool_execute".to_string(),
        wasm_file: "plugin.wasm".to_string(),
        wasm_sha256: "0".repeat(64),
        capabilities: vec![],
        hooks: vec![],
        min_runtime_api: 2,
        max_runtime_api: 2,
        allowed_host_calls: vec![],
    };
    fs::write(
        project_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    println!(
        "Scaffolded Rust plugin project at {}",
        project_dir.display()
    );
    println!();
    println!("  cd {}", project_dir.display());
    println!("  cargo build --release");
    println!(
        "  agentzero plugin package --manifest manifest.json --wasm target/wasm32-wasip1/release/{crate_name}.wasm --out {id}-{version}.tar"
    );

    Ok(())
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
                scaffold: None,
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
