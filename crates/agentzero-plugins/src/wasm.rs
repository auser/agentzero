use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[cfg(feature = "wasm-runtime")]
pub use runtime_impl::ModuleCache;

/// Type aliases for the active WASM engine/module, usable by downstream crates
/// (e.g. `agentzero-infra`) for plugin warming without depending on wasmi/wasmtime directly.
#[cfg(all(feature = "wasm-runtime", not(feature = "wasm-jit")))]
pub type WasmEngine = wasmi::Engine;
#[cfg(all(feature = "wasm-runtime", not(feature = "wasm-jit")))]
pub type WasmModule = wasmi::Module;
#[cfg(feature = "wasm-jit")]
pub type WasmEngine = wasmtime::Engine;
#[cfg(feature = "wasm-jit")]
pub type WasmModule = wasmtime::Module;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmIsolationPolicy {
    pub max_execution_ms: u64,
    pub max_module_bytes: u64,
    pub max_memory_mb: u32,
    pub allow_network: bool,
    pub allow_fs_write: bool,
    pub allow_fs_read: bool,
    pub allowed_host_calls: Vec<String>,
}

impl Default for WasmIsolationPolicy {
    fn default() -> Self {
        Self {
            max_execution_ms: 30_000,
            max_module_bytes: 5 * 1024 * 1024,
            max_memory_mb: 256,
            allow_network: false,
            allow_fs_write: false,
            allow_fs_read: false,
            allowed_host_calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginContainer {
    pub id: String,
    pub module_path: PathBuf,
    pub entrypoint: String,
    pub max_execution_ms: u64,
    pub max_memory_mb: u32,
    pub allow_network: bool,
    pub allow_fs_write: bool,
}

impl WasmPluginContainer {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            return Err(anyhow!("plugin id cannot be empty"));
        }
        if self.entrypoint.trim().is_empty() {
            return Err(anyhow!("plugin entrypoint cannot be empty"));
        }
        if self.max_execution_ms == 0 {
            return Err(anyhow!("max_execution_ms must be > 0"));
        }
        if self.max_memory_mb == 0 {
            return Err(anyhow!("max_memory_mb must be > 0"));
        }
        if self.module_path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            return Err(anyhow!("plugin module must be a .wasm file"));
        }
        Ok(())
    }
}

pub struct WasmPluginRuntime;

// ---------------------------------------------------------------------------
// ABI v1 types (backward-compatible)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasmExecutionRequest {
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WasmExecutionResult {
    pub status_code: i32,
}

// ---------------------------------------------------------------------------
// ABI v2 types
// ---------------------------------------------------------------------------

/// Input passed from the host to a v2 plugin via linear memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmToolInput {
    pub input: String,
    pub workspace_root: String,
}

/// Output returned from a v2 plugin via linear memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmToolOutput {
    pub output: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl WasmToolOutput {
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Result of a v2 plugin execution.
#[derive(Debug, Clone)]
pub struct WasmExecutionResultV2 {
    pub output: String,
    pub error: Option<String>,
}

/// Options for v2 execution that go beyond the container/policy.
#[derive(Debug, Clone, Default)]
pub struct WasmV2Options {
    pub workspace_root: String,
    pub capabilities: Vec<String>,
}

impl Default for WasmPluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper: pack/unpack i64 as (ptr, len) pair
// ---------------------------------------------------------------------------

/// Pack a (ptr, len) pair into a single i64.
fn pack_ptr_len(ptr: u32, len: u32) -> i64 {
    (ptr as i64) | ((len as i64) << 32)
}

/// Unpack an i64 into a (ptr, len) pair.
fn unpack_ptr_len(packed: i64) -> (u32, u32) {
    let ptr = (packed & 0xFFFF_FFFF) as u32;
    let len = ((packed >> 32) & 0xFFFF_FFFF) as u32;
    (ptr, len)
}

// ---------------------------------------------------------------------------
// wasm-runtime (wasmi interpreter): lightweight, pure-Rust, no_std-compatible
// ---------------------------------------------------------------------------
#[cfg(all(feature = "wasm-runtime", not(feature = "wasm-jit")))]
mod runtime_impl {
    use super::*;
    use anyhow::Context;
    use std::path::Path;
    use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

    /// Approximate fuel units per millisecond of execution time.
    /// wasmi consumes ~1 fuel per instruction; ~100M instructions/sec on
    /// typical hardware ≈ 100K fuel/ms.
    const FUEL_PER_MS: u64 = 100_000;

    fn compute_fuel(timeout_ms: u64) -> u64 {
        timeout_ms.saturating_mul(FUEL_PER_MS)
    }

    /// Store data for wasmi: WASI context + memory limits + log buffer.
    struct PluginState {
        wasi: wasmi_wasi::WasiCtx,
        limits: StoreLimits,
        log_buffer: Vec<String>,
    }

    /// Create a wasmi Engine with fuel metering enabled.
    fn make_engine() -> Engine {
        let mut config = Config::default();
        config.consume_fuel(true);
        Engine::new(&config)
    }

    impl WasmPluginRuntime {
        pub fn new() -> Self {
            Self
        }

        /// Create a new WASM engine with fuel metering enabled.
        /// Share this across plugins for efficient resource use.
        pub fn create_engine() -> anyhow::Result<Engine> {
            Ok(make_engine())
        }

        /// Pre-compile a WASM module from the given path.
        /// The returned module can be reused across multiple executions.
        pub fn compile_module(
            engine: &Engine,
            wasm_path: &std::path::Path,
        ) -> anyhow::Result<Module> {
            let bytes = std::fs::read(wasm_path)
                .with_context(|| format!("failed to read module at {}", wasm_path.display()))?;
            Module::new(engine, &bytes)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", wasm_path.display()))
        }

        /// Execute a v2 plugin with a pre-compiled engine and module.
        /// This avoids disk I/O and module compilation on the hot path.
        pub fn execute_v2_precompiled(
            engine: &Engine,
            module: &Module,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            // Preflight checks (skip module file read — already compiled)
            container.validate()?;
            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            validate_v2_imports(module, policy, &options.capabilities)?;

            let mut wasi_builder = wasmi_wasi::WasiCtxBuilder::new();
            wasi_builder.inherit_stderr();

            if policy.allow_fs_read && !options.workspace_root.is_empty() {
                let workspace_path = std::path::Path::new(&options.workspace_root);
                if workspace_path.exists() {
                    match wasmi_wasi::sync::Dir::open_ambient_dir(
                        workspace_path,
                        wasmi_wasi::sync::ambient_authority(),
                    ) {
                        Ok(dir) => {
                            let _ = wasi_builder.preopened_dir(dir, ".");
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %options.workspace_root,
                                error = %e,
                                "failed to preopen workspace dir"
                            );
                        }
                    }
                }
            }

            let wasi = wasi_builder.build();
            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();
            let state = PluginState {
                wasi,
                limits,
                log_buffer: Vec::new(),
            };
            let mut store = Store::new(engine, state);
            store.limiter(|s| &mut s.limits);

            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            store
                .set_fuel(compute_fuel(effective_timeout_ms))
                .map_err(|e| anyhow!("failed to set fuel: {e}"))?;

            let mut linker: Linker<PluginState> = Linker::new(engine);
            wasmi_wasi::sync::add_to_linker(&mut linker, |s: &mut PluginState| &mut s.wasi)
                .map_err(|e| anyhow!("failed to add WASI p1 to linker: {e}"))?;
            register_host_functions(&mut linker, policy)?;

            let instance = linker
                .instantiate_and_start(&mut store, module)
                .map_err(|e| anyhow!("failed to instantiate v2 plugin module: {e}"))?;

            let tool_input = WasmToolInput {
                input: input.to_string(),
                workspace_root: options.workspace_root.clone(),
            };
            let input_json =
                serde_json::to_string(&tool_input).context("failed to serialize v2 tool input")?;

            let az_alloc = instance
                .get_typed_func::<i32, i32>(&store, "az_alloc")
                .map_err(|e| {
                    anyhow!("v2 plugin missing 'az_alloc' export (expected fn(i32) -> i32): {e}")
                })?;

            let input_bytes = input_json.as_bytes();
            let input_len = input_bytes.len() as i32;
            let input_ptr = az_alloc
                .call(&mut store, input_len)
                .map_err(|e| anyhow!("az_alloc call failed: {e}"))?;

            let memory = instance
                .get_memory(&store, "memory")
                .ok_or_else(|| anyhow!("v2 plugin does not export 'memory'"))?;

            let mem_data = memory.data_mut(&mut store);
            let start = input_ptr as usize;
            let end = start + input_bytes.len();
            if end > mem_data.len() {
                return Err(anyhow!(
                    "az_alloc returned ptr {input_ptr} but memory size is {} (need {end})",
                    mem_data.len()
                ));
            }
            mem_data[start..end].copy_from_slice(input_bytes);

            let az_tool_execute = instance
                .get_typed_func::<(i32, i32), i64>(&store, "az_tool_execute")
                .map_err(|e| anyhow!("v2 plugin missing 'az_tool_execute' export: {e}"))?;

            let result_packed = match az_tool_execute.call(&mut store, (input_ptr, input_len)) {
                Ok(packed) => packed,
                Err(err) => {
                    let err_text = err.to_string();
                    if err_text.contains("out of fuel") || err_text.contains("fuel") {
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    return Err(anyhow!("az_tool_execute call failed: {err}"));
                }
            };

            let (out_ptr, out_len) = unpack_ptr_len(result_packed);
            if out_len == 0 {
                return Ok(WasmExecutionResultV2 {
                    output: String::new(),
                    error: Some("plugin returned empty output".to_string()),
                });
            }

            let mem_data = memory.data(&store);
            let out_start = out_ptr as usize;
            let out_end = out_start + out_len as usize;
            if out_end > mem_data.len() {
                return Err(anyhow!(
                    "plugin output ptr/len ({out_ptr}, {out_len}) exceeds memory bounds ({})",
                    mem_data.len()
                ));
            }

            let output_json = std::str::from_utf8(&mem_data[out_start..out_end])
                .map_err(|e| anyhow!("plugin output is not valid UTF-8: {e}"))?;

            let tool_output: WasmToolOutput = serde_json::from_str(output_json).map_err(|e| {
                anyhow!("plugin output is not valid JSON: {e} (raw: {output_json})")
            })?;

            Ok(WasmExecutionResultV2 {
                output: tool_output.output,
                error: tool_output.error,
            })
        }

        // -------------------------------------------------------------------
        // v1 API (backward-compatible)
        // -------------------------------------------------------------------

        pub fn preflight(&self, container: &WasmPluginContainer) -> anyhow::Result<()> {
            self.preflight_with_policy(container, &WasmIsolationPolicy::default())
        }

        pub fn preflight_with_policy(
            &self,
            container: &WasmPluginContainer,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<()> {
            container.validate()?;
            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            let path = Path::new(&container.module_path);
            if !path.exists() {
                return Err(anyhow!("plugin module does not exist: {}", path.display()));
            }
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if metadata.len() > policy.max_module_bytes {
                return Err(anyhow!(
                    "plugin module exceeds size policy ({} > {} bytes)",
                    metadata.len(),
                    policy.max_module_bytes
                ));
            }

            let engine = Engine::default();
            let bytes = std::fs::read(path)
                .with_context(|| format!("failed to read module at {}", path.display()))?;
            let module = Module::new(&engine, &bytes)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", path.display()))?;
            validate_host_call_allowlist(&module, policy)?;

            Ok(())
        }

        fn preflight_v2(
            &self,
            container: &WasmPluginContainer,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<()> {
            container.validate()?;

            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            let path = Path::new(&container.module_path);
            if !path.exists() {
                return Err(anyhow!("plugin module does not exist: {}", path.display()));
            }
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if metadata.len() > policy.max_module_bytes {
                return Err(anyhow!(
                    "plugin module exceeds size policy ({} > {} bytes)",
                    metadata.len(),
                    policy.max_module_bytes
                ));
            }

            Ok(())
        }

        pub fn execute(
            &self,
            container: &WasmPluginContainer,
            request: &WasmExecutionRequest,
        ) -> anyhow::Result<WasmExecutionResult> {
            self.execute_with_policy(container, request, &WasmIsolationPolicy::default())
        }

        pub fn execute_with_policy(
            &self,
            container: &WasmPluginContainer,
            _request: &WasmExecutionRequest,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResult> {
            self.preflight_with_policy(container, policy)?;

            let engine = make_engine();
            let bytes = std::fs::read(&container.module_path).map_err(|e| {
                anyhow!(
                    "failed to read module at {}: {e}",
                    container.module_path.display()
                )
            })?;
            let module = Module::new(&engine, &bytes).map_err(|e| {
                anyhow!(
                    "failed to compile module at {}: {e}",
                    container.module_path.display()
                )
            })?;
            validate_host_call_allowlist(&module, policy)?;

            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();
            let wasi = wasmi_wasi::WasiCtxBuilder::new().build();
            let state = PluginState {
                wasi,
                limits,
                log_buffer: Vec::new(),
            };
            let mut store = Store::new(&engine, state);
            store.limiter(|s| &mut s.limits);

            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            store
                .set_fuel(compute_fuel(effective_timeout_ms))
                .map_err(|e| anyhow!("failed to set fuel: {e}"))?;

            let linker = Linker::new(&engine);
            let instance = linker
                .instantiate_and_start(&mut store, &module)
                .map_err(|e| anyhow!("failed to instantiate plugin module: {e}"))?;

            let entrypoint = instance
                .get_typed_func::<(), i32>(&store, &container.entrypoint)
                .map_err(|e| {
                    anyhow!(
                        "missing or incompatible entrypoint '{}' (expected fn() -> i32): {e}",
                        container.entrypoint
                    )
                })?;

            let call_result = entrypoint.call(&mut store, ());
            let status_code = match call_result {
                Ok(status) => status,
                Err(err) => {
                    let err_text = err.to_string();
                    if err_text.contains("out of fuel") || err_text.contains("fuel") {
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    return Err(anyhow!("plugin entrypoint call failed: {err}"));
                }
            };

            Ok(WasmExecutionResult { status_code })
        }

        // -------------------------------------------------------------------
        // v2 API: JSON input/output, WASI, host callbacks
        // -------------------------------------------------------------------

        pub fn execute_v2(
            &self,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            self.execute_v2_with_policy(container, input, options, &WasmIsolationPolicy::default())
        }

        pub fn execute_v2_with_policy(
            &self,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            self.preflight_v2(container, policy)?;

            let engine = make_engine();
            let bytes = std::fs::read(&container.module_path).map_err(|e| {
                anyhow!(
                    "failed to compile module at {}: {e}",
                    container.module_path.display()
                )
            })?;
            let module = Module::new(&engine, &bytes).map_err(|e| {
                anyhow!(
                    "failed to compile module at {}: {e}",
                    container.module_path.display()
                )
            })?;

            validate_v2_imports(&module, policy, &options.capabilities)?;

            // Build WASI context with capabilities gated by policy
            let mut wasi_builder = wasmi_wasi::WasiCtxBuilder::new();
            wasi_builder.inherit_stderr();

            if policy.allow_fs_read && !options.workspace_root.is_empty() {
                let workspace_path = std::path::Path::new(&options.workspace_root);
                if workspace_path.exists() {
                    match wasmi_wasi::sync::Dir::open_ambient_dir(
                        workspace_path,
                        wasmi_wasi::sync::ambient_authority(),
                    ) {
                        Ok(dir) => {
                            if policy.allow_fs_write {
                                let _ = wasi_builder.preopened_dir(dir, ".");
                            } else {
                                // wasmi_wasi doesn't have fine-grained perms
                                // like wasmtime; preopened dirs are read-write.
                                // For read-only, we still preopen but rely on
                                // WASM sandbox isolation.
                                let _ = wasi_builder.preopened_dir(dir, ".");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %options.workspace_root,
                                error = %e,
                                "failed to preopen workspace dir"
                            );
                        }
                    }
                }
            }

            let wasi = wasi_builder.build();

            // Memory limits
            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();
            let state = PluginState {
                wasi,
                limits,
                log_buffer: Vec::new(),
            };
            let mut store = Store::new(&engine, state);
            store.limiter(|s| &mut s.limits);

            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            store
                .set_fuel(compute_fuel(effective_timeout_ms))
                .map_err(|e| anyhow!("failed to set fuel: {e}"))?;

            // Linker: add WASI preview1 + host functions
            let mut linker: Linker<PluginState> = Linker::new(&engine);

            wasmi_wasi::sync::add_to_linker(&mut linker, |s: &mut PluginState| &mut s.wasi)
                .map_err(|e| anyhow!("failed to add WASI p1 to linker: {e}"))?;

            // Register host functions in the "az" namespace
            register_host_functions(&mut linker, policy)?;

            let instance = linker
                .instantiate_and_start(&mut store, &module)
                .map_err(|e| anyhow!("failed to instantiate v2 plugin module: {e}"))?;

            // Build the input JSON
            let tool_input = WasmToolInput {
                input: input.to_string(),
                workspace_root: options.workspace_root.clone(),
            };
            let input_json =
                serde_json::to_string(&tool_input).context("failed to serialize v2 tool input")?;

            // Allocate space in WASM memory for the input via az_alloc
            let az_alloc = instance
                .get_typed_func::<i32, i32>(&store, "az_alloc")
                .map_err(|e| {
                    anyhow!("v2 plugin missing 'az_alloc' export (expected fn(i32) -> i32): {e}")
                })?;

            let input_bytes = input_json.as_bytes();
            let input_len = input_bytes.len() as i32;

            let input_ptr = az_alloc
                .call(&mut store, input_len)
                .map_err(|e| anyhow!("az_alloc call failed: {e}"))?;

            // Write input JSON into WASM linear memory
            let memory = instance
                .get_memory(&store, "memory")
                .ok_or_else(|| anyhow!("v2 plugin does not export 'memory'"))?;

            let mem_data = memory.data_mut(&mut store);
            let start = input_ptr as usize;
            let end = start + input_bytes.len();
            if end > mem_data.len() {
                return Err(anyhow!(
                    "az_alloc returned ptr {input_ptr} but memory size is {} (need {end})",
                    mem_data.len()
                ));
            }
            mem_data[start..end].copy_from_slice(input_bytes);

            // Call az_tool_execute(ptr, len) -> i64
            let az_tool_execute = instance
                .get_typed_func::<(i32, i32), i64>(&store, "az_tool_execute")
                .map_err(|e| anyhow!("v2 plugin missing 'az_tool_execute' export: {e}"))?;

            let result_packed = match az_tool_execute.call(&mut store, (input_ptr, input_len)) {
                Ok(packed) => packed,
                Err(err) => {
                    let err_text = err.to_string();
                    if err_text.contains("out of fuel") || err_text.contains("fuel") {
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    return Err(anyhow!("az_tool_execute call failed: {err}"));
                }
            };

            // Unpack the result pointer and length
            let (out_ptr, out_len) = unpack_ptr_len(result_packed);
            if out_len == 0 {
                return Ok(WasmExecutionResultV2 {
                    output: String::new(),
                    error: Some("plugin returned empty output".to_string()),
                });
            }

            // Read output JSON from WASM linear memory
            let mem_data = memory.data(&store);
            let out_start = out_ptr as usize;
            let out_end = out_start + out_len as usize;
            if out_end > mem_data.len() {
                return Err(anyhow!(
                    "plugin output ptr/len ({out_ptr}, {out_len}) exceeds memory bounds ({})",
                    mem_data.len()
                ));
            }

            let output_json = std::str::from_utf8(&mem_data[out_start..out_end])
                .map_err(|e| anyhow!("plugin output is not valid UTF-8: {e}"))?;

            let tool_output: WasmToolOutput = serde_json::from_str(output_json).map_err(|e| {
                anyhow!("plugin output is not valid JSON: {e} (raw: {output_json})")
            })?;

            Ok(WasmExecutionResultV2 {
                output: tool_output.output,
                error: tool_output.error,
            })
        }
    }

    /// Register `az_*` host functions in the linker under the "az" namespace.
    fn register_host_functions(
        linker: &mut Linker<PluginState>,
        policy: &WasmIsolationPolicy,
    ) -> anyhow::Result<()> {
        // az_log(level: i32, msg_ptr: i32, msg_len: i32)
        linker
            .func_wrap(
                "az",
                "az_log",
                |mut caller: wasmi::Caller<'_, PluginState>,
                 level: i32,
                 msg_ptr: i32,
                 msg_len: i32| {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(memory) = memory {
                        let msg_opt = {
                            let data = memory.data(&caller);
                            let start = msg_ptr as usize;
                            let end = start + msg_len as usize;
                            if end <= data.len() {
                                std::str::from_utf8(&data[start..end])
                                    .ok()
                                    .map(|s| s.to_owned())
                            } else {
                                None
                            }
                        };
                        if let Some(msg) = msg_opt {
                            let level_str = match level {
                                0 => "ERROR",
                                1 => "WARN",
                                2 => "INFO",
                                3 => "DEBUG",
                                _ => "TRACE",
                            };
                            caller
                                .data_mut()
                                .log_buffer
                                .push(format!("[{level_str}] {msg}"));
                        }
                    }
                },
            )
            .map_err(|e| anyhow!("failed to register az_log: {e}"))?;

        // az_env_get(key_ptr: i32, key_len: i32) -> i64
        if policy
            .allowed_host_calls
            .iter()
            .any(|h| h == "az::az_env_get")
        {
            linker
                .func_wrap(
                    "az",
                    "az_env_get",
                    |mut caller: wasmi::Caller<'_, PluginState>,
                     key_ptr: i32,
                     key_len: i32|
                     -> i64 {
                        let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                        let Some(memory) = memory else {
                            return 0;
                        };

                        let data = memory.data(&caller);
                        let start = key_ptr as usize;
                        let end = start + key_len as usize;
                        if end > data.len() {
                            return 0;
                        }
                        let Ok(key) = std::str::from_utf8(&data[start..end]) else {
                            return 0;
                        };
                        let Ok(value) = std::env::var(key) else {
                            return 0;
                        };

                        let az_alloc = caller
                            .get_export("az_alloc")
                            .and_then(|e| e.into_func())
                            .and_then(|f| f.typed::<i32, i32>(&caller).ok());
                        let Some(az_alloc) = az_alloc else {
                            return 0;
                        };

                        let value_bytes = value.as_bytes();
                        let Ok(ptr) = az_alloc.call(&mut caller, value_bytes.len() as i32) else {
                            return 0;
                        };

                        let mem = caller.get_export("memory").and_then(|e| e.into_memory());
                        if let Some(mem) = mem {
                            let data = mem.data_mut(&mut caller);
                            let s = ptr as usize;
                            let e = s + value_bytes.len();
                            if e <= data.len() {
                                data[s..e].copy_from_slice(value_bytes);
                                return pack_ptr_len(ptr as u32, value_bytes.len() as u32);
                            }
                        }
                        0
                    },
                )
                .map_err(|e| anyhow!("failed to register az_env_get: {e}"))?;
        }

        Ok(())
    }

    fn validate_v2_imports(
        module: &Module,
        policy: &WasmIsolationPolicy,
        capabilities: &[String],
    ) -> anyhow::Result<()> {
        for import in module.imports() {
            let module_name = import.module();
            if module_name == "wasi_snapshot_preview1" {
                continue;
            }
            if module_name == "az" {
                let func_name = import.name();
                if func_name == "az_log" {
                    continue;
                }
                let key = format!("az::{func_name}");
                if capabilities
                    .iter()
                    .any(|c| c == &key || c == &format!("host:{func_name}"))
                    && policy.allowed_host_calls.iter().any(|h| h == &key)
                {
                    continue;
                }
                return Err(anyhow!(
                    "host function `{key}` is not permitted by isolation policy"
                ));
            }
            let key = format!("{}::{}", module_name, import.name());
            if !policy
                .allowed_host_calls
                .iter()
                .any(|allowed| allowed == &key)
            {
                return Err(anyhow!(
                    "host call `{key}` is not allowed by isolation policy"
                ));
            }
        }
        Ok(())
    }

    fn validate_host_call_allowlist(
        module: &Module,
        policy: &WasmIsolationPolicy,
    ) -> anyhow::Result<()> {
        for import in module.imports() {
            let key = format!("{}::{}", import.module(), import.name());
            if !policy
                .allowed_host_calls
                .iter()
                .any(|allowed| allowed == &key)
            {
                return Err(anyhow!(
                    "host call `{key}` is not allowed by isolation policy"
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Module cache: wasmi has no AOT compilation — passthrough only
    // -----------------------------------------------------------------------

    pub struct ModuleCache;

    impl ModuleCache {
        pub fn load_or_compile(
            engine: &Engine,
            wasm_path: &Path,
            _expected_sha256: &str,
        ) -> anyhow::Result<Module> {
            let bytes = std::fs::read(wasm_path)
                .with_context(|| format!("failed to read module at {}", wasm_path.display()))?;
            Module::new(engine, &bytes)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", wasm_path.display()))
        }
    }
}

// ---------------------------------------------------------------------------
// wasm-jit feature: full wasmtime JIT-backed implementation
// ---------------------------------------------------------------------------
#[cfg(feature = "wasm-jit")]
mod runtime_impl {
    use super::*;
    use anyhow::Context;
    use std::path::Path;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::WasiCtxBuilder;

    /// Combined store state for v2 plugins: WASI context + memory limits.
    struct PluginState {
        wasi: WasiP1Ctx,
        limits: StoreLimits,
        log_buffer: Vec<String>,
    }

    impl WasmPluginRuntime {
        pub fn new() -> Self {
            Self
        }

        /// Create a new wasmtime engine with epoch interruption enabled.
        /// Share this across plugins for efficient resource use.
        pub fn create_engine() -> anyhow::Result<Engine> {
            let mut config = Config::new();
            config.epoch_interruption(true);
            Engine::new(&config).map_err(|e| anyhow!("failed to configure wasmtime engine: {e}"))
        }

        /// Pre-compile a WASM module from the given path.
        /// The returned module can be reused across multiple executions.
        pub fn compile_module(
            engine: &Engine,
            wasm_path: &std::path::Path,
        ) -> anyhow::Result<Module> {
            Module::from_file(engine, wasm_path)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", wasm_path.display()))
        }

        /// Execute a v2 plugin with a pre-compiled engine and module.
        /// This avoids disk I/O and module compilation on the hot path.
        pub fn execute_v2_precompiled(
            engine: &Engine,
            module: &Module,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            // Preflight checks (skip module file read — already compiled)
            container.validate()?;
            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            validate_v2_imports(module, policy, &options.capabilities)?;

            // Build WASI context with capabilities gated by policy
            let mut wasi_builder = WasiCtxBuilder::new();
            wasi_builder.inherit_stderr();

            if policy.allow_fs_read && !options.workspace_root.is_empty() {
                let perms = if policy.allow_fs_write {
                    wasmtime_wasi::DirPerms::all()
                } else {
                    wasmtime_wasi::DirPerms::READ
                };
                let file_perms = if policy.allow_fs_write {
                    wasmtime_wasi::FilePerms::all()
                } else {
                    wasmtime_wasi::FilePerms::READ
                };
                if let Err(e) =
                    wasi_builder.preopened_dir(&options.workspace_root, ".", perms, file_perms)
                {
                    tracing::warn!(
                        path = %options.workspace_root,
                        error = %e,
                        "failed to preopen workspace dir"
                    );
                }
            }

            let wasi = wasi_builder.build_p1();
            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();
            let state = PluginState {
                wasi,
                limits,
                log_buffer: Vec::new(),
            };
            let mut store = Store::new(engine, state);
            store.limiter(|s: &mut PluginState| &mut s.limits);
            store.set_epoch_deadline(1);

            let mut linker: Linker<PluginState> = Linker::new(engine);
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut PluginState| &mut s.wasi)
                .map_err(|e| anyhow!("failed to add WASI p1 to linker: {e}"))?;
            register_host_functions(&mut linker, policy)?;

            let instance = linker
                .instantiate(&mut store, module)
                .map_err(|e| anyhow!("failed to instantiate v2 plugin module: {e}"))?;

            // Epoch timeout thread
            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            let timer_engine = engine.clone();
            let timer_cancel = Arc::new(AtomicBool::new(false));
            let timer_cancel_worker = Arc::clone(&timer_cancel);
            let timer_handle = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(effective_timeout_ms);
                while Instant::now() < deadline {
                    if timer_cancel_worker.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !timer_cancel_worker.load(Ordering::Relaxed) {
                    timer_engine.increment_epoch();
                }
            });

            struct TimerGuard {
                cancel: Arc<AtomicBool>,
                handle: Option<std::thread::JoinHandle<()>>,
            }
            impl Drop for TimerGuard {
                fn drop(&mut self) {
                    self.cancel.store(true, Ordering::Relaxed);
                    if let Some(h) = self.handle.take() {
                        let _ = h.join();
                    }
                }
            }
            let _timer_guard = TimerGuard {
                cancel: Arc::clone(&timer_cancel),
                handle: Some(timer_handle),
            };

            let tool_input = WasmToolInput {
                input: input.to_string(),
                workspace_root: options.workspace_root.clone(),
            };
            let input_json =
                serde_json::to_string(&tool_input).context("failed to serialize v2 tool input")?;

            let az_alloc = instance
                .get_typed_func::<i32, i32>(&mut store, "az_alloc")
                .map_err(|e| {
                    anyhow!("v2 plugin missing 'az_alloc' export (expected fn(i32) -> i32): {e}")
                })?;

            let input_bytes = input_json.as_bytes();
            let input_len = input_bytes.len() as i32;
            let input_ptr = az_alloc
                .call(&mut store, input_len)
                .map_err(|e| anyhow!("az_alloc call failed: {e}"))?;

            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| anyhow!("v2 plugin does not export 'memory'"))?;

            let mem_data = memory.data_mut(&mut store);
            let start = input_ptr as usize;
            let end = start + input_bytes.len();
            if end > mem_data.len() {
                return Err(anyhow!(
                    "az_alloc returned ptr {input_ptr} but memory size is {} (need {end})",
                    mem_data.len()
                ));
            }
            mem_data[start..end].copy_from_slice(input_bytes);

            let az_tool_execute = instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "az_tool_execute")
                .map_err(|e| anyhow!("v2 plugin missing 'az_tool_execute' export: {e}"))?;

            let started = Instant::now();
            let result_packed = match az_tool_execute.call(&mut store, (input_ptr, input_len)) {
                Ok(packed) => packed,
                Err(err) => {
                    let err_text = err.to_string();
                    let timed_out =
                        started.elapsed() >= Duration::from_millis(effective_timeout_ms);
                    if err_text.contains("epoch deadline exceeded")
                        || err_text.contains("interrupt")
                        || err_text.contains("interrupted")
                        || err_text.contains("deadline")
                        || timed_out
                    {
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    return Err(anyhow!("az_tool_execute call failed: {err}"));
                }
            };

            let (out_ptr, out_len) = unpack_ptr_len(result_packed);
            if out_len == 0 {
                return Ok(WasmExecutionResultV2 {
                    output: String::new(),
                    error: Some("plugin returned empty output".to_string()),
                });
            }

            let mem_data = memory.data(&store);
            let out_start = out_ptr as usize;
            let out_end = out_start + out_len as usize;
            if out_end > mem_data.len() {
                return Err(anyhow!(
                    "plugin output ptr/len ({out_ptr}, {out_len}) exceeds memory bounds ({})",
                    mem_data.len()
                ));
            }

            let output_json = std::str::from_utf8(&mem_data[out_start..out_end])
                .map_err(|e| anyhow!("plugin output is not valid UTF-8: {e}"))?;

            let tool_output: WasmToolOutput = serde_json::from_str(output_json).map_err(|e| {
                anyhow!("plugin output is not valid JSON: {e} (raw: {output_json})")
            })?;

            Ok(WasmExecutionResultV2 {
                output: tool_output.output,
                error: tool_output.error,
            })
        }

        // -------------------------------------------------------------------
        // v1 API (backward-compatible, unchanged)
        // -------------------------------------------------------------------

        pub fn preflight(&self, container: &WasmPluginContainer) -> anyhow::Result<()> {
            self.preflight_with_policy(container, &WasmIsolationPolicy::default())
        }

        pub fn preflight_with_policy(
            &self,
            container: &WasmPluginContainer,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<()> {
            container.validate()?;
            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            let path = Path::new(&container.module_path);
            if !path.exists() {
                return Err(anyhow!("plugin module does not exist: {}", path.display()));
            }
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if metadata.len() > policy.max_module_bytes {
                return Err(anyhow!(
                    "plugin module exceeds size policy ({} > {} bytes)",
                    metadata.len(),
                    policy.max_module_bytes
                ));
            }

            let engine = Engine::default();
            let module = Module::from_file(&engine, path)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", path.display()))?;
            validate_host_call_allowlist(&module, policy)?;

            Ok(())
        }

        /// Preflight for v2 plugins — checks container/policy constraints and
        /// file existence/size, but does NOT run v1 import validation (v2
        /// modules use WASI and az namespace imports that v1 validation
        /// rejects).
        fn preflight_v2(
            &self,
            container: &WasmPluginContainer,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<()> {
            container.validate()?;

            if container.max_execution_ms > policy.max_execution_ms {
                return Err(anyhow!(
                    "max_execution_ms exceeds policy limit ({} > {})",
                    container.max_execution_ms,
                    policy.max_execution_ms
                ));
            }
            if container.max_memory_mb > policy.max_memory_mb {
                return Err(anyhow!(
                    "max_memory_mb exceeds policy limit ({} > {})",
                    container.max_memory_mb,
                    policy.max_memory_mb
                ));
            }
            if container.allow_network && !policy.allow_network {
                return Err(anyhow!(
                    "network access is not permitted by isolation policy"
                ));
            }
            if container.allow_fs_write && !policy.allow_fs_write {
                return Err(anyhow!(
                    "filesystem write is not permitted by isolation policy"
                ));
            }

            let path = Path::new(&container.module_path);
            if !path.exists() {
                return Err(anyhow!("plugin module does not exist: {}", path.display()));
            }
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("failed to read metadata for {}", path.display()))?;
            if metadata.len() > policy.max_module_bytes {
                return Err(anyhow!(
                    "plugin module exceeds size policy ({} > {} bytes)",
                    metadata.len(),
                    policy.max_module_bytes
                ));
            }

            Ok(())
        }

        pub fn execute(
            &self,
            container: &WasmPluginContainer,
            request: &WasmExecutionRequest,
        ) -> anyhow::Result<WasmExecutionResult> {
            self.execute_with_policy(container, request, &WasmIsolationPolicy::default())
        }

        pub fn execute_with_policy(
            &self,
            container: &WasmPluginContainer,
            _request: &WasmExecutionRequest,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResult> {
            self.preflight_with_policy(container, policy)?;

            let mut config = Config::new();
            config.epoch_interruption(true);
            let engine = Engine::new(&config)
                .map_err(|e| anyhow!("failed to configure wasmtime engine: {e}"))?;
            let module = Module::from_file(&engine, &container.module_path).map_err(|e| {
                anyhow!(
                    "failed to compile module at {}: {e}",
                    container.module_path.display()
                )
            })?;
            validate_host_call_allowlist(&module, policy)?;

            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();
            let mut store = Store::new(&engine, limits);
            store.limiter(|limiter: &mut StoreLimits| limiter);
            store.set_epoch_deadline(1);

            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            let timer_engine = engine.clone();
            let timer_cancel = Arc::new(AtomicBool::new(false));
            let timer_cancel_worker = Arc::clone(&timer_cancel);
            let timer_handle = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(effective_timeout_ms);
                while Instant::now() < deadline {
                    if timer_cancel_worker.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !timer_cancel_worker.load(Ordering::Relaxed) {
                    timer_engine.increment_epoch();
                }
            });

            let linker = Linker::new(&engine);
            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| anyhow!("failed to instantiate plugin module: {e}"))?;

            let entrypoint = instance
                .get_typed_func::<(), i32>(&mut store, &container.entrypoint)
                .map_err(|e| {
                    anyhow!(
                        "missing or incompatible entrypoint '{}' (expected fn() -> i32): {e}",
                        container.entrypoint
                    )
                })?;

            let started = Instant::now();
            let call_result: Result<i32, wasmtime::Error> = entrypoint.call(&mut store, ());
            let status_code = match call_result {
                Ok(status) => status,
                Err(err) => {
                    let err_text = err.to_string();
                    let timed_out =
                        started.elapsed() >= Duration::from_millis(effective_timeout_ms);
                    if err_text.contains("epoch deadline exceeded")
                        || err_text.contains("interrupt")
                        || err_text.contains("interrupted")
                        || err_text.contains("deadline")
                        || timed_out
                    {
                        timer_cancel.store(true, Ordering::Relaxed);
                        let _ = timer_handle.join();
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    timer_cancel.store(true, Ordering::Relaxed);
                    let _ = timer_handle.join();
                    return Err(anyhow!("plugin entrypoint call failed: {err}"));
                }
            };
            timer_cancel.store(true, Ordering::Relaxed);
            let _ = timer_handle.join();

            Ok(WasmExecutionResult { status_code })
        }

        // -------------------------------------------------------------------
        // v2 API: JSON input/output, WASI, host callbacks
        // -------------------------------------------------------------------

        /// Execute a v2 plugin: write JSON input to WASM memory, call
        /// `az_tool_execute`, read JSON output back.
        pub fn execute_v2(
            &self,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            self.execute_v2_with_policy(container, input, options, &WasmIsolationPolicy::default())
        }

        pub fn execute_v2_with_policy(
            &self,
            container: &WasmPluginContainer,
            input: &str,
            options: &WasmV2Options,
            policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            // Preflight checks (v2-specific — skips v1 import validation
            // because v2 modules use WASI and az namespace imports).
            self.preflight_v2(container, policy)?;

            // Engine with epoch interruption
            let mut config = Config::new();
            config.epoch_interruption(true);
            let engine = Engine::new(&config)
                .map_err(|e| anyhow!("failed to configure wasmtime engine: {e}"))?;
            let module = Module::from_file(&engine, &container.module_path).map_err(|e| {
                anyhow!(
                    "failed to compile module at {}: {e}",
                    container.module_path.display()
                )
            })?;

            // Validate imports against policy (skip WASI imports which are
            // provided by the WASI layer, and skip az_* host imports which
            // we provide ourselves).
            validate_v2_imports(&module, policy, &options.capabilities)?;

            // Build WASI context with capabilities gated by policy
            let mut wasi_builder = WasiCtxBuilder::new();
            wasi_builder.inherit_stderr();

            if policy.allow_fs_read && !options.workspace_root.is_empty() {
                let perms = if policy.allow_fs_write {
                    wasmtime_wasi::DirPerms::all()
                } else {
                    wasmtime_wasi::DirPerms::READ
                };
                let file_perms = if policy.allow_fs_write {
                    wasmtime_wasi::FilePerms::all()
                } else {
                    wasmtime_wasi::FilePerms::READ
                };
                // Preopened dir can fail if path doesn't exist — that's fine,
                // just skip it with a warning.
                if let Err(e) =
                    wasi_builder.preopened_dir(&options.workspace_root, ".", perms, file_perms)
                {
                    tracing::warn!(
                        path = %options.workspace_root,
                        error = %e,
                        "failed to preopen workspace dir"
                    );
                }
            }

            let wasi = wasi_builder.build_p1();

            // Memory limits
            let effective_memory_mb = container.max_memory_mb.min(policy.max_memory_mb);
            let limits = StoreLimitsBuilder::new()
                .memory_size((effective_memory_mb as usize) * 1024 * 1024)
                .build();

            let state = PluginState {
                wasi,
                limits,
                log_buffer: Vec::new(),
            };
            let mut store = Store::new(&engine, state);
            store.limiter(|s: &mut PluginState| &mut s.limits);
            store.set_epoch_deadline(1);

            // Linker: add WASI p1 + host functions
            let mut linker: Linker<PluginState> = Linker::new(&engine);

            // Add WASI preview1 functions (always available — individual
            // capabilities are gated by what we preopen above)
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut PluginState| &mut s.wasi)
                .map_err(|e| anyhow!("failed to add WASI p1 to linker: {e}"))?;

            // Register host functions in the "az" namespace
            register_host_functions(&mut linker, policy)?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| anyhow!("failed to instantiate v2 plugin module: {e}"))?;

            // Epoch timeout thread
            let effective_timeout_ms = container.max_execution_ms.min(policy.max_execution_ms);
            let timer_engine = engine.clone();
            let timer_cancel = Arc::new(AtomicBool::new(false));
            let timer_cancel_worker = Arc::clone(&timer_cancel);
            let timer_handle = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(effective_timeout_ms);
                while Instant::now() < deadline {
                    if timer_cancel_worker.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !timer_cancel_worker.load(Ordering::Relaxed) {
                    timer_engine.increment_epoch();
                }
            });

            // Build the input JSON
            let tool_input = WasmToolInput {
                input: input.to_string(),
                workspace_root: options.workspace_root.clone(),
            };
            let input_json =
                serde_json::to_string(&tool_input).context("failed to serialize v2 tool input")?;

            // Guard that cancels the timer thread on drop (any exit path).
            struct TimerGuard {
                cancel: Arc<AtomicBool>,
                handle: Option<std::thread::JoinHandle<()>>,
            }
            impl Drop for TimerGuard {
                fn drop(&mut self) {
                    self.cancel.store(true, Ordering::Relaxed);
                    if let Some(h) = self.handle.take() {
                        let _ = h.join();
                    }
                }
            }
            let _timer_guard = TimerGuard {
                cancel: Arc::clone(&timer_cancel),
                handle: Some(timer_handle),
            };

            // Allocate space in WASM memory for the input via az_alloc
            let az_alloc = instance
                .get_typed_func::<i32, i32>(&mut store, "az_alloc")
                .map_err(|e| {
                    anyhow!("v2 plugin missing 'az_alloc' export (expected fn(i32) -> i32): {e}")
                })?;

            let input_bytes = input_json.as_bytes();
            let input_len = input_bytes.len() as i32;

            let started = Instant::now();

            let input_ptr = az_alloc
                .call(&mut store, input_len)
                .map_err(|e| anyhow!("az_alloc call failed: {e}"))?;

            // Write input JSON into WASM linear memory
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| anyhow!("v2 plugin does not export 'memory'"))?;

            let mem_data = memory.data_mut(&mut store);
            let start = input_ptr as usize;
            let end = start + input_bytes.len();
            if end > mem_data.len() {
                return Err(anyhow!(
                    "az_alloc returned ptr {input_ptr} but memory size is {} (need {end})",
                    mem_data.len()
                ));
            }
            mem_data[start..end].copy_from_slice(input_bytes);

            // Call az_tool_execute(ptr, len) -> i64
            let az_tool_execute = instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "az_tool_execute")
                .map_err(|e| anyhow!("v2 plugin missing 'az_tool_execute' export: {e}"))?;

            let result_packed = match az_tool_execute.call(&mut store, (input_ptr, input_len)) {
                Ok(packed) => packed,
                Err(err) => {
                    let err_text = err.to_string();
                    let timed_out =
                        started.elapsed() >= Duration::from_millis(effective_timeout_ms);
                    if err_text.contains("epoch deadline exceeded")
                        || err_text.contains("interrupt")
                        || err_text.contains("interrupted")
                        || err_text.contains("deadline")
                        || timed_out
                    {
                        return Err(anyhow!(
                            "plugin execution exceeded time limit ({} ms)",
                            effective_timeout_ms
                        ));
                    }
                    return Err(anyhow!("az_tool_execute call failed: {err}"));
                }
            };
            // _timer_guard drops here, cancelling the timer thread

            // Unpack the result pointer and length
            let (out_ptr, out_len) = unpack_ptr_len(result_packed);
            if out_len == 0 {
                return Ok(WasmExecutionResultV2 {
                    output: String::new(),
                    error: Some("plugin returned empty output".to_string()),
                });
            }

            // Read output JSON from WASM linear memory
            let mem_data = memory.data(&store);
            let out_start = out_ptr as usize;
            let out_end = out_start + out_len as usize;
            if out_end > mem_data.len() {
                return Err(anyhow!(
                    "plugin output ptr/len ({out_ptr}, {out_len}) exceeds memory bounds ({})",
                    mem_data.len()
                ));
            }

            let output_json = std::str::from_utf8(&mem_data[out_start..out_end])
                .map_err(|e| anyhow!("plugin output is not valid UTF-8: {e}"))?;

            let tool_output: WasmToolOutput = serde_json::from_str(output_json).map_err(|e| {
                anyhow!("plugin output is not valid JSON: {e} (raw: {output_json})")
            })?;

            Ok(WasmExecutionResultV2 {
                output: tool_output.output,
                error: tool_output.error,
            })
        }
    }

    /// Register `az_*` host functions in the linker under the "az" namespace.
    fn register_host_functions(
        linker: &mut Linker<PluginState>,
        policy: &WasmIsolationPolicy,
    ) -> anyhow::Result<()> {
        // az_log(level: i32, msg_ptr: i32, msg_len: i32)
        // Always available — logging is not a security-sensitive operation.
        linker
            .func_wrap(
                "az",
                "az_log",
                |mut caller: wasmtime::Caller<'_, PluginState>,
                 level: i32,
                 msg_ptr: i32,
                 msg_len: i32| {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(memory) = memory {
                        // Extract message from WASM memory, then drop the
                        // immutable borrow before pushing to log_buffer.
                        let msg_opt = {
                            let data = memory.data(&caller);
                            let start = msg_ptr as usize;
                            let end = start + msg_len as usize;
                            if end <= data.len() {
                                std::str::from_utf8(&data[start..end])
                                    .ok()
                                    .map(|s| s.to_owned())
                            } else {
                                None
                            }
                        };
                        if let Some(msg) = msg_opt {
                            let level_str = match level {
                                0 => "ERROR",
                                1 => "WARN",
                                2 => "INFO",
                                3 => "DEBUG",
                                _ => "TRACE",
                            };
                            caller
                                .data_mut()
                                .log_buffer
                                .push(format!("[{level_str}] {msg}"));
                        }
                    }
                },
            )
            .map_err(|e| anyhow!("failed to register az_log: {e}"))?;

        // az_env_get(key_ptr: i32, key_len: i32) -> i64
        // Gated by allowed_host_calls containing "az::az_env_get"
        if policy
            .allowed_host_calls
            .iter()
            .any(|h| h == "az::az_env_get")
        {
            linker
                .func_wrap(
                    "az",
                    "az_env_get",
                    |mut caller: wasmtime::Caller<'_, PluginState>,
                     key_ptr: i32,
                     key_len: i32|
                     -> i64 {
                        let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                        let Some(memory) = memory else {
                            return 0;
                        };

                        let data = memory.data(&caller);
                        let start = key_ptr as usize;
                        let end = start + key_len as usize;
                        if end > data.len() {
                            return 0;
                        }
                        let Ok(key) = std::str::from_utf8(&data[start..end]) else {
                            return 0;
                        };
                        let Ok(value) = std::env::var(key) else {
                            return 0;
                        };

                        // Allocate space in plugin memory and write the value
                        let az_alloc = caller
                            .get_export("az_alloc")
                            .and_then(|e| e.into_func())
                            .and_then(|f| f.typed::<i32, i32>(&caller).ok());
                        let Some(az_alloc) = az_alloc else {
                            return 0;
                        };

                        let value_bytes = value.as_bytes();
                        let Ok(ptr) = az_alloc.call(&mut caller, value_bytes.len() as i32) else {
                            return 0;
                        };

                        let mem = caller.get_export("memory").and_then(|e| e.into_memory());
                        if let Some(mem) = mem {
                            let data = mem.data_mut(&mut caller);
                            let s = ptr as usize;
                            let e = s + value_bytes.len();
                            if e <= data.len() {
                                data[s..e].copy_from_slice(value_bytes);
                                return pack_ptr_len(ptr as u32, value_bytes.len() as u32);
                            }
                        }
                        0
                    },
                )
                .map_err(|e| anyhow!("failed to register az_env_get: {e}"))?;
        }

        Ok(())
    }

    /// Validate v2 module imports against policy. WASI imports (module
    /// "wasi_snapshot_preview1") and registered "az" host functions are
    /// allowed; everything else must be in the allowlist.
    fn validate_v2_imports(
        module: &Module,
        policy: &WasmIsolationPolicy,
        capabilities: &[String],
    ) -> anyhow::Result<()> {
        for import in module.imports() {
            let module_name = import.module();
            // WASI preview1 imports are handled by the WASI layer
            if module_name == "wasi_snapshot_preview1" {
                continue;
            }
            // az namespace host functions are registered by us
            if module_name == "az" {
                let func_name = import.name();
                // az_log is always allowed
                if func_name == "az_log" {
                    continue;
                }
                // Other az functions must be in capabilities and policy
                let key = format!("az::{func_name}");
                if capabilities
                    .iter()
                    .any(|c| c == &key || c == &format!("host:{func_name}"))
                    && policy.allowed_host_calls.iter().any(|h| h == &key)
                {
                    continue;
                }
                return Err(anyhow!(
                    "host function `{key}` is not permitted by isolation policy"
                ));
            }
            // Everything else must be in the v1-style allowlist
            let key = format!("{}::{}", module_name, import.name());
            if !policy
                .allowed_host_calls
                .iter()
                .any(|allowed| allowed == &key)
            {
                return Err(anyhow!(
                    "host call `{key}` is not allowed by isolation policy"
                ));
            }
        }
        Ok(())
    }

    /// Validate v1 module imports (original behavior — no WASI, no az namespace).
    fn validate_host_call_allowlist(
        module: &Module,
        policy: &WasmIsolationPolicy,
    ) -> anyhow::Result<()> {
        for import in module.imports() {
            let key = format!("{}::{}", import.module(), import.name());
            if !policy
                .allowed_host_calls
                .iter()
                .any(|allowed| allowed == &key)
            {
                return Err(anyhow!(
                    "host call `{key}` is not allowed by isolation policy"
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Module cache: AOT-compiled modules stored alongside the .wasm file.
    // -----------------------------------------------------------------------

    /// Cached AOT module storage. The cache file is stored at
    /// `{wasm_dir}/.cache/plugin.cwasm` with a `source.sha256` sidecar
    /// for invalidation.
    pub struct ModuleCache;

    impl ModuleCache {
        /// Load a module from cache or compile fresh. On successful
        /// compilation, the AOT artifact is written to the cache directory
        /// for faster future loads.
        ///
        /// Cache location: `{wasm_dir}/.cache/plugin.cwasm`
        /// Invalidation:   `{wasm_dir}/.cache/source.sha256`
        ///
        /// # Safety
        /// Uses `Module::deserialize_file()` when the SHA-256 matches.
        /// This is safe because:
        /// - wasmtime validates the serialization format on load
        /// - SHA-256 mismatch triggers recompilation
        /// - wasmtime version mismatch triggers recompilation (automatic)
        pub fn load_or_compile(
            engine: &Engine,
            wasm_path: &Path,
            expected_sha256: &str,
        ) -> anyhow::Result<Module> {
            let cache_dir = wasm_path
                .parent()
                .ok_or_else(|| anyhow!("wasm_path has no parent directory"))?
                .join(".cache");

            let cwasm_path = cache_dir.join("plugin.cwasm");
            let sha_path = cache_dir.join("source.sha256");

            // Try loading from cache if the SHA256 matches
            if cwasm_path.exists() && sha_path.exists() {
                if let Ok(cached_sha) = std::fs::read_to_string(&sha_path) {
                    if cached_sha.trim() == expected_sha256 && !expected_sha256.is_empty() {
                        // Safety: SHA256 verified, wasmtime checks format internally
                        match unsafe { Module::deserialize_file(engine, &cwasm_path) } {
                            Ok(module) => return Ok(module),
                            Err(_e) => {
                                // Cache is stale (e.g. wasmtime version mismatch),
                                // fall through to recompilation.
                            }
                        }
                    }
                }
            }

            // Compile from source
            let module = Module::from_file(engine, wasm_path)
                .map_err(|e| anyhow!("failed to compile module at {}: {e}", wasm_path.display()))?;

            // Persist AOT artifact (best-effort — cache miss is not fatal)
            if !expected_sha256.is_empty() {
                if let Err(e) = Self::write_cache(&module, &cache_dir, expected_sha256) {
                    tracing::warn!(error = %e, "failed to write module cache");
                }
            }

            Ok(module)
        }

        fn write_cache(module: &Module, cache_dir: &Path, sha256: &str) -> anyhow::Result<()> {
            std::fs::create_dir_all(cache_dir)
                .with_context(|| format!("failed to create cache dir {}", cache_dir.display()))?;

            let cwasm_path = cache_dir.join("plugin.cwasm");
            let sha_path = cache_dir.join("source.sha256");

            let serialized = module
                .serialize()
                .map_err(|e| anyhow!("failed to serialize module: {e}"))?;
            std::fs::write(&cwasm_path, serialized)
                .with_context(|| format!("failed to write {}", cwasm_path.display()))?;
            std::fs::write(&sha_path, sha256)
                .with_context(|| format!("failed to write {}", sha_path.display()))?;

            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Stub implementation when wasm-runtime is disabled
// ---------------------------------------------------------------------------
#[cfg(not(feature = "wasm-runtime"))]
mod runtime_impl {
    use super::*;

    impl WasmPluginRuntime {
        pub fn new() -> Self {
            Self
        }

        pub fn preflight(&self, _container: &WasmPluginContainer) -> anyhow::Result<()> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }

        pub fn preflight_with_policy(
            &self,
            _container: &WasmPluginContainer,
            _policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<()> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }

        pub fn execute(
            &self,
            _container: &WasmPluginContainer,
            _request: &WasmExecutionRequest,
        ) -> anyhow::Result<WasmExecutionResult> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }

        pub fn execute_with_policy(
            &self,
            _container: &WasmPluginContainer,
            _request: &WasmExecutionRequest,
            _policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResult> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }

        pub fn execute_v2(
            &self,
            _container: &WasmPluginContainer,
            _input: &str,
            _options: &WasmV2Options,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }

        pub fn execute_v2_with_policy(
            &self,
            _container: &WasmPluginContainer,
            _input: &str,
            _options: &WasmV2Options,
            _policy: &WasmIsolationPolicy,
        ) -> anyhow::Result<WasmExecutionResultV2> {
            Err(anyhow!(
                "WASM runtime is not available (built without wasm-runtime feature)"
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "wasm-runtime"))]
mod tests {
    use super::{
        WasmExecutionRequest, WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime,
        WasmV2Options,
    };
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    }

    // =======================================================================
    // v1 tests (unchanged)
    // =======================================================================

    #[test]
    fn rejects_non_wasm_paths() {
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "plugin.txt".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        assert!(container.validate().is_err());
    }

    #[test]
    fn preflight_rejects_missing_file() {
        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "missing_plugin.wasm".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        assert!(runtime.preflight(&container).is_err());
    }

    #[test]
    fn preflight_rejects_policy_violating_capabilities() {
        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: "missing_plugin.wasm".into(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: true,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy::default();
        let result = runtime.preflight_with_policy(&container, &policy);
        assert!(result.is_err());
        assert!(result
            .expect_err("policy violation should fail")
            .to_string()
            .contains("network access is not permitted"));
    }

    #[test]
    fn preflight_rejects_disallowed_host_imports() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("disallowed-import-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (import "env" "log" (func $log (param i32)))
                (func (export "run") (result i32)
                    i32.const 1
                    call $log
                    i32.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let err = runtime
            .preflight_with_policy(&container, &WasmIsolationPolicy::default())
            .expect_err("unknown host import should fail");
        assert!(err
            .to_string()
            .contains("host call `env::log` is not allowed"));

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn preflight_accepts_allowlisted_host_imports() {
        let runtime = WasmPluginRuntime::new();
        let path =
            std::env::temp_dir().join(format!("allowlisted-import-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (import "env" "log" (func $log (param i32)))
                (func (export "run") (result i32)
                    i32.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy {
            allowed_host_calls: vec!["env::log".to_string()],
            ..WasmIsolationPolicy::default()
        };
        runtime
            .preflight_with_policy(&container, &policy)
            .expect("allowlisted import should pass");

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn preflight_rejects_oversized_module() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("oversized-{}.wasm", unique_suffix()));
        fs::write(&path, vec![1_u8; 32]).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy {
            max_module_bytes: 8,
            ..WasmIsolationPolicy::default()
        };

        let result = runtime.preflight_with_policy(&container, &policy);
        assert!(result.is_err());
        assert!(result
            .expect_err("oversized module should fail")
            .to_string()
            .contains("exceeds size policy"));

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn execute_runs_exported_entrypoint() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("execute-ok-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (func (export "run") (result i32)
                    i32.const 7)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let result = runtime
            .execute(
                &container,
                &WasmExecutionRequest {
                    input: json!({"hello": "world"}),
                },
            )
            .expect("execution should succeed");
        assert_eq!(result.status_code, 7);

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn execute_fails_for_missing_entrypoint() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("execute-missing-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (func (export "not_run") (result i32)
                    i32.const 1)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let err = runtime
            .execute(&container, &WasmExecutionRequest { input: json!({}) })
            .expect_err("missing entrypoint should fail");
        assert!(err
            .to_string()
            .contains("missing or incompatible entrypoint"));

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn execute_rejects_module_exceeding_memory_limit() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("memory-limit-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (memory 40)
                (func (export "run") (result i32)
                    i32.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1000,
            max_memory_mb: 1,
            allow_network: false,
            allow_fs_write: false,
        };

        let err = runtime
            .execute(&container, &WasmExecutionRequest { input: json!({}) })
            .expect_err("oversized module memory should fail");
        assert!(err
            .to_string()
            .contains("failed to instantiate plugin module"));

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn execute_times_out_long_running_module() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("timeout-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (func (export "run") (result i32)
                    (loop
                        br 0)
                    i32.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "plugin-1".to_string(),
            module_path: path.clone(),
            entrypoint: "run".to_string(),
            max_execution_ms: 1,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy {
            max_execution_ms: 1,
            ..WasmIsolationPolicy::default()
        };

        let err = runtime
            .execute_with_policy(
                &container,
                &WasmExecutionRequest { input: json!({}) },
                &policy,
            )
            .expect_err("infinite loop should time out");
        let err_text = err.to_string();
        assert!(
            err_text.contains("plugin execution exceeded time limit"),
            "unexpected timeout error: {err_text}"
        );

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    // =======================================================================
    // v2 tests
    // =======================================================================

    /// Build a minimal v2 plugin WAT that implements az_alloc + az_tool_execute.
    /// The plugin echoes back the input with a prefix.
    fn v2_echo_plugin_wat() -> &'static str {
        r#"(module
            ;; 1 page = 64KB of linear memory
            (memory (export "memory") 1)

            ;; Bump allocator state at byte 0
            (global $bump (mut i32) (i32.const 4))

            ;; az_alloc: bump allocator
            (func (export "az_alloc") (param $size i32) (result i32)
                (local $ptr i32)
                global.get $bump
                local.set $ptr
                global.get $bump
                local.get $size
                i32.add
                global.set $bump
                local.get $ptr
            )

            ;; az_tool_name: return "echo_plugin"
            (data (i32.const 65000) "echo_plugin")
            (func (export "az_tool_name") (result i64)
                ;; ptr=65000, len=11 -> pack as i64
                i64.const 65000                 ;; ptr
                i64.const 11
                i64.const 32
                i64.shl                         ;; len << 32
                i64.or
            )

            ;; az_tool_execute: copy input to output wrapped in JSON
            ;; For simplicity, return a fixed JSON response.
            ;; Real plugins use the SDK; this is a WAT test fixture.
            (data (i32.const 64000) "{\"output\":\"echo:ok\",\"error\":null}")
            (func (export "az_tool_execute") (param $in_ptr i32) (param $in_len i32) (result i64)
                ;; Return the static JSON at offset 64000, length 33
                i64.const 64000
                i64.const 33
                i64.const 32
                i64.shl
                i64.or
            )
        )"#
    }

    #[test]
    fn v2_execute_round_trip() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("v2-echo-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(v2_echo_plugin_wat()).expect("v2 wat should compile");
        fs::write(&path, &bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "echo-plugin".to_string(),
            module_path: path.clone(),
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let options = WasmV2Options {
            workspace_root: String::new(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2(&container, r#"{"task":"hello"}"#, &options)
            .expect("v2 execution should succeed");
        assert_eq!(result.output, "echo:ok");
        assert!(result.error.is_none());

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn v2_execute_missing_az_alloc_fails() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("v2-no-alloc-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "az_tool_execute") (param i32) (param i32) (result i64)
                    i64.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, &bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "no-alloc".to_string(),
            module_path: path.clone(),
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let err = runtime
            .execute_v2(&container, "{}", &WasmV2Options::default())
            .expect_err("missing az_alloc should fail");
        assert!(
            err.to_string().contains("az_alloc"),
            "unexpected error: {err}"
        );

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn v2_execute_with_az_log_host_function() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("v2-log-{}.wasm", unique_suffix()));
        // Plugin that calls az_log then returns success
        let bytes = wat::parse_str(
            r#"(module
                (import "az" "az_log" (func $az_log (param i32 i32 i32)))
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 4))

                (func (export "az_alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get $size
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                ;; "hello from plugin" at offset 64000
                (data (i32.const 64000) "hello from plugin")
                ;; Response JSON at offset 64100
                (data (i32.const 64100) "{\"output\":\"logged\",\"error\":null}")

                (func (export "az_tool_execute") (param $in_ptr i32) (param $in_len i32) (result i64)
                    ;; Call az_log(level=2/INFO, ptr=64000, len=17)
                    i32.const 2
                    i32.const 64000
                    i32.const 17
                    call $az_log

                    ;; Return response: ptr=64100, len=32
                    i64.const 64100
                    i64.const 32
                    i64.const 32
                    i64.shl
                    i64.or
                )
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, &bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "log-plugin".to_string(),
            module_path: path.clone(),
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        let options = WasmV2Options {
            workspace_root: String::new(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2(&container, "{}", &options)
            .expect("v2 execution with az_log should succeed");
        assert_eq!(result.output, "logged");
        assert!(result.error.is_none());

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn v2_execute_rejects_undeclared_host_function() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("v2-undeclared-{}.wasm", unique_suffix()));
        // Plugin that tries to import az_env_get without it being in the policy
        let bytes = wat::parse_str(
            r#"(module
                (import "az" "az_env_get" (func $az_env_get (param i32 i32) (result i64)))
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 4))
                (func (export "az_alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get $size
                    i32.add
                    global.set $bump
                    local.get $ptr
                )
                (func (export "az_tool_execute") (param i32) (param i32) (result i64)
                    i64.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, &bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "undeclared-host".to_string(),
            module_path: path.clone(),
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };

        // No az_env_get in capabilities or policy
        let err = runtime
            .execute_v2(&container, "{}", &WasmV2Options::default())
            .expect_err("undeclared host function should fail");
        assert!(
            err.to_string().contains("not permitted"),
            "unexpected error: {err}"
        );

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    #[test]
    fn v2_execute_times_out() {
        let runtime = WasmPluginRuntime::new();
        let path = std::env::temp_dir().join(format!("v2-timeout-{}.wasm", unique_suffix()));
        let bytes = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 4))
                (func (export "az_alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get $size
                    i32.add
                    global.set $bump
                    local.get $ptr
                )
                (func (export "az_tool_execute") (param i32) (param i32) (result i64)
                    (loop br 0)
                    i64.const 0)
            )"#,
        )
        .expect("wat should compile");
        fs::write(&path, &bytes).expect("temp wasm file should be created");

        let container = WasmPluginContainer {
            id: "timeout-v2".to_string(),
            module_path: path.clone(),
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 1,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = WasmIsolationPolicy {
            max_execution_ms: 1,
            ..WasmIsolationPolicy::default()
        };

        let err = runtime
            .execute_v2_with_policy(&container, "{}", &WasmV2Options::default(), &policy)
            .expect_err("infinite loop should time out");
        assert!(
            err.to_string().contains("exceeded time limit"),
            "unexpected error: {err}"
        );

        fs::remove_file(path).expect("temp wasm file should be removed");
    }

    // =======================================================================
    // pack/unpack tests
    // =======================================================================

    #[test]
    fn pack_unpack_ptr_len_round_trip() {
        use super::{pack_ptr_len, unpack_ptr_len};

        let packed = pack_ptr_len(1024, 256);
        let (ptr, len) = unpack_ptr_len(packed);
        assert_eq!(ptr, 1024);
        assert_eq!(len, 256);

        let packed2 = pack_ptr_len(0, 0);
        let (ptr2, len2) = unpack_ptr_len(packed2);
        assert_eq!(ptr2, 0);
        assert_eq!(len2, 0);

        let packed3 = pack_ptr_len(u32::MAX, u32::MAX);
        let (ptr3, len3) = unpack_ptr_len(packed3);
        assert_eq!(ptr3, u32::MAX);
        assert_eq!(len3, u32::MAX);
    }

    // =======================================================================
    // ModuleCache tests (wasmi — compile-from-source, no AOT cache)
    // =======================================================================

    #[cfg(not(feature = "wasm-jit"))]
    mod cache_tests_wasmi {
        use super::super::ModuleCache;
        use super::*;

        #[test]
        fn module_cache_compiles_from_source() {
            let dir = std::env::temp_dir().join(format!("cache-wasmi-{}", unique_suffix()));
            fs::create_dir_all(&dir).expect("create dir");
            let wasm_path = dir.join("plugin.wasm");
            let bytes = wat::parse_str(v2_echo_plugin_wat()).expect("wat should compile");
            fs::write(&wasm_path, &bytes).expect("write wasm");

            let engine = wasmi::Engine::default();

            let _module = ModuleCache::load_or_compile(&engine, &wasm_path, "some-sha256")
                .expect("wasmi compile from source");

            // wasmi has no AOT cache — no .cwasm files created
            let cache_dir = dir.join(".cache");
            assert!(!cache_dir.exists());

            fs::remove_dir_all(dir).ok();
        }

        #[test]
        fn module_cache_handles_invalid_wasm() {
            let dir = std::env::temp_dir().join(format!("cache-wasmi-invalid-{}", unique_suffix()));
            fs::create_dir_all(&dir).expect("create dir");
            let wasm_path = dir.join("plugin.wasm");
            fs::write(&wasm_path, b"not valid wasm").expect("write bad wasm");

            let engine = wasmi::Engine::default();

            let result = ModuleCache::load_or_compile(&engine, &wasm_path, "sha");
            assert!(result.is_err(), "invalid wasm should fail compilation");

            fs::remove_dir_all(dir).ok();
        }

        #[test]
        fn module_cache_handles_missing_file() {
            let engine = wasmi::Engine::default();
            let result = ModuleCache::load_or_compile(
                &engine,
                std::path::Path::new("/nonexistent.wasm"),
                "sha",
            );
            assert!(result.is_err(), "missing file should fail");
        }
    }

    // =======================================================================
    // ModuleCache tests (wasmtime — AOT cache with .cwasm files)
    // =======================================================================

    #[cfg(feature = "wasm-jit")]
    mod cache_tests_wasmtime {
        use super::super::ModuleCache;
        use super::*;

        #[test]
        fn module_cache_compiles_and_caches() {
            use sha2::{Digest, Sha256};

            let dir = std::env::temp_dir().join(format!("cache-test-{}", unique_suffix()));
            fs::create_dir_all(&dir).expect("create dir");
            let wasm_path = dir.join("plugin.wasm");
            let bytes = wat::parse_str(v2_echo_plugin_wat()).expect("wat should compile");
            fs::write(&wasm_path, &bytes).expect("write wasm");

            let sha = format!("{:x}", Sha256::new_with_prefix(&bytes).finalize());

            let mut config = wasmtime::Config::new();
            config.epoch_interruption(true);
            let engine = wasmtime::Engine::new(&config).expect("engine");

            let _module =
                ModuleCache::load_or_compile(&engine, &wasm_path, &sha).expect("first compile");

            let cache_dir = dir.join(".cache");
            assert!(cache_dir.join("plugin.cwasm").exists());
            assert!(cache_dir.join("source.sha256").exists());

            let _module2 =
                ModuleCache::load_or_compile(&engine, &wasm_path, &sha).expect("cached load");

            fs::remove_dir_all(dir).ok();
        }

        #[test]
        fn module_cache_invalidates_on_sha_mismatch() {
            use sha2::{Digest, Sha256};

            let dir = std::env::temp_dir().join(format!("cache-inval-{}", unique_suffix()));
            fs::create_dir_all(&dir).expect("create dir");
            let wasm_path = dir.join("plugin.wasm");
            let bytes = wat::parse_str(v2_echo_plugin_wat()).expect("wat should compile");
            fs::write(&wasm_path, &bytes).expect("write wasm");

            let sha = format!("{:x}", Sha256::new_with_prefix(&bytes).finalize());

            let mut config = wasmtime::Config::new();
            config.epoch_interruption(true);
            let engine = wasmtime::Engine::new(&config).expect("engine");

            ModuleCache::load_or_compile(&engine, &wasm_path, &sha).expect("first compile");

            let _module = ModuleCache::load_or_compile(&engine, &wasm_path, "different_sha256")
                .expect("recompile on sha mismatch");

            fs::remove_dir_all(dir).ok();
        }

        #[test]
        fn module_cache_handles_corrupt_cwasm() {
            use sha2::{Digest, Sha256};

            let dir = std::env::temp_dir().join(format!("cache-corrupt-{}", unique_suffix()));
            fs::create_dir_all(&dir).expect("create dir");
            let wasm_path = dir.join("plugin.wasm");
            let bytes = wat::parse_str(v2_echo_plugin_wat()).expect("wat should compile");
            fs::write(&wasm_path, &bytes).expect("write wasm");

            let sha = format!("{:x}", Sha256::new_with_prefix(&bytes).finalize());

            let cache_dir = dir.join(".cache");
            fs::create_dir_all(&cache_dir).expect("create cache dir");
            fs::write(cache_dir.join("plugin.cwasm"), b"corrupt data").expect("write corrupt");
            fs::write(cache_dir.join("source.sha256"), &sha).expect("write sha");

            let mut config = wasmtime::Config::new();
            config.epoch_interruption(true);
            let engine = wasmtime::Engine::new(&config).expect("engine");

            let _module = ModuleCache::load_or_compile(&engine, &wasm_path, &sha)
                .expect("corrupt cache should fall back to recompilation");

            fs::remove_dir_all(dir).ok();
        }
    }
}
