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
        if !wasm_path.exists() {
            return Err(anyhow!("wasm file does not exist: {}", wasm_path.display()));
        }

        let engine = match engine {
            Some(e) => e,
            None => Arc::new(WasmPluginRuntime::create_engine()?),
        };
        let module = Arc::new(WasmPluginRuntime::compile_module(&engine, &wasm_path)?);

        // Leak the plugin name for the &'static str requirement.
        // ~20 bytes per plugin; agent processes are short-lived.
        let name: &'static str = Box::leak(manifest.id.clone().into_boxed_str());

        Ok(Self {
            name,
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
        "WASM plugin tool (description provided by plugin manifest)"
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
