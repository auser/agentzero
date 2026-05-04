//! WASM sandbox runtime for AgentZero.
//!
//! Executes WASM modules inside a sandboxed wasmtime engine per ADR 0006.
//! WASM modules have no ambient host access — all capabilities are
//! explicitly declared and policy-checked.
//!
//! Requires the `wasm` feature flag: `cargo build --features wasm`

#[cfg(feature = "wasm")]
mod runtime {
    use agentzero_core::ExecutionId;
    use agentzero_tracing::{debug, info};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum WasmError {
        #[error("wasm compilation failed: {0}")]
        CompilationFailed(String),
        #[error("wasm execution failed: {0}")]
        ExecutionFailed(String),
        #[error("wasm module not found: {0}")]
        NotFound(String),
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

        /// Execute a WASM module from bytes.
        pub fn execute(&self, wasm_bytes: &[u8]) -> Result<WasmResult, WasmError> {
            let execution_id = ExecutionId::new();

            debug!(
                execution_id = %execution_id,
                bytes = wasm_bytes.len(),
                "compiling wasm module"
            );

            let module = wasmtime::Module::new(&self.engine, wasm_bytes)
                .map_err(|e| WasmError::CompilationFailed(e.to_string()))?;

            let mut store = wasmtime::Store::new(&self.engine, ());

            // Set fuel limit based on duration (rough approximation)
            let fuel = self.config.max_duration_secs * 1_000_000;
            store
                .set_fuel(fuel)
                .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

            let instance = wasmtime::Instance::new(&mut store, &module, &[])
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
