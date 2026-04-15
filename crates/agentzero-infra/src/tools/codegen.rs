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
        // [workspace] prevents Cargo from treating this as part of the parent workspace.
        let mut cargo_toml = format!(
            r#"[package]
name = "agentzero-codegen-{tool_name}"
version = "0.1.0"
edition = "2021"

[workspace]

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
            // Unset coverage instrumentation flags so that wasm32-wasip1 builds
            // spawned by codegen tests don't inherit `-C instrument-coverage` from
            // cargo-llvm-cov, which is incompatible with the wasm target.
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-codegen-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Find the workspace root by walking up from CARGO_MANIFEST_DIR.
    fn workspace_root() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR should be set during tests");
        PathBuf::from(manifest)
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .expect("should find workspace root")
            .to_path_buf()
    }

    fn sdk_path() -> PathBuf {
        workspace_root().join("crates/agentzero-plugin-sdk")
    }

    #[test]
    fn scaffold_creates_project_structure() {
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let source = r#"
use agentzero_plugin_sdk::prelude::*;
declare_tool!("test_scaffold", handler);
fn handler(input: ToolInput) -> ToolOutput {
    ToolOutput::success("ok".to_string())
}
"#;

        let project = compiler
            .scaffold_project("test_scaffold", source, &[])
            .expect("scaffold should succeed");

        assert!(project.join("src/lib.rs").exists());
        assert!(project.join("Cargo.toml").exists());
        assert!(project.join(".cargo/config.toml").exists());

        let cargo_toml = std::fs::read_to_string(project.join("Cargo.toml")).expect("read");
        assert!(cargo_toml.contains("agentzero-plugin-sdk"));
        assert!(cargo_toml.contains("[workspace]"));
        assert!(cargo_toml.contains("cdylib"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn scaffold_includes_extra_deps() {
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let project = compiler
            .scaffold_project(
                "with_deps",
                "fn x(){}",
                &[("regex", "1"), ("chrono", "0.4")],
            )
            .expect("scaffold");

        let cargo_toml = std::fs::read_to_string(project.join("Cargo.toml")).expect("read");
        assert!(cargo_toml.contains("regex = \"1\""));
        assert!(cargo_toml.contains("chrono = \"0.4\""));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn scaffold_rejects_unlisted_deps() {
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let project = compiler
            .scaffold_project("reject_deps", "fn x(){}", &[("tokio", "1"), ("regex", "1")])
            .expect("scaffold");

        let cargo_toml = std::fs::read_to_string(project.join("Cargo.toml")).expect("read");
        assert!(cargo_toml.contains("regex"));
        assert!(!cargo_toml.contains("tokio"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hash_source_deterministic() {
        let h1 = CodegenCompiler::hash_source("hello world");
        let h2 = CodegenCompiler::hash_source("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars

        let h3 = CodegenCompiler::hash_source("different");
        assert_ne!(h1, h3);
    }

    #[test]
    fn gc_removes_stale_dirs() {
        let dir = temp_dir();
        let codegen_dir = dir.join("codegen");
        std::fs::create_dir_all(codegen_dir.join("active_tool")).expect("mkdir");
        std::fs::create_dir_all(codegen_dir.join("stale_tool")).expect("mkdir");
        std::fs::create_dir_all(codegen_dir.join(".target")).expect("mkdir");

        let compiler = CodegenCompiler {
            codegen_dir: codegen_dir.clone(),
            sdk_path: None,
        };

        let removed = compiler
            .gc(&["active_tool".to_string()])
            .expect("gc should work");

        assert_eq!(removed, 1);
        assert!(codegen_dir.join("active_tool").exists());
        assert!(!codegen_dir.join("stale_tool").exists());
        assert!(codegen_dir.join(".target").exists()); // .target preserved

        std::fs::remove_dir_all(dir).ok();
    }

    // ── End-to-end: compile + execute WASM from source ────────────────
    //
    // These tests require `wasm32-wasip1` target and (for execution)
    // the `wasm-plugins` feature. Run with:
    //   cargo nextest run -p agentzero-infra --features wasm-plugins -E 'test(codegen)'

    #[tokio::test]
    #[cfg(feature = "wasm-plugins")]
    async fn check_toolchain_passes() {
        let dir = temp_dir();
        let compiler = CodegenCompiler::new(&dir);
        compiler
            .check_toolchain()
            .await
            .expect("wasm32-wasip1 should be installed");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    #[cfg(feature = "wasm-plugins")]
    async fn compile_and_execute_codegen_tool() {
        // Coverage instrumentation flags are incompatible with wasm compilation.
        if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
            eprintln!("skipping: wasm codegen incompatible with llvm-cov instrumentation");
            return;
        }
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let source = r#"use agentzero_plugin_sdk::prelude::*;

declare_tool!("reverse_string", handler);

fn handler(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let text = req["text"].as_str().unwrap_or("");
    let reversed: String = text.chars().rev().collect();
    ToolOutput::success(reversed)
}
"#;

        // Build
        let (wasm_path, wasm_sha256, source_hash) = compiler
            .build_tool("reverse_string", source, &[])
            .await
            .expect("compilation should succeed");

        assert!(
            wasm_path.exists(),
            "WASM file should exist at {}",
            wasm_path.display()
        );
        assert_eq!(wasm_sha256.len(), 64, "SHA-256 should be 64 hex chars");
        assert_eq!(source_hash.len(), 64);

        // Verify hash is correct
        let recomputed = CodegenCompiler::compute_hash(&wasm_path).expect("hash should work");
        assert_eq!(wasm_sha256, recomputed);

        // Execute
        let ctx = agentzero_core::ToolContext::new(dir.to_string_lossy().to_string());
        let result =
            execute_codegen_tool(&wasm_path.to_string_lossy(), r#"{"text": "hello"}"#, &ctx)
                .await
                .expect("execution should succeed");

        assert_eq!(result.output, "olleh");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    #[cfg(feature = "wasm-plugins")]
    async fn compile_with_extra_deps() {
        // Coverage instrumentation flags are incompatible with wasm compilation.
        if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
            eprintln!("skipping: wasm codegen incompatible with llvm-cov instrumentation");
            return;
        }
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let source = r#"// deps: regex
use agentzero_plugin_sdk::prelude::*;

declare_tool!("regex_match", handler);

fn handler(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let pattern = req["pattern"].as_str().unwrap_or(".*");
    let text = req["text"].as_str().unwrap_or("");

    match regex::Regex::new(pattern) {
        Ok(re) => {
            let matched = re.is_match(text);
            ToolOutput::success(format!("{matched}"))
        }
        Err(e) => ToolOutput::error(format!("invalid regex: {e}")),
    }
}
"#;

        let extra_deps = crate::tools::codegen::extract_deps_from_source(source);
        let (wasm_path, _, _) = compiler
            .build_tool("regex_match", source, &extra_deps)
            .await
            .expect("compilation with regex dep should succeed");

        // Execute
        let ctx = agentzero_core::ToolContext::new(dir.to_string_lossy().to_string());
        let result = execute_codegen_tool(
            &wasm_path.to_string_lossy(),
            r#"{"pattern": "^he", "text": "hello"}"#,
            &ctx,
        )
        .await
        .expect("regex tool should execute");

        assert_eq!(result.output, "true");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn compile_failure_returns_error() {
        let dir = temp_dir();
        let compiler = CodegenCompiler {
            codegen_dir: dir.join("codegen"),
            sdk_path: Some(sdk_path()),
        };

        let bad_source = r#"
use agentzero_plugin_sdk::prelude::*;
declare_tool!("broken", handler);
fn handler(input: ToolInput) -> ToolOutput {
    let x: i32 = "not a number"; // type error
    ToolOutput::success(format!("{x}"))
}
"#;

        let result = compiler.build_tool("broken", bad_source, &[]).await;

        assert!(result.is_err(), "compilation of bad source should fail");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("compilation failed"),
            "error should mention compilation: {err}"
        );

        std::fs::remove_dir_all(dir).ok();
    }
}
