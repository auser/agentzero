//! WASM sandbox runtime for AgentZero.
//!
//! Executes WASM modules inside a sandboxed wasmtime engine per ADR 0006.
//! WASM modules have no ambient host access — all capabilities are
//! explicitly declared and policy-checked.
//!
//! Requires the `wasm` feature flag: `cargo build --features wasm`

/// Trait for host callbacks invoked by WASM guest modules.
///
/// Implementations provide policy-checked filesystem, logging, and secret
/// access. The sandbox crate defines the trait; `agentzero-session` provides
/// the implementation that delegates to `ToolExecutor` + `PolicyEngine`.
///
/// See ADR 0013 (WIT interface) for the intended contract.
pub trait WasmHostCallbacks: Send + Sync {
    /// Read file contents. Must policy-check FileRead before returning.
    fn read_file(&self, path: &str) -> Result<String, String>;
    /// Write file contents. Must policy-check FileWrite before returning.
    fn write_file(&self, path: &str, content: &str) -> Result<bool, String>;
    /// Emit an audit-logged message from the WASM guest.
    fn log(&self, message: &str);
}

/// No-op host callbacks for modules that don't need host access.
/// All operations return errors.
pub struct DenyAllHostCallbacks;

impl WasmHostCallbacks for DenyAllHostCallbacks {
    fn read_file(&self, _path: &str) -> Result<String, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn write_file(&self, _path: &str, _content: &str) -> Result<bool, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn log(&self, _message: &str) {}
}

#[cfg(feature = "wasm")]
mod runtime {
    use agentzero_core::ExecutionId;
    use agentzero_tracing::{debug, info};
    use thiserror::Error;

    use super::WasmHostCallbacks;
    use std::sync::Arc;

    #[derive(Debug, Error)]
    pub enum WasmError {
        #[error("wasm compilation failed: {0}")]
        CompilationFailed(String),
        #[error("wasm execution failed: {0}")]
        ExecutionFailed(String),
        #[error("wasm module not found: {0}")]
        NotFound(String),
    }

    /// State held in the wasmtime `Store`, accessible to host functions.
    struct HostState {
        callbacks: Arc<dyn WasmHostCallbacks>,
    }

    /// Result of a WASM module execution.
    #[derive(Debug, Clone)]
    pub struct WasmResult {
        pub execution_id: ExecutionId,
        pub success: bool,
        pub output: String,
    }

    /// Configuration for the WASM sandbox engine.
    #[derive(Debug, Clone)]
    pub struct WasmConfig {
        /// Maximum memory in bytes (default: 64MB).
        pub max_memory_bytes: u64,
        /// Maximum execution time in seconds.
        pub max_duration_secs: u64,
        /// Whether to allow WASI filesystem access.
        pub allow_filesystem: bool,
    }

    impl Default for WasmConfig {
        fn default() -> Self {
            Self {
                max_memory_bytes: 64 * 1024 * 1024,
                max_duration_secs: 30,
                allow_filesystem: false,
            }
        }
    }

    /// WASM sandbox engine backed by wasmtime.
    pub struct WasmEngine {
        engine: wasmtime::Engine,
        config: WasmConfig,
    }

    impl WasmEngine {
        /// Create a new WASM engine with the given configuration.
        pub fn new(config: WasmConfig) -> Result<Self, WasmError> {
            let mut wasm_config = wasmtime::Config::new();
            wasm_config.consume_fuel(true);

            let engine = wasmtime::Engine::new(&wasm_config)
                .map_err(|e| WasmError::CompilationFailed(e.to_string()))?;

            info!(
                max_memory = config.max_memory_bytes,
                max_duration = config.max_duration_secs,
                "wasm engine created"
            );

            Ok(Self { engine, config })
        }

        /// Execute a WASM module from bytes (no host imports).
        ///
        /// Modules with imports are rejected. Use `execute_with_host` for
        /// modules that need filesystem/logging/secrets access.
        pub fn execute(&self, wasm_bytes: &[u8]) -> Result<WasmResult, WasmError> {
            self.execute_inner(wasm_bytes, None)
        }

        /// Execute a WASM module with host callbacks providing filesystem,
        /// logging, and secret access per ADR 0013.
        ///
        /// Host functions are registered via wasmtime `Linker` under the
        /// `"az"` module namespace:
        /// - `az::read_file(ptr, len) -> ptr` (policy-checked)
        /// - `az::write_file(path_ptr, path_len, content_ptr, content_len) -> i32`
        /// - `az::log(ptr, len)`
        ///
        /// Modules that don't import any `az::*` functions work without callbacks.
        pub fn execute_with_host(
            &self,
            wasm_bytes: &[u8],
            callbacks: Arc<dyn WasmHostCallbacks>,
        ) -> Result<WasmResult, WasmError> {
            self.execute_inner(wasm_bytes, Some(callbacks))
        }

        fn execute_inner(
            &self,
            wasm_bytes: &[u8],
            callbacks: Option<Arc<dyn WasmHostCallbacks>>,
        ) -> Result<WasmResult, WasmError> {
            let execution_id = ExecutionId::new();

            debug!(
                execution_id = %execution_id,
                bytes = wasm_bytes.len(),
                has_host = callbacks.is_some(),
                "compiling wasm module"
            );

            let module = wasmtime::Module::new(&self.engine, wasm_bytes)
                .map_err(|e| WasmError::CompilationFailed(e.to_string()))?;

            // Validate imports: only allow "az" module imports when callbacks are provided.
            let imports: Vec<_> = module.imports().collect();
            let has_az_imports = imports.iter().any(|i| i.module() == "az");
            let has_other_imports = imports.iter().any(|i| i.module() != "az");

            if has_other_imports {
                let unknown: Vec<String> = imports
                    .iter()
                    .filter(|i| i.module() != "az")
                    .map(|i| format!("{}::{}", i.module(), i.name()))
                    .collect();
                return Err(WasmError::ExecutionFailed(format!(
                    "WASM module has undeclared imports (only az::* allowed): {}",
                    unknown.join(", ")
                )));
            }

            if has_az_imports && callbacks.is_none() {
                return Err(WasmError::ExecutionFailed(
                    "WASM module imports az::* functions but no host callbacks provided".into(),
                ));
            }

            // Build store with host state
            let host_state = HostState {
                callbacks: callbacks.unwrap_or_else(|| {
                    Arc::new(super::DenyAllHostCallbacks)
                }),
            };
            let mut store = wasmtime::Store::new(&self.engine, host_state);

            let fuel = self.config.max_duration_secs * 1_000_000;
            store
                .set_fuel(fuel)
                .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

            // Register host functions via Linker
            let mut linker = wasmtime::Linker::new(&self.engine);

            // az::log(ptr: i32, len: i32)
            linker
                .func_wrap("az", "log", |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| {
                    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                        let data = memory.data(&caller);
                        if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                            if let Ok(msg) = std::str::from_utf8(slice) {
                                caller.data().callbacks.log(msg);
                            }
                        }
                    }
                })
                .map_err(|e| WasmError::ExecutionFailed(format!("failed to register az::log: {e}")))?;

            // az::read_file(ptr: i32, len: i32) -> i32
            // Returns 0 on success (result written to shared state), 1 on error.
            // For Phase 1 we use a simple return code; string passing via shared
            // memory will be refined when WIT component model is adopted.
            linker
                .func_wrap("az", "read_file", |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
                    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                        let data = memory.data(&caller);
                        if let Some(slice) = data.get(ptr as usize..(ptr as usize + len as usize)) {
                            if let Ok(path) = std::str::from_utf8(slice) {
                                match caller.data().callbacks.read_file(path) {
                                    Ok(_content) => return 0, // success
                                    Err(_) => return 1,       // error
                                }
                            }
                        }
                    }
                    1 // error
                })
                .map_err(|e| WasmError::ExecutionFailed(format!("failed to register az::read_file: {e}")))?;

            // az::write_file(path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32) -> i32
            linker
                .func_wrap(
                    "az",
                    "write_file",
                    |mut caller: wasmtime::Caller<'_, HostState>, path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32| -> i32 {
                        if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                            let data = memory.data(&caller);
                            let path = data
                                .get(path_ptr as usize..(path_ptr as usize + path_len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok());
                            let content = data
                                .get(content_ptr as usize..(content_ptr as usize + content_len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok());
                            if let (Some(path), Some(content)) = (path, content) {
                                match caller.data().callbacks.write_file(path, content) {
                                    Ok(true) => return 0,
                                    _ => return 1,
                                }
                            }
                        }
                        1
                    },
                )
                .map_err(|e| WasmError::ExecutionFailed(format!("failed to register az::write_file: {e}")))?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

            // Try to call _start (WASI convention) or main
            let result = if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start")
            {
                start.call(&mut store, ()).map(|()| String::new())
            } else if let Ok(main) = instance.get_typed_func::<(), i32>(&mut store, "main") {
                main.call(&mut store, ())
                    .map(|code| format!("exit code: {code}"))
            } else {
                Ok("module loaded (no _start or main export)".into())
            };

            match result {
                Ok(output) => {
                    info!(execution_id = %execution_id, "wasm execution complete");
                    Ok(WasmResult {
                        execution_id,
                        success: true,
                        output,
                    })
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("fuel") {
                        Err(WasmError::ExecutionFailed(
                            "execution exceeded time limit".into(),
                        ))
                    } else {
                        Err(WasmError::ExecutionFailed(msg))
                    }
                }
            }
        }
    }
}

#[cfg(feature = "wasm")]
pub use runtime::{WasmConfig, WasmEngine, WasmError, WasmResult};
// WasmHostCallbacks and DenyAllHostCallbacks are always available (no feature gate)
// since they're traits that other crates implement.

// When the wasm feature is not enabled, provide stub types for compilation
#[cfg(not(feature = "wasm"))]
mod stubs {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum WasmError {
        #[error("wasm feature not enabled — rebuild with --features wasm")]
        NotEnabled,
    }

    /// Check if the wasm feature is available.
    pub fn is_available() -> bool {
        false
    }
}

#[cfg(not(feature = "wasm"))]
pub use stubs::{is_available as wasm_is_available, WasmError};

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::*;

    /// Minimal valid WASM module that exports `main() -> i32` returning 42.
    ///
    /// WAT equivalent:
    /// ```wat
    /// (module
    ///   (func $main (export "main") (result i32)
    ///     i32.const 42)
    ///   (memory (export "memory") 1))
    /// ```
    fn minimal_wasm_module() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6D, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
            // Type section (1 type: () -> i32)
            0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F,
            // Function section (1 func, type 0)
            0x03, 0x02, 0x01, 0x00, // Memory section (1 memory, min 1 page)
            0x05, 0x03, 0x01, 0x00, 0x01,
            // Export section (2 exports: "main" func 0, "memory" mem 0)
            0x07, 0x11, 0x02, 0x04, 0x6D, 0x61, 0x69, 0x6E, 0x00, 0x00, 0x06, 0x6D, 0x65, 0x6D,
            0x6F, 0x72, 0x79, 0x02, 0x00,
            // Code section (1 func body: i32.const 42, end)
            0x0A, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2A, 0x0B,
        ]
    }

    #[test]
    fn execute_minimal_wasm_module() {
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let wasm = minimal_wasm_module();
        let result = engine.execute(&wasm).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("42"));
    }

    #[test]
    fn wasm_rejects_invalid_bytes() {
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute(b"not valid wasm");
        assert!(result.is_err());
    }

    #[test]
    fn wasm_respects_fuel_limits() {
        let config = WasmConfig {
            max_duration_secs: 0, // zero fuel
            ..WasmConfig::default()
        };
        let engine = WasmEngine::new(config).expect("engine should create");
        let wasm = minimal_wasm_module();
        // With zero fuel, execution should fail
        let result = engine.execute(&wasm);
        assert!(result.is_err());
    }

    #[test]
    fn wasm_rejects_module_with_non_az_imports() {
        // wasmtime can parse WAT text format directly
        let wat = r#"(module (import "env" "abort" (func)))"#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute(wat.as_bytes());
        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(
            err.to_string().contains("undeclared imports"),
            "error should mention undeclared imports: {err}"
        );
    }

    #[test]
    fn wasm_rejects_az_imports_without_callbacks() {
        let wat = r#"(module (import "az" "log" (func (param i32 i32))))"#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute(wat.as_bytes());
        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(
            err.to_string().contains("no host callbacks"),
            "error should mention no host callbacks: {err}"
        );
    }

    #[test]
    fn wasm_accepts_az_imports_with_callbacks() {
        use super::DenyAllHostCallbacks;
        use std::sync::Arc;

        // Module that imports az::log and exports main
        let wat = r#"
            (module
                (import "az" "log" (func $log (param i32 i32)))
                (memory (export "memory") 1)
                (func (export "main") (result i32)
                    i32.const 0
                    i32.const 5
                    call $log
                    i32.const 0))
        "#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute_with_host(wat.as_bytes(), Arc::new(DenyAllHostCallbacks));
        assert!(result.is_ok(), "should succeed with callbacks: {result:?}");
        let output = result.expect("should succeed");
        assert!(output.success);
        assert!(output.output.contains("0"));
    }

    #[test]
    fn wasm_default_config_is_sane() {
        let config = WasmConfig::default();
        assert_eq!(config.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_duration_secs, 30);
        assert!(!config.allow_filesystem);
    }
}
