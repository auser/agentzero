use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

// --- wasm_module ---

#[derive(Debug, Deserialize)]
struct WasmModuleInput {
    op: String,
    #[serde(default)]
    path: Option<String>,
}

/// Load and inspect WASM modules.
///
/// Operations:
/// - `inspect`: Read a .wasm file and return basic module info (size, header validation)
/// - `list`: List .wasm files in the workspace plugins directory
#[derive(Debug, Default, Clone, Copy)]
pub struct WasmModuleTool;

#[async_trait]
impl Tool for WasmModuleTool {
    fn name(&self) -> &'static str {
        "wasm_module"
    }

    fn description(&self) -> &'static str {
        "Inspect or list WASM modules in the workspace plugins directory."
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: WasmModuleInput =
            serde_json::from_str(input).context("wasm_module expects JSON: {\"op\", ...}")?;

        match req.op.as_str() {
            "inspect" => {
                let path_str = req
                    .path
                    .as_deref()
                    .ok_or_else(|| anyhow!("inspect requires a `path` field"))?;

                if path_str.trim().is_empty() {
                    return Err(anyhow!("path must not be empty"));
                }

                let full_path = resolve_wasm_path(&ctx.workspace_root, path_str);
                if !full_path.exists() {
                    return Err(anyhow!("WASM file not found: {}", full_path.display()));
                }

                let metadata = tokio::fs::metadata(&full_path)
                    .await
                    .context("failed to read WASM file metadata")?;

                let bytes = tokio::fs::read(&full_path)
                    .await
                    .context("failed to read WASM file")?;

                let valid_magic = bytes.len() >= 4 && bytes[..4] == [0x00, 0x61, 0x73, 0x6D];
                let version = if bytes.len() >= 8 {
                    Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
                } else {
                    None
                };

                let output = json!({
                    "path": full_path.display().to_string(),
                    "size_bytes": metadata.len(),
                    "valid_wasm_header": valid_magic,
                    "wasm_version": version,
                })
                .to_string();

                Ok(ToolResult { output })
            }
            "list" => {
                let plugins_dir = Path::new(&ctx.workspace_root).join(".agentzero/plugins");
                let mut wasm_files = Vec::new();

                if plugins_dir.exists() {
                    let mut entries = tokio::fs::read_dir(&plugins_dir)
                        .await
                        .context("failed to read plugins directory")?;

                    while let Some(entry) = entries.next_entry().await? {
                        let path = entry.path();
                        if path.extension().is_some_and(|ext| ext == "wasm") {
                            let meta = tokio::fs::metadata(&path).await.ok();
                            wasm_files.push(json!({
                                "name": path.file_name().unwrap_or_default().to_string_lossy(),
                                "path": path.display().to_string(),
                                "size_bytes": meta.map(|m| m.len()).unwrap_or(0),
                            }));
                        }
                    }
                }

                if wasm_files.is_empty() {
                    return Ok(ToolResult {
                        output: "no WASM modules found".to_string(),
                    });
                }

                Ok(ToolResult {
                    output: serde_json::to_string_pretty(&wasm_files)
                        .unwrap_or_else(|_| "[]".to_string()),
                })
            }
            other => Ok(ToolResult {
                output: json!({ "error": format!("unknown op: {other}") }).to_string(),
            }),
        }
    }
}

// --- wasm_tool ---

#[derive(Debug, Deserialize)]
struct WasmToolInput {
    module: String,
    #[serde(default)]
    function: Option<String>,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// Execute WASM-based tools via plugin runtime.
///
/// This tool loads a WASM module and invokes a function within it.
/// Currently validates the module path and reports that WASM execution
/// requires the `wasmtime` runtime (future integration).
#[derive(Debug, Default, Clone, Copy)]
pub struct WasmToolExecTool;

#[async_trait]
impl Tool for WasmToolExecTool {
    fn name(&self) -> &'static str {
        "wasm_tool"
    }

    fn description(&self) -> &'static str {
        "Execute a function within a WASM module."
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: WasmToolInput =
            serde_json::from_str(input).context("wasm_tool expects JSON: {\"module\", ...}")?;

        if req.module.trim().is_empty() {
            return Err(anyhow!("module must not be empty"));
        }

        let full_path = resolve_wasm_path(&ctx.workspace_root, &req.module);
        if !full_path.exists() {
            return Err(anyhow!("WASM module not found: {}", full_path.display()));
        }

        let bytes = tokio::fs::read(&full_path)
            .await
            .context("failed to read WASM module")?;

        let valid_magic = bytes.len() >= 4 && bytes[..4] == [0x00, 0x61, 0x73, 0x6D];
        if !valid_magic {
            return Err(anyhow!(
                "file is not a valid WASM module (invalid magic bytes)"
            ));
        }

        let function = req.function.as_deref().unwrap_or("_start");

        // WASM runtime execution is a future integration point.
        // For now, validate the module and report readiness.
        let output = json!({
            "module": full_path.display().to_string(),
            "function": function,
            "args": req.args,
            "size_bytes": bytes.len(),
            "valid": true,
            "status": "validated",
            "note": "WASM execution requires wasmtime runtime (not yet integrated)"
        })
        .to_string();

        Ok(ToolResult { output })
    }
}

fn resolve_wasm_path(workspace_root: &str, path: &str) -> std::path::PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(workspace_root).join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
    use std::fs;
    use std::path::PathBuf;
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
            "agentzero-wasm-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_minimal_wasm(dir: &Path, name: &str) -> PathBuf {
        // Minimal valid WASM: magic + version only (8 bytes)
        let path = dir.join(name);
        let wasm_header: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        fs::write(&path, wasm_header).expect("write wasm");
        path
    }

    // --- wasm_module tests ---

    #[tokio::test]
    async fn wasm_module_inspect_valid() {
        let dir = temp_dir();
        let wasm_path = write_minimal_wasm(&dir, "test.wasm");

        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let result = WasmModuleTool
            .execute(
                &format!(r#"{{"op": "inspect", "path": "{}"}}"#, wasm_path.display()),
                &ctx,
            )
            .await
            .expect("inspect should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["valid_wasm_header"], true);
        assert_eq!(v["wasm_version"], 1);
        assert_eq!(v["size_bytes"], 8);

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn wasm_module_inspect_not_found() {
        let ctx = ToolContext::new("/tmp".to_string());
        let err = WasmModuleTool
            .execute(r#"{"op": "inspect", "path": "nonexistent.wasm"}"#, &ctx)
            .await
            .expect_err("missing file should fail");
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn wasm_module_inspect_invalid_header() {
        let dir = temp_dir();
        let path = dir.join("bad.wasm");
        fs::write(&path, b"not a wasm file").unwrap();

        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let result = WasmModuleTool
            .execute(
                &format!(r#"{{"op": "inspect", "path": "{}"}}"#, path.display()),
                &ctx,
            )
            .await
            .expect("inspect should succeed even for invalid wasm");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["valid_wasm_header"], false);

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn wasm_module_list_empty() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = WasmModuleTool
            .execute(r#"{"op": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no WASM modules found"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn wasm_module_list_finds_files() {
        let dir = temp_dir();
        let plugins_dir = dir.join(".agentzero/plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        write_minimal_wasm(&plugins_dir, "plugin1.wasm");
        write_minimal_wasm(&plugins_dir, "plugin2.wasm");

        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let result = WasmModuleTool
            .execute(r#"{"op": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("plugin1.wasm"));
        assert!(result.output.contains("plugin2.wasm"));

        fs::remove_dir_all(dir).ok();
    }

    // --- wasm_tool tests ---

    #[tokio::test]
    async fn wasm_tool_validates_module() {
        let dir = temp_dir();
        let wasm_path = write_minimal_wasm(&dir, "tool.wasm");

        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let result = WasmToolExecTool
            .execute(
                &format!(
                    r#"{{"module": "{}", "function": "run"}}"#,
                    wasm_path.display()
                ),
                &ctx,
            )
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["valid"], true);
        assert_eq!(v["status"], "validated");
        assert_eq!(v["function"], "run");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn wasm_tool_rejects_missing_module() {
        let ctx = ToolContext::new("/tmp".to_string());
        let err = WasmToolExecTool
            .execute(r#"{"module": "missing.wasm"}"#, &ctx)
            .await
            .expect_err("missing module should fail");
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn wasm_tool_rejects_invalid_wasm() {
        let dir = temp_dir();
        let path = dir.join("bad.wasm");
        fs::write(&path, b"not wasm").unwrap();

        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let err = WasmToolExecTool
            .execute(&format!(r#"{{"module": "{}"}}"#, path.display()), &ctx)
            .await
            .expect_err("invalid wasm should fail");
        assert!(err.to_string().contains("invalid magic bytes"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn wasm_tool_empty_module_fails() {
        let ctx = ToolContext::new("/tmp".to_string());
        let err = WasmToolExecTool
            .execute(r#"{"module": ""}"#, &ctx)
            .await
            .expect_err("empty module should fail");
        assert!(err.to_string().contains("module must not be empty"));
    }
}
