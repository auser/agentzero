//! LLM-callable tool for scaffolding, building, and deploying WASM plugins.
//!
//! Generates a complete Rust project that compiles to a WASM plugin using the
//! `agentzero-plugin-sdk` crate and the `declare_tool!` macro.

use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{bail, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tool_logic: Option<String>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PluginScaffoldTool;

impl PluginScaffoldTool {
    fn plugins_dir(ctx: &ToolContext) -> PathBuf {
        PathBuf::from(&ctx.workspace_root)
            .join(".agentzero")
            .join("plugins")
    }
}

#[async_trait]
impl Tool for PluginScaffoldTool {
    fn name(&self) -> &'static str {
        "plugin_scaffold"
    }

    fn description(&self) -> &'static str {
        "Scaffold, build, and deploy WASM plugin tools. Actions: scaffold (generate a Rust \
         plugin project), list (show scaffolded plugins), build (compile to wasm32-wasip1), \
         deploy (copy wasm + generate manifest), status (check plugin state)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["scaffold", "list", "build", "deploy", "status"],
                    "description": "The plugin operation to perform"
                },
                "name": {
                    "type": "string",
                    "description": "Plugin/tool name (used as crate name and tool identifier)"
                },
                "description": {
                    "type": "string",
                    "description": "For scaffold: what the tool does"
                },
                "tool_logic": {
                    "type": "string",
                    "description": "For scaffold: the Rust code body for the tool's execute function. Receives `input: &str` and returns `Result<String, String>`."
                },
                "capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "For scaffold: plugin capabilities (e.g. ['network', 'fs_read'])"
                },
                "version": {
                    "type": "string",
                    "description": "For scaffold: semver version (default: 0.1.0)"
                }
            },
            "required": ["action"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: Input = serde_json::from_str(input).context("invalid plugin_scaffold input")?;

        if ctx.depth > 0 {
            bail!("plugin_scaffold is not available to sub-agents (depth > 0)");
        }

        let plugins_dir = Self::plugins_dir(ctx);

        match req.action.as_str() {
            "scaffold" => action_scaffold(&plugins_dir, req),
            "list" => action_list(&plugins_dir),
            "build" => action_build(&plugins_dir, req.name).await,
            "deploy" => action_deploy(&plugins_dir, req.name),
            "status" => action_status(&plugins_dir, req.name),
            other => bail!("unknown plugin_scaffold action: {other}"),
        }
    }
}

fn validate_plugin_name(name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        bail!("plugin name cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("plugin name must be alphanumeric with hyphens/underscores only");
    }
    Ok(())
}

fn action_scaffold(plugins_dir: &Path, req: Input) -> anyhow::Result<ToolResult> {
    let name = req.name.context("'name' is required for scaffold")?;
    let description = req
        .description
        .unwrap_or_else(|| format!("A WASM plugin tool: {name}"));
    let version = req.version.unwrap_or_else(|| "0.1.0".to_string());
    let capabilities = req.capabilities.unwrap_or_default();
    let tool_logic = req.tool_logic.unwrap_or_else(|| {
        r#"    // Parse input JSON
    let _input: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("invalid input: {e}"))?;

    // TODO: implement tool logic here

    Ok(format!("{{\"result\": \"hello from {}\"}}", env!("CARGO_PKG_NAME")))"#
            .to_string()
    });

    validate_plugin_name(&name)?;

    let project_dir = plugins_dir.join(&name);
    if project_dir.exists() {
        bail!("plugin directory already exists: {}", project_dir.display());
    }

    std::fs::create_dir_all(project_dir.join("src"))
        .context("failed to create plugin project directory")?;

    // Generate Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "{version}"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
agentzero-plugin-sdk = {{ path = "../../../../crates/agentzero-plugin-sdk" }}
serde_json = "1"
"#
    );
    std::fs::write(project_dir.join("Cargo.toml"), &cargo_toml)
        .context("failed to write Cargo.toml")?;

    // Sanitize the name for use as a Rust function name
    let fn_name = name.replace('-', "_");

    // Generate src/lib.rs
    let lib_rs = format!(
        r#"use agentzero_plugin_sdk::declare_tool;

fn {fn_name}_handler(input: &str) -> Result<String, String> {{
{tool_logic}
}}

declare_tool!("{name}", {fn_name}_handler);
"#
    );
    std::fs::write(project_dir.join("src").join("lib.rs"), &lib_rs)
        .context("failed to write src/lib.rs")?;

    // Generate manifest.json
    let manifest = serde_json::json!({
        "id": name,
        "version": version,
        "description": description,
        "entrypoint": format!("{fn_name}_handler"),
        "wasm_file": format!("target/wasm32-wasip1/release/{fn_name}.wasm"),
        "wasm_sha256": "",
        "capabilities": capabilities,
        "hooks": [],
        "min_runtime_api": "2",
        "max_runtime_api": "2",
        "allowed_host_calls": [],
        "dependencies": {}
    });
    std::fs::write(
        project_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).expect("json serialization should not fail"),
    )
    .context("failed to write manifest.json")?;

    Ok(ToolResult {
        output: format!(
            "Plugin '{}' scaffolded at {}.\n\
             Files created:\n  - Cargo.toml\n  - src/lib.rs\n  - manifest.json\n\n\
             Next steps:\n  1. Edit src/lib.rs to implement your tool logic\n  \
             2. Use plugin_scaffold with action 'build' to compile\n  \
             3. Use plugin_scaffold with action 'deploy' to install",
            name,
            project_dir.display()
        ),
    })
}

fn action_list(plugins_dir: &Path) -> anyhow::Result<ToolResult> {
    if !plugins_dir.exists() {
        return Ok(ToolResult {
            output: "No plugins directory found. Use 'scaffold' to create your first plugin."
                .to_string(),
        });
    }

    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(plugins_dir).context("failed to read plugins directory")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let has_manifest = entry.path().join("manifest.json").exists();
            let wasm_built = find_wasm_file(&entry.path()).is_some();
            plugins.push(format!(
                "  - {} (manifest: {}, built: {})",
                name, has_manifest, wasm_built
            ));
        }
    }

    if plugins.is_empty() {
        return Ok(ToolResult {
            output: "No scaffolded plugins found.".to_string(),
        });
    }

    Ok(ToolResult {
        output: format!("{} plugin(s) found:\n{}", plugins.len(), plugins.join("\n")),
    })
}

async fn action_build(plugins_dir: &Path, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for build")?;
    let project_dir = plugins_dir.join(&name);
    if !project_dir.exists() {
        bail!("plugin '{}' not found at {}", name, project_dir.display());
    }

    let output = tokio::process::Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip1", "--release"])
        .current_dir(&project_dir)
        .output()
        .await
        .context("failed to run cargo build")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(ToolResult {
            output: format!(
                "Plugin '{}' built successfully.\n\nstdout:\n{}\nstderr:\n{}",
                name, stdout, stderr
            ),
        })
    } else {
        Ok(ToolResult {
            output: format!(
                "Build failed for plugin '{}'.\n\nstdout:\n{}\nstderr:\n{}",
                name, stdout, stderr
            ),
        })
    }
}

fn action_deploy(plugins_dir: &Path, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for deploy")?;
    let project_dir = plugins_dir.join(&name);
    if !project_dir.exists() {
        bail!("plugin '{}' not found at {}", name, project_dir.display());
    }

    let wasm_path = find_wasm_file(&project_dir).with_context(|| {
        format!(
            "no .wasm file found for plugin '{}' — run build first",
            name
        )
    })?;

    // Compute SHA256
    let wasm_bytes = std::fs::read(&wasm_path).context("failed to read wasm file")?;
    let sha256 = sha256_hex(&wasm_bytes);

    // Update manifest with correct wasm path and sha256
    let manifest_path = project_dir.join("manifest.json");
    let manifest_str =
        std::fs::read_to_string(&manifest_path).context("failed to read manifest.json")?;
    let mut manifest: serde_json::Value =
        serde_json::from_str(&manifest_str).context("failed to parse manifest.json")?;

    let wasm_filename = wasm_path
        .file_name()
        .context("wasm file has no name")?
        .to_string_lossy()
        .to_string();

    manifest["wasm_file"] = serde_json::Value::String(wasm_filename.clone());
    manifest["wasm_sha256"] = serde_json::Value::String(sha256.clone());

    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("json serialization should not fail"),
    )
    .context("failed to write updated manifest.json")?;

    // Copy wasm to the plugin directory root (next to manifest.json)
    let deploy_wasm = project_dir.join(&wasm_filename);
    if deploy_wasm != wasm_path {
        std::fs::copy(&wasm_path, &deploy_wasm).context("failed to copy wasm to plugin dir")?;
    }

    Ok(ToolResult {
        output: format!(
            "Plugin '{}' deployed.\n  manifest: {}\n  wasm: {} ({} bytes, sha256: {})\n\n\
             The plugin will be available on next agent startup with wasm-plugins enabled.",
            name,
            manifest_path.display(),
            deploy_wasm.display(),
            wasm_bytes.len(),
            &sha256[..16],
        ),
    })
}

fn action_status(plugins_dir: &Path, name: Option<String>) -> anyhow::Result<ToolResult> {
    let name = name.context("'name' is required for status")?;
    let project_dir = plugins_dir.join(&name);
    if !project_dir.exists() {
        return Ok(ToolResult {
            output: format!("Plugin '{}' not found.", name),
        });
    }

    let has_cargo = project_dir.join("Cargo.toml").exists();
    let has_manifest = project_dir.join("manifest.json").exists();
    let wasm_file = find_wasm_file(&project_dir);

    let mut status = format!("Plugin '{}' status:\n", name);
    status.push_str(&format!(
        "  Cargo.toml: {}\n",
        if has_cargo { "yes" } else { "no" }
    ));
    status.push_str(&format!(
        "  manifest.json: {}\n",
        if has_manifest { "yes" } else { "no" }
    ));
    status.push_str(&format!(
        "  wasm binary: {}\n",
        if let Some(ref p) = wasm_file {
            format!("yes ({})", p.display())
        } else {
            "no (run build first)".to_string()
        }
    ));

    if has_manifest {
        if let Ok(content) = std::fs::read_to_string(project_dir.join("manifest.json")) {
            if let Ok(m) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
                    status.push_str(&format!("  version: {v}\n"));
                }
                if let Some(caps) = m.get("capabilities").and_then(|v| v.as_array()) {
                    let cap_strs: Vec<&str> = caps.iter().filter_map(|c| c.as_str()).collect();
                    status.push_str(&format!("  capabilities: [{}]\n", cap_strs.join(", ")));
                }
            }
        }
    }

    Ok(ToolResult { output: status })
}

/// Find the compiled wasm file in the project's target directory.
fn find_wasm_file(project_dir: &std::path::Path) -> Option<PathBuf> {
    let release_dir = project_dir
        .join("target")
        .join("wasm32-wasip1")
        .join("release");
    if !release_dir.exists() {
        // Also check if there's a deployed wasm next to manifest
        if let Ok(entries) = std::fs::read_dir(project_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "wasm") {
                    return Some(path);
                }
            }
        }
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(&release_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "wasm") {
                return Some(path);
            }
        }
    }
    None
}

/// Compute SHA256 hex digest without external dependencies.
fn sha256_hex(data: &[u8]) -> String {
    use std::process::Command;
    // Use system sha256sum/shasum since we don't want to add a crypto dependency
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(data);
            }
            child.wait_with_output()
        });

    match output {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            s.split_whitespace().next().unwrap_or("unknown").to_string()
        }
        Err(_) => "unknown".to_string(),
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
            "agentzero-plugmgr-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn make_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext::new(dir.to_string_lossy().to_string())
    }

    #[tokio::test]
    async fn scaffold_creates_project() {
        let dir = temp_dir();
        let ctx = make_ctx(&dir);
        let tool = PluginScaffoldTool;

        let input = serde_json::json!({
            "action": "scaffold",
            "name": "dns_lookup",
            "description": "Look up DNS records for a domain"
        });
        let result = tool
            .execute(&serde_json::to_string(&input).expect("json"), &ctx)
            .await
            .expect("scaffold should succeed");
        assert!(result.output.contains("scaffolded"));

        let plugin_dir = dir.join(".agentzero").join("plugins").join("dns_lookup");
        assert!(plugin_dir.join("Cargo.toml").exists());
        assert!(plugin_dir.join("src").join("lib.rs").exists());
        assert!(plugin_dir.join("manifest.json").exists());

        // Verify lib.rs content
        let lib_rs = fs::read_to_string(plugin_dir.join("src").join("lib.rs")).expect("read");
        assert!(lib_rs.contains("declare_tool!"));
        assert!(lib_rs.contains("dns_lookup"));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn scaffold_rejects_duplicate() {
        let dir = temp_dir();
        let ctx = make_ctx(&dir);
        let tool = PluginScaffoldTool;

        let input = serde_json::json!({
            "action": "scaffold",
            "name": "my_tool",
            "description": "test"
        });
        let input_str = serde_json::to_string(&input).expect("json");

        tool.execute(&input_str, &ctx)
            .await
            .expect("first scaffold should succeed");

        let err = tool
            .execute(&input_str, &ctx)
            .await
            .expect_err("duplicate should fail");
        assert!(err.to_string().contains("already exists"));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn list_empty() {
        let dir = temp_dir();
        let ctx = make_ctx(&dir);
        let tool = PluginScaffoldTool;

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("No plugins directory"));

        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn depth_blocks_sub_agents() {
        let dir = temp_dir();
        let mut ctx = make_ctx(&dir);
        ctx.depth = 1;
        let tool = PluginScaffoldTool;

        let err = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect_err("sub-agent should be blocked");
        assert!(err.to_string().contains("not available to sub-agents"));
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
