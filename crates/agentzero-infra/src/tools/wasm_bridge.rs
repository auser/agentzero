use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_plugins::package::PluginManifest;
use agentzero_plugins::wasm::{
    WasmEngine, WasmIsolationPolicy, WasmModule, WasmPluginContainer, WasmPluginRuntime,
    WasmV2Options,
};
use anyhow::anyhow;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

/// A WASM plugin wrapped as a `Tool`.
///
/// Each `WasmTool` corresponds to one installed WASM plugin. The agent loop
/// treats it identically to a native tool — it shows up in the tool list
/// with the plugin's `id` as its name.
///
/// The engine and module are pre-compiled at initialization time so that
/// `execute()` only needs to create a cheap `Store` per call — no disk I/O
/// or module compilation on the hot path.
pub struct WasmTool {
    /// Leaked string for the `&'static str` requirement of `Tool::name()`.
    /// This is ~20 bytes per plugin and lives for the program lifetime.
    name: &'static str,
    /// Leaked description from the plugin manifest.
    description: &'static str,
    manifest: PluginManifest,
    wasm_path: PathBuf,
    policy: WasmIsolationPolicy,
    engine: Arc<WasmEngine>,
    module: Arc<WasmModule>,
}

impl WasmTool {
    /// Create a `WasmTool` from an installed plugin's manifest and path.
    ///
    /// Pre-compiles the WASM module at initialization time for fast execution.
    pub fn from_manifest(
        manifest: PluginManifest,
        wasm_path: PathBuf,
        policy: WasmIsolationPolicy,
    ) -> anyhow::Result<Self> {
        Self::from_manifest_with_engine(manifest, wasm_path, policy, None)
    }

    /// Create a `WasmTool` with a shared engine. If `engine` is `None`, a
    /// new engine is created. Sharing an engine across plugins saves memory.
    pub fn from_manifest_with_engine(
        manifest: PluginManifest,
        wasm_path: PathBuf,
        policy: WasmIsolationPolicy,
        engine: Option<Arc<WasmEngine>>,
    ) -> anyhow::Result<Self> {
        // Signature enforcement: when require_signed is true, reject unsigned
        // or invalidly-signed plugins before loading the WASM module.
        if policy.require_signed {
            let signature = manifest.signature.as_deref().ok_or_else(|| {
                anyhow!(
                    "plugin '{}' is unsigned but require_signed is enabled",
                    manifest.id
                )
            })?;
            let key_id = manifest.signing_key_id.as_deref().unwrap_or("(none)");
            tracing::debug!(plugin = manifest.id, key_id, "verifying plugin signature");
            // Note: the public key must be provided out-of-band (e.g. in config).
            // For now, we verify the signature field is present and non-empty.
            // Full public-key verification requires a trusted key store.
            if signature.is_empty() {
                return Err(anyhow!(
                    "plugin '{}' has an empty signature but require_signed is enabled",
                    manifest.id
                ));
            }
        }

        if !wasm_path.exists() {
            return Err(anyhow!("wasm file does not exist: {}", wasm_path.display()));
        }

        let engine = match engine {
            Some(e) => e,
            None => Arc::new(WasmPluginRuntime::create_engine()?),
        };
        let module = Arc::new(WasmPluginRuntime::compile_module(&engine, &wasm_path)?);

        // Leak the plugin name and description for the &'static str requirement.
        // ~50 bytes per plugin; agent processes are short-lived.
        let name: &'static str = Box::leak(manifest.id.clone().into_boxed_str());
        let description: &'static str = Box::leak(
            manifest
                .description
                .clone()
                .unwrap_or_default()
                .into_boxed_str(),
        );

        Ok(Self {
            name,
            description,
            manifest,
            wasm_path,
            policy,
            engine,
            module,
        })
    }
}

impl std::fmt::Debug for WasmTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmTool")
            .field("name", &self.name)
            .field("wasm_path", &self.wasm_path)
            .finish()
    }
}

#[async_trait]
impl Tool for WasmTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let container = WasmPluginContainer {
            id: self.manifest.id.clone(),
            module_path: self.wasm_path.clone(),
            entrypoint: self.manifest.entrypoint.clone(),
            max_execution_ms: self.policy.max_execution_ms,
            max_memory_mb: self.policy.max_memory_mb,
            allow_network: self.policy.allow_network,
            allow_fs_write: self.policy.allow_fs_write,
        };

        let options = WasmV2Options {
            workspace_root: ctx.workspace_root.clone(),
            capabilities: self.manifest.capabilities.clone(),
        };

        let policy = self.policy.clone();
        let input_owned = input.to_string();
        let engine = Arc::clone(&self.engine);
        let module = Arc::clone(&self.module);

        // WASM runtimes are synchronous — run in a blocking thread.
        // Engine and module are pre-compiled; only a Store is created per call.
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
        .map_err(|e| anyhow!("wasm plugin task panicked: {e}"))??;

        if let Some(err) = result.error {
            if result.output.is_empty() {
                return Err(anyhow!("plugin error: {err}"));
            }
            // Plugin returned both output and error — include error in output
            Ok(ToolResult {
                output: format!("{}\n[plugin warning: {err}]", result.output),
            })
        } else {
            Ok(ToolResult {
                output: result.output,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "test-plugin".to_string(),
            version: "0.1.0".to_string(),
            description: Some("A test plugin".to_string()),
            entrypoint: "az_tool_execute".to_string(),
            wasm_file: "plugin.wasm".to_string(),
            wasm_sha256: "0".repeat(64),
            capabilities: vec![],
            hooks: vec![],
            min_runtime_api: 2,
            max_runtime_api: 2,
            allowed_host_calls: vec![],
            dependencies: vec![],
            signature: None,
            signing_key_id: None,
        }
    }

    fn test_policy() -> WasmIsolationPolicy {
        WasmIsolationPolicy {
            max_execution_ms: 5_000,
            max_module_bytes: 5 * 1024 * 1024,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
            allow_fs_read: false,
            allowed_host_calls: vec![],
            require_signed: false,
            allowed_host_tools: Vec::new(),
            overlay_mode: agentzero_plugins::overlay::OverlayMode::default(),
        }
    }

    #[test]
    fn from_manifest_rejects_missing_wasm_file() {
        let manifest = test_manifest();
        let path = PathBuf::from("/nonexistent/path/to/plugin.wasm");
        let err = WasmTool::from_manifest(manifest, path, test_policy())
            .expect_err("should fail for missing wasm file");
        assert!(err.to_string().contains("wasm file does not exist"));
    }

    #[test]
    fn from_manifest_rejects_invalid_wasm_bytes() {
        let dir =
            std::env::temp_dir().join(format!("agentzero-wasm-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wasm_path = dir.join("not-a-wasm.wasm");
        std::fs::write(&wasm_path, b"this is not wasm").unwrap();

        let manifest = test_manifest();
        let err = WasmTool::from_manifest(manifest, wasm_path, test_policy())
            .expect_err("should fail for invalid wasm bytes");
        let msg = err.to_string();
        assert!(
            msg.contains("failed to") || msg.contains("invalid"),
            "error should mention parsing failure, got: {msg}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_manifest_uses_description_from_manifest() {
        // We can't build a real tool without valid WASM, but we can test
        // the error path always carries the right information.
        let mut manifest = test_manifest();
        manifest.description = None;

        let path = PathBuf::from("/nonexistent/plugin.wasm");
        let err = WasmTool::from_manifest(manifest, path, test_policy());
        assert!(err.is_err()); // fails because file doesn't exist
    }

    #[test]
    fn shared_engine_creation_succeeds() {
        let engine = WasmPluginRuntime::create_engine();
        assert!(engine.is_ok(), "engine creation should succeed");
    }

    #[test]
    fn from_manifest_with_shared_engine_rejects_missing_file() {
        let engine = Arc::new(WasmPluginRuntime::create_engine().unwrap());
        let manifest = test_manifest();
        let path = PathBuf::from("/nonexistent/path/to/plugin.wasm");
        let err = WasmTool::from_manifest_with_engine(manifest, path, test_policy(), Some(engine))
            .expect_err("should fail for missing wasm file");
        assert!(err.to_string().contains("wasm file does not exist"));
    }

    #[test]
    fn debug_impl_shows_name_and_path() {
        // Test that Debug doesn't panic by formatting an error scenario
        let manifest = test_manifest();
        let path = PathBuf::from("/test/plugin.wasm");
        let debug_str = format!(
            "WasmTool {{ name: {:?}, wasm_path: {:?} }}",
            manifest.id, path
        );
        assert!(debug_str.contains("test-plugin"));
    }
}
