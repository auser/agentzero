//! Codegen strategy — compile LLM-generated Rust source to WASM and execute.
//!
//! Pipeline: LLM generates `declare_tool!` source → scaffold Cargo project →
//! `cargo build --target wasm32-wasip1` → load pre-compiled module → execute.

use agentzero_core::{ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

/// Curated allowlist of crates the LLM may use in codegen tools.
/// Pinned versions keep compile times predictable and prevent supply-chain risk.
pub const ALLOWED_CRATES: &[(&str, &str)] = &[
    ("serde", "1"),
    ("serde_json", "1"),
    ("regex", "1"),
    ("chrono", "0.4"),
    ("url", "2"),
    ("base64", "0.22"),
    ("sha2", "0.10"),
    ("hex", "0.4"),
    ("rand", "0.8"),
    ("csv", "1"),
];

/// Maximum time to wait for `cargo build` (seconds).
#[allow(dead_code)]
const COMPILE_TIMEOUT_SECS: u64 = 120;

/// The codegen compiler: scaffolds, compiles, and caches WASM modules.
pub struct CodegenCompiler {
    /// Root directory for codegen projects (`.agentzero/codegen/`).
    codegen_dir: PathBuf,
    /// Path to the `agentzero-plugin-sdk` crate for path dependencies.
    sdk_path: Option<PathBuf>,
}

impl CodegenCompiler {
    /// Create a compiler rooted at `data_dir/.agentzero/codegen/`.
    pub fn new(data_dir: &Path) -> Self {
        let codegen_dir = data_dir.join("codegen");
        // Try to find the SDK relative to the current executable or workspace.
        let sdk_path = find_sdk_path();
        Self {
            codegen_dir,
            sdk_path,
        }
    }

    /// Check that the `wasm32-wasip1` target is installed.
    pub async fn check_toolchain(&self) -> anyhow::Result<()> {
        let output = tokio::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .await
            .context("failed to run `rustup` — is Rust toolchain installed?")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.lines().any(|l| l.trim() == "wasm32-wasip1") {
            Ok(())
        } else {
            Err(anyhow!(
                "wasm32-wasip1 target is not installed. Run: rustup target add wasm32-wasip1"
            ))
        }
    }

    /// Scaffold a Cargo project for the given tool source.
    pub fn scaffold_project(
        &self,
        tool_name: &str,
        source: &str,
        extra_deps: &[(&str, &str)],
    ) -> anyhow::Result<PathBuf> {
        let project_dir = self.codegen_dir.join(tool_name);
        let src_dir = project_dir.join("src");
        std::fs::create_dir_all(&src_dir)
            .with_context(|| format!("failed to create codegen dir: {}", src_dir.display()))?;

        // Write source
        std::fs::write(src_dir.join("lib.rs"), source).context("failed to write codegen source")?;

        // Build Cargo.toml
        let mut cargo_toml = format!(
            r#"[package]
name = "agentzero-codegen-{tool_name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde_json = "1"
"#
        );

        // Add SDK dependency
        if let Some(ref sdk) = self.sdk_path {
            cargo_toml.push_str(&format!(
                "agentzero-plugin-sdk = {{ path = \"{}\" }}\n",
                sdk.display()
            ));
        } else {
            // Fallback: assume it's published to crates.io
            cargo_toml.push_str("agentzero-plugin-sdk = \"0.10\"\n");
        }

        // Add extra dependencies from allowlist
        for (name, version) in extra_deps {
            if ALLOWED_CRATES.iter().any(|(n, _)| n == name) {
                if *name == "serde" {
                    cargo_toml.push_str(&format!(
                        "{name} = {{ version = \"{version}\", features = [\"derive\"] }}\n"
                    ));
                } else {
                    cargo_toml.push_str(&format!("{name} = \"{version}\"\n"));
                }
            } else {
                tracing::warn!(crate_name = name, "codegen: rejecting unlisted dependency");
            }
        }

        std::fs::write(project_dir.join("Cargo.toml"), cargo_toml)
            .context("failed to write codegen Cargo.toml")?;

        // Write .cargo/config.toml for WASM target
        let cargo_config_dir = project_dir.join(".cargo");
        std::fs::create_dir_all(&cargo_config_dir)?;
        std::fs::write(
            cargo_config_dir.join("config.toml"),
            "[build]\ntarget = \"wasm32-wasip1\"\n",
        )
        .context("failed to write .cargo/config.toml")?;

        Ok(project_dir)
    }

    /// Compile the project to WASM. Returns the path to the `.wasm` file.
    pub async fn compile(&self, project_dir: &Path) -> anyhow::Result<PathBuf> {
        let shared_target = self.codegen_dir.join(".target");
        std::fs::create_dir_all(&shared_target)?;

        let output = tokio::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .env("CARGO_TARGET_DIR", &shared_target)
            .current_dir(project_dir)
            .output()
            .await
            .context("failed to spawn cargo build for codegen tool")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("codegen compilation failed:\n{stderr}"));
        }

        // Find the .wasm file
        let crate_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("invalid codegen project dir"))?;
        // Cargo converts hyphens to underscores in artifact names
        let artifact_name = crate_name.replace('-', "_");
        let wasm_name = format!("agentzero_codegen_{artifact_name}.wasm");
        let wasm_path = shared_target
            .join("wasm32-wasip1")
            .join("release")
            .join(&wasm_name);

        if !wasm_path.exists() {
            return Err(anyhow!(
                "expected WASM artifact not found at: {}",
                wasm_path.display()
            ));
        }

        Ok(wasm_path)
    }

    /// Compute SHA-256 hash of a file.
    pub fn compute_hash(path: &Path) -> anyhow::Result<String> {
        use sha2::{Digest, Sha256};
        let bytes =
            std::fs::read(path).with_context(|| format!("failed to read: {}", path.display()))?;
        let hash = Sha256::digest(&bytes);
        Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Compute SHA-256 hash of source code string.
    pub fn hash_source(source: &str) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(source.as_bytes());
        hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Full pipeline: scaffold → compile → hash. Returns (wasm_path, wasm_sha256, source_hash).
    pub async fn build_tool(
        &self,
        tool_name: &str,
        source: &str,
        extra_deps: &[(&str, &str)],
    ) -> anyhow::Result<(PathBuf, String, String)> {
        let project_dir = self.scaffold_project(tool_name, source, extra_deps)?;
        let wasm_path = self.compile(&project_dir).await?;
        let wasm_sha256 = Self::compute_hash(&wasm_path)?;
        let source_hash = Self::hash_source(source);
        Ok((wasm_path, wasm_sha256, source_hash))
    }

    /// Remove codegen projects not referenced by any registered tool name.
    pub fn gc(&self, active_tool_names: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0;
        if !self.codegen_dir.exists() {
            return Ok(0);
        }
        for entry in std::fs::read_dir(&self.codegen_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Skip the shared target dir and hidden dirs
            if name_str.starts_with('.') || name_str == ".target" {
                continue;
            }
            if entry.file_type()?.is_dir() && !active_tool_names.contains(&name_str.to_string()) {
                tracing::info!(dir = %name_str, "codegen gc: removing stale project");
                std::fs::remove_dir_all(entry.path()).ok();
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Extract dependency names from source code comments.
/// Looks for lines like: `// deps: regex, chrono`
pub fn extract_deps_from_source(source: &str) -> Vec<(&str, &str)> {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(deps_str) = trimmed.strip_prefix("// deps:") {
            return deps_str
                .split(',')
                .filter_map(|dep| {
                    let name = dep.trim();
                    ALLOWED_CRATES.iter().find(|(n, _)| *n == name).copied()
                })
                .collect();
        }
    }
    Vec::new()
}

/// Execute a codegen tool's compiled WASM.
///
/// This is called from `DynamicTool::execute()` when the strategy is `Codegen`.
#[cfg(feature = "wasm-plugins")]
pub async fn execute_codegen_tool(
    wasm_path: &str,
    input: &str,
    ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::sync::Arc;

    let path = PathBuf::from(wasm_path);
    if !path.exists() {
        return Err(anyhow!(
            "codegen WASM not found at: {wasm_path} — tool may need recompilation"
        ));
    }

    // Create engine + module (TODO: cache these across invocations)
    let engine = Arc::new(WasmPluginRuntime::create_engine()?);
    let module = Arc::new(WasmPluginRuntime::compile_module(&engine, &path)?);

    let container = WasmPluginContainer {
        id: format!(
            "codegen-{}",
            path.file_stem().unwrap_or_default().to_string_lossy()
        ),
        module_path: path.clone(),
        entrypoint: "az_tool_execute".to_string(),
        max_execution_ms: 30_000,
        max_memory_mb: 256,
        allow_network: false,
        allow_fs_write: false,
    };

    let options = WasmV2Options {
        workspace_root: ctx.workspace_root.clone(),
        capabilities: vec![],
    };

    let policy = WasmIsolationPolicy::default();
    let input_owned = input.to_string();

    let result = tokio::task::spawn_blocking(move || {
        WasmPluginRuntime::execute_v2_precompiled(
            &engine,
            &module,
            &container,
            &input_owned,
            &options,
            &policy,
        )
    })
    .await
    .map_err(|e| anyhow!("codegen wasm task panicked: {e}"))??;

    if let Some(err) = result.error {
        if result.output.is_empty() {
            return Err(anyhow!("codegen tool error: {err}"));
        }
        Ok(ToolResult {
            output: format!("{}\n[codegen warning: {err}]", result.output),
        })
    } else {
        Ok(ToolResult {
            output: result.output,
        })
    }
}

/// Fallback when `wasm-plugins` feature is not enabled.
#[cfg(not(feature = "wasm-plugins"))]
pub async fn execute_codegen_tool(
    _wasm_path: &str,
    _input: &str,
    _ctx: &ToolContext,
) -> anyhow::Result<ToolResult> {
    Err(anyhow!(
        "codegen tools require the `wasm-plugins` feature to be enabled"
    ))
}

/// Try to find the `agentzero-plugin-sdk` crate path relative to the workspace.
fn find_sdk_path() -> Option<PathBuf> {
    // Try relative to current executable
    if let Ok(exe) = std::env::current_exe() {
        // Walk up from binary to workspace root
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            if let Some(ref d) = dir {
                let candidate = d.join("crates/agentzero-plugin-sdk");
                if candidate.join("Cargo.toml").exists() {
                    return Some(candidate);
                }
                dir = d.parent().map(|p| p.to_path_buf());
            }
        }
    }

    // Try CARGO_MANIFEST_DIR (works during development)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let workspace = PathBuf::from(manifest_dir)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        if let Some(ws) = workspace {
            let candidate = ws.join("crates/agentzero-plugin-sdk");
            if candidate.join("Cargo.toml").exists() {
                return Some(candidate);
            }
        }
    }

    None
}
