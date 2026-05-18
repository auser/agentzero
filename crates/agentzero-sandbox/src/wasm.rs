//! WASM sandbox runtime for AgentZero.
//!
//! Executes WASM modules inside a sandboxed wasmtime engine per ADR 0006.
//! WASM modules have no ambient host access — all capabilities are
//! explicitly declared and policy-checked.
//!
//! Requires the `wasm` feature flag: `cargo build --features wasm`

/// Trait for host callbacks invoked by WASM guest modules.
///
/// Implementations provide policy-checked filesystem, logging, clock,
/// and secret access. The sandbox crate defines the trait;
/// `agentzero-session` provides the implementation that delegates to
/// `ToolExecutor` + `PolicyEngine`.
///
/// See ADR 0013 (WIT interface) and `az:host@0.2.0` WIT spec.
pub trait WasmHostCallbacks: Send + Sync {
    /// Read file contents. Must policy-check FileRead before returning.
    fn read_file(&self, path: &str) -> Result<String, String>;
    /// Write file contents. Must policy-check FileWrite before returning.
    fn write_file(&self, path: &str, content: &str) -> Result<bool, String>;
    /// Append content to a file (creates if missing). Must policy-check FileWrite.
    fn append_file(&self, path: &str, content: &str) -> Result<bool, String>;
    /// List entries in a directory. Must policy-check FileRead.
    fn list_dir(&self, path: &str) -> Result<Vec<String>, String>;
    /// Create a directory and parents. Must policy-check FileWrite.
    fn create_dir(&self, path: &str) -> Result<bool, String>;
    /// Check whether a path exists. Must policy-check FileRead.
    fn file_exists(&self, path: &str) -> Result<bool, String>;
    /// Emit an audit-logged message from the WASM guest.
    fn log(&self, message: &str);
    /// Current date-time in ISO 8601 format.
    fn now(&self) -> String;
    /// Make an outbound HTTP request. Must policy-check NetworkRequest
    /// and validate the URL against the sandbox network policy.
    ///
    /// Returns a JSON string: `{"status": u16, "headers": {}, "body": "..."}`.
    fn http_request(
        &self,
        url: &str,
        method: &str,
        headers_json: &str,
        body: &str,
    ) -> Result<String, String>;
}

/// No-op host callbacks for modules that don't need host access.
/// All operations return errors (except `now` which is not security-sensitive).
pub struct DenyAllHostCallbacks;

impl WasmHostCallbacks for DenyAllHostCallbacks {
    fn read_file(&self, _path: &str) -> Result<String, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn write_file(&self, _path: &str, _content: &str) -> Result<bool, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn append_file(&self, _path: &str, _content: &str) -> Result<bool, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn list_dir(&self, _path: &str) -> Result<Vec<String>, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn create_dir(&self, _path: &str) -> Result<bool, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn file_exists(&self, _path: &str) -> Result<bool, String> {
        Err("host imports not available (no callbacks provided)".into())
    }
    fn log(&self, _message: &str) {}
    fn now(&self) -> String {
        chrono::Local::now().to_rfc3339()
    }
    fn http_request(
        &self,
        _url: &str,
        _method: &str,
        _headers_json: &str,
        _body: &str,
    ) -> Result<String, String> {
        Err("network access denied (no callbacks provided)".into())
    }
}

#[cfg(feature = "wasm")]
mod runtime {
    use agentzero_core::ExecutionId;
    use agentzero_tracing::{debug, info};
    use thiserror::Error;
    use wasmtime::{AsContext, AsContextMut};

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

    /// Write a string into guest linear memory using the guest-exported `alloc` function.
    ///
    /// Returns `(ptr, len)` packed into an i64: high 32 bits = ptr, low 32 bits = len.
    /// Returns -1 if allocation or write fails.
    ///
    /// The guest must export: `alloc(size: i32) -> i32`
    fn write_string_to_guest(
        caller: &mut wasmtime::Caller<'_, HostState>,
        instance: Option<&wasmtime::Instance>,
        s: &str,
    ) -> i64 {
        let alloc = if let Some(inst) = instance {
            inst.get_typed_func::<i32, i32>(caller.as_context_mut(), "alloc")
                .ok()
        } else {
            caller
                .get_export("alloc")
                .and_then(|e| e.into_func())
                .and_then(|f| f.typed::<i32, i32>(caller.as_context()).ok())
        };

        let alloc = match alloc {
            Some(f) => f,
            None => return -1,
        };

        let len = s.len() as i32;
        let ptr = match alloc.call(caller.as_context_mut(), len) {
            Ok(p) => p,
            Err(_) => return -1,
        };

        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(m) => m,
            None => return -1,
        };

        let dest = memory.data_mut(caller.as_context_mut());
        let start = ptr as usize;
        let end = start + s.len();
        if end > dest.len() {
            return -1;
        }
        dest[start..end].copy_from_slice(s.as_bytes());

        ((ptr as i64) << 32) | (len as i64)
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
            self.execute_inner(wasm_bytes, None, None)
        }

        /// Execute a WASM module with host callbacks providing filesystem,
        /// logging, and secret access per ADR 0013.
        ///
        /// Host functions are registered via wasmtime `Linker` under the
        /// `"az"` module namespace:
        /// - `az::read_file(ptr, len) -> i64` (policy-checked)
        /// - `az::write_file(path_ptr, path_len, content_ptr, content_len) -> i32`
        /// - `az::log(ptr, len)`
        ///
        /// Modules that don't import any `az::*` functions work without callbacks.
        pub fn execute_with_host(
            &self,
            wasm_bytes: &[u8],
            callbacks: Arc<dyn WasmHostCallbacks>,
        ) -> Result<WasmResult, WasmError> {
            self.execute_inner(wasm_bytes, Some(callbacks), None)
        }

        /// Execute a WASM module with host callbacks and an input string.
        ///
        /// If the module exports `run(ptr: i32, len: i32) -> i64`, the input
        /// is written into guest memory via the alloc protocol and `run` is
        /// called. The return value is a packed `(ptr, len)` i64 pointing to
        /// the output string in guest memory.
        ///
        /// Falls back to `_start()` / `main()` if `run` is not exported.
        pub fn execute_with_input(
            &self,
            wasm_bytes: &[u8],
            callbacks: Arc<dyn WasmHostCallbacks>,
            input: &str,
        ) -> Result<WasmResult, WasmError> {
            self.execute_inner(wasm_bytes, Some(callbacks), Some(input))
        }

        fn execute_inner(
            &self,
            wasm_bytes: &[u8],
            callbacks: Option<Arc<dyn WasmHostCallbacks>>,
            input: Option<&str>,
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
                callbacks: callbacks.unwrap_or_else(|| Arc::new(super::DenyAllHostCallbacks)),
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
                .func_wrap(
                    "az",
                    "log",
                    |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| {
                        if let Some(memory) =
                            caller.get_export("memory").and_then(|e| e.into_memory())
                        {
                            let data = memory.data(&caller);
                            if let Some(slice) =
                                data.get(ptr as usize..(ptr as usize + len as usize))
                            {
                                if let Ok(msg) = std::str::from_utf8(slice) {
                                    caller.data().callbacks.log(msg);
                                }
                            }
                        }
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::log: {e}"))
                })?;

            // az::read_file(ptr: i32, len: i32) -> i64
            // Returns packed (ptr, len) via write_string_to_guest on success, -1 on error.
            // Guest must export `alloc(size: i32) -> i32` to receive string data.
            linker
                .func_wrap(
                    "az",
                    "read_file",
                    |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i64 {
                        let path = {
                            let memory =
                                match caller.get_export("memory").and_then(|e| e.into_memory()) {
                                    Some(m) => m,
                                    None => return -1,
                                };
                            let data = memory.data(&caller);
                            match data
                                .get(ptr as usize..(ptr as usize + len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok())
                            {
                                Some(p) => p.to_owned(),
                                None => return -1,
                            }
                        };
                        match caller.data().callbacks.read_file(&path) {
                            Ok(content) => write_string_to_guest(&mut caller, None, &content),
                            Err(_) => -1,
                        }
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::read_file: {e}"))
                })?;

            // az::write_file(path_ptr, path_len, content_ptr, content_len) -> i32
            linker
                .func_wrap(
                    "az",
                    "write_file",
                    |mut caller: wasmtime::Caller<'_, HostState>,
                     path_ptr: i32,
                     path_len: i32,
                     content_ptr: i32,
                     content_len: i32|
                     -> i32 {
                        if let Some(memory) =
                            caller.get_export("memory").and_then(|e| e.into_memory())
                        {
                            let data = memory.data(&caller);
                            let path = data
                                .get(path_ptr as usize..(path_ptr as usize + path_len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok());
                            let content = data
                                .get(
                                    content_ptr as usize
                                        ..(content_ptr as usize + content_len as usize),
                                )
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
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::write_file: {e}"))
                })?;

            // az::append_file(path_ptr, path_len, content_ptr, content_len) -> i32
            linker
                .func_wrap(
                    "az",
                    "append_file",
                    |mut caller: wasmtime::Caller<'_, HostState>,
                     path_ptr: i32,
                     path_len: i32,
                     content_ptr: i32,
                     content_len: i32|
                     -> i32 {
                        if let Some(memory) =
                            caller.get_export("memory").and_then(|e| e.into_memory())
                        {
                            let data = memory.data(&caller);
                            let path = data
                                .get(path_ptr as usize..(path_ptr as usize + path_len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok());
                            let content = data
                                .get(
                                    content_ptr as usize
                                        ..(content_ptr as usize + content_len as usize),
                                )
                                .and_then(|s| std::str::from_utf8(s).ok());
                            if let (Some(path), Some(content)) = (path, content) {
                                match caller.data().callbacks.append_file(path, content) {
                                    Ok(true) => return 0,
                                    _ => return 1,
                                }
                            }
                        }
                        1
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::append_file: {e}"))
                })?;

            // az::list_dir(ptr, len) -> i64
            // Returns packed (ptr, len) of a JSON array of entry names, -1 on error.
            linker
                .func_wrap(
                    "az",
                    "list_dir",
                    |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i64 {
                        let path = {
                            let memory =
                                match caller.get_export("memory").and_then(|e| e.into_memory()) {
                                    Some(m) => m,
                                    None => return -1,
                                };
                            let data = memory.data(&caller);
                            match data
                                .get(ptr as usize..(ptr as usize + len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok())
                            {
                                Some(p) => p.to_owned(),
                                None => return -1,
                            }
                        };
                        match caller.data().callbacks.list_dir(&path) {
                            Ok(entries) => {
                                let json =
                                    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into());
                                write_string_to_guest(&mut caller, None, &json)
                            }
                            Err(_) => -1,
                        }
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::list_dir: {e}"))
                })?;

            // az::create_dir(ptr, len) -> i32
            linker
                .func_wrap(
                    "az",
                    "create_dir",
                    |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
                        if let Some(memory) =
                            caller.get_export("memory").and_then(|e| e.into_memory())
                        {
                            let data = memory.data(&caller);
                            if let Some(path) = data
                                .get(ptr as usize..(ptr as usize + len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok())
                            {
                                match caller.data().callbacks.create_dir(path) {
                                    Ok(true) => return 0,
                                    _ => return 1,
                                }
                            }
                        }
                        1
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::create_dir: {e}"))
                })?;

            // az::file_exists(ptr, len) -> i32
            // Returns 0 if exists, 1 if not, -1 on error (e.g. policy denial).
            linker
                .func_wrap(
                    "az",
                    "file_exists",
                    |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
                        if let Some(memory) =
                            caller.get_export("memory").and_then(|e| e.into_memory())
                        {
                            let data = memory.data(&caller);
                            if let Some(path) = data
                                .get(ptr as usize..(ptr as usize + len as usize))
                                .and_then(|s| std::str::from_utf8(s).ok())
                            {
                                match caller.data().callbacks.file_exists(path) {
                                    Ok(true) => return 0,
                                    Ok(false) => return 1,
                                    Err(_) => return -1,
                                }
                            }
                        }
                        -1
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::file_exists: {e}"))
                })?;

            // az::now() -> i64
            // Returns packed (ptr, len) of ISO 8601 timestamp string.
            linker
                .func_wrap(
                    "az",
                    "now",
                    |mut caller: wasmtime::Caller<'_, HostState>| -> i64 {
                        let timestamp = caller.data().callbacks.now();
                        write_string_to_guest(&mut caller, None, &timestamp)
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::now: {e}"))
                })?;

            // az::http_request(url_ptr, url_len, method_ptr, method_len,
            //                  headers_ptr, headers_len, body_ptr, body_len) -> i64
            // Returns packed (ptr, len) of JSON response string, -1 on error.
            linker
                .func_wrap(
                    "az",
                    "http_request",
                    |mut caller: wasmtime::Caller<'_, HostState>,
                     url_ptr: i32,
                     url_len: i32,
                     method_ptr: i32,
                     method_len: i32,
                     headers_ptr: i32,
                     headers_len: i32,
                     body_ptr: i32,
                     body_len: i32|
                     -> i64 {
                        let (url, method, headers_json, body) = {
                            let memory =
                                match caller.get_export("memory").and_then(|e| e.into_memory()) {
                                    Some(m) => m,
                                    None => return -1,
                                };
                            let data = memory.data(&caller);
                            let read_str = |ptr: i32, len: i32| -> Option<String> {
                                data.get(ptr as usize..(ptr as usize + len as usize))
                                    .and_then(|s| std::str::from_utf8(s).ok())
                                    .map(|s| s.to_owned())
                            };
                            match (
                                read_str(url_ptr, url_len),
                                read_str(method_ptr, method_len),
                                read_str(headers_ptr, headers_len),
                                read_str(body_ptr, body_len),
                            ) {
                                (Some(u), Some(m), Some(h), Some(b)) => (u, m, h, b),
                                _ => return -1,
                            }
                        };
                        match caller.data().callbacks.http_request(
                            &url,
                            &method,
                            &headers_json,
                            &body,
                        ) {
                            Ok(response_json) => {
                                write_string_to_guest(&mut caller, None, &response_json)
                            }
                            Err(_) => -1,
                        }
                    },
                )
                .map_err(|e| {
                    WasmError::ExecutionFailed(format!("failed to register az::http_request: {e}"))
                })?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| WasmError::ExecutionFailed(e.to_string()))?;

            // Try entry points in order: run(input), _start(), main()
            //
            // If input is provided and the module exports run(ptr, len) -> i64,
            // we pass the input via the alloc protocol and read the output back.
            // Otherwise fall back to _start() or main().
            let result = if let (Some(input_str), Ok(run_fn)) = (
                input,
                instance.get_typed_func::<(i32, i32), i64>(&mut store, "run"),
            ) {
                Self::call_run(&instance, &mut store, &run_fn, input_str)
            } else if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
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

        /// Call a guest's `run(ptr, len) -> i64` export with an input string.
        ///
        /// Writes input into guest memory via alloc, calls run, reads output back.
        /// Returns `Result<String, wasmtime::Error>` to match the _start/main branches.
        fn call_run(
            instance: &wasmtime::Instance,
            store: &mut wasmtime::Store<HostState>,
            run_fn: &wasmtime::TypedFunc<(i32, i32), i64>,
            input_str: &str,
        ) -> Result<String, wasmtime::Error> {
            // Get the guest's alloc function
            let alloc_fn = instance
                .get_typed_func::<i32, i32>(store.as_context_mut(), "alloc")
                .map_err(|_| {
                    wasmtime::Error::msg("module exports run() but not alloc() — cannot pass input")
                })?;

            // Allocate space in guest memory and write the input string
            let input_bytes = input_str.as_bytes();
            let input_len = input_bytes.len() as i32;
            let input_ptr = alloc_fn.call(store.as_context_mut(), input_len)?;

            let memory = instance
                .get_memory(store.as_context_mut(), "memory")
                .ok_or_else(|| wasmtime::Error::msg("no memory export"))?;
            let dest = memory.data_mut(store.as_context_mut());
            let start_offset = input_ptr as usize;
            let end_offset = start_offset + input_bytes.len();
            if end_offset > dest.len() {
                return Err(wasmtime::Error::msg(
                    "input string too large for guest memory",
                ));
            }
            dest[start_offset..end_offset].copy_from_slice(input_bytes);

            // Call run(ptr, len) -> i64 (packed output ptr/len or -1 for error)
            let output_packed = run_fn.call(store.as_context_mut(), (input_ptr, input_len))?;

            if output_packed == -1 {
                return Err(wasmtime::Error::msg("run() returned error (-1)"));
            }

            // Read output string from guest memory
            let out_ptr = (output_packed >> 32) as usize;
            let out_len = (output_packed & 0xFFFF_FFFF) as usize;
            let mem = instance
                .get_memory(store.as_context_mut(), "memory")
                .ok_or_else(|| wasmtime::Error::msg("no memory export"))?;
            let data = mem.data(store.as_context());
            let output = data
                .get(out_ptr..out_ptr + out_len)
                .and_then(|s| std::str::from_utf8(s).ok())
                .unwrap_or("")
                .to_string();
            Ok(output)
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
    /// Build a WASM module that exports `run(ptr, len) -> i64` (echo) and `alloc`.
    ///
    /// The `run` function packs its input (ptr, len) back as `(ptr << 32) | len`
    /// so the host reads back exactly the input it wrote. Used to test
    /// `execute_with_input` without relying on WAT text parsing.
    fn build_echo_run_module() -> Vec<u8> {
        use wasm_encoder::*;

        let mut module = Module::new();

        // Types: alloc(i32) -> i32, run(i32, i32) -> i64
        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32], vec![ValType::I32]); // type 0: alloc
        types
            .ty()
            .function(vec![ValType::I32, ValType::I32], vec![ValType::I64]); // type 1: run
        module.section(&types);

        // Functions: 0=alloc (type 0), 1=run (type 1)
        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(1);
        module.section(&functions);

        // Memory: 1 page
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Global: $bump = 1024 (mutable i32)
        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(1024),
        );
        module.section(&globals);

        // Exports: alloc, run, memory
        let mut exports = ExportSection::new();
        exports.export("alloc", ExportKind::Func, 0);
        exports.export("run", ExportKind::Func, 1);
        exports.export("memory", ExportKind::Memory, 0);
        module.section(&exports);

        // Code
        let mut code = CodeSection::new();

        // alloc(size) -> ptr: bump allocator
        {
            let mut f = Function::new(vec![(1, ValType::I32)]); // 1 local: $ptr
                                                                // ptr = bump
            f.instruction(&Instruction::GlobalGet(0));
            f.instruction(&Instruction::LocalSet(1));
            // bump += size
            f.instruction(&Instruction::GlobalGet(0));
            f.instruction(&Instruction::LocalGet(0));
            f.instruction(&Instruction::I32Add);
            f.instruction(&Instruction::GlobalSet(0));
            // return ptr
            f.instruction(&Instruction::LocalGet(1));
            f.instruction(&Instruction::End);
            code.function(&f);
        }

        // run(ptr, len) -> i64: echo input back as packed (ptr << 32) | len
        {
            let mut f = Function::new(vec![]);
            // (i64.extend_i32_u ptr) << 32
            f.instruction(&Instruction::LocalGet(0));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::I64Const(32));
            f.instruction(&Instruction::I64Shl);
            // | (i64.extend_i32_u len)
            f.instruction(&Instruction::LocalGet(1));
            f.instruction(&Instruction::I64ExtendI32U);
            f.instruction(&Instruction::I64Or);
            f.instruction(&Instruction::End);
            code.function(&f);
        }

        module.section(&code);
        module.finish()
    }

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
        // Build a module with a non-az import using wasm-encoder
        use wasm_encoder::*;
        let mut module = Module::new();
        let mut types = TypeSection::new();
        types.ty().function(vec![], vec![]);
        module.section(&types);
        let mut imports = ImportSection::new();
        imports.import("env", "abort", EntityType::Function(0));
        module.section(&imports);
        let bytes = module.finish();

        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute(&bytes);
        assert!(result.is_err());
        let err = result.expect_err("should fail");
        assert!(
            err.to_string().contains("undeclared imports"),
            "error should mention undeclared imports: {err}"
        );
    }

    #[test]
    fn wasm_rejects_az_imports_without_callbacks() {
        // Use the Logger template which imports az::log
        let bytes =
            crate::codegen::generate(&crate::codegen::ToolTemplate::Logger).expect("should gen");
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute(&bytes);
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

        // Use the Logger template which imports az::log
        let bytes =
            crate::codegen::generate(&crate::codegen::ToolTemplate::Logger).expect("should gen");
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine should create");
        let result = engine.execute_with_host(&bytes, Arc::new(DenyAllHostCallbacks));
        assert!(result.is_ok(), "should succeed with callbacks: {result:?}");
        let output = result.expect("should succeed");
        assert!(output.success);
    }

    #[test]
    fn wasm_default_config_is_sane() {
        let config = WasmConfig::default();
        assert_eq!(config.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_duration_secs, 30);
        assert!(!config.allow_filesystem);
    }

    #[test]
    fn deny_all_now_returns_valid_iso8601() {
        let cb = DenyAllHostCallbacks;
        let ts = cb.now();
        assert!(
            chrono::DateTime::parse_from_rfc3339(&ts).is_ok(),
            "DenyAllHostCallbacks::now() should return valid ISO 8601: {ts}"
        );
    }

    #[test]
    fn deny_all_rejects_new_operations() {
        let cb = DenyAllHostCallbacks;
        assert!(cb.append_file("x", "y").is_err());
        assert!(cb.list_dir(".").is_err());
        assert!(cb.create_dir("x").is_err());
        assert!(cb.file_exists("x").is_err());
    }

    /// Test end-to-end string return: guest exports `alloc`, calls `az::now()`,
    /// and the host writes the timestamp into guest memory via the alloc protocol.
    #[test]
    fn string_return_via_alloc_now() {
        use std::sync::Arc;

        // WAT module that:
        // 1. Exports an `alloc` function (bump allocator at offset 1024)
        // 2. Calls `az::now()` which returns i64 (packed ptr+len)
        // 3. Returns 0 if the packed value is > 0 (success), 1 if -1 (error)
        let wat = r#"
            (module
                (import "az" "now" (func $now (result i64)))
                (memory (export "memory") 1)
                ;; Simple bump allocator starting at offset 1024
                (global $bump (mut i32) (i32.const 1024))
                (func (export "alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get $size
                    i32.add
                    global.set $bump
                    local.get $ptr)
                (func (export "main") (result i32)
                    (local $result i64)
                    call $now
                    local.set $result
                    ;; If result is -1 (error), return 1
                    local.get $result
                    i64.const -1
                    i64.eq
                    if (result i32)
                        i32.const 1
                    else
                        i32.const 0
                    end))
        "#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_host(
                &wat::parse_str(wat).expect("valid WAT"),
                Arc::new(DenyAllHostCallbacks),
            )
            .expect("should execute");
        assert!(result.success);
        assert!(
            result.output.contains("0"),
            "now() should succeed via alloc: {result:?}"
        );
    }

    /// Test string return with read_file — guest gets actual file content.
    #[test]
    fn string_return_read_file_with_alloc() {
        use std::sync::{Arc, Mutex};

        struct TestCallbacks {
            last_log: Arc<Mutex<String>>,
        }

        impl WasmHostCallbacks for TestCallbacks {
            fn read_file(&self, _path: &str) -> Result<String, String> {
                Ok("hello from host".to_string())
            }
            fn write_file(&self, _: &str, _: &str) -> Result<bool, String> {
                Err("not implemented".into())
            }
            fn append_file(&self, _: &str, _: &str) -> Result<bool, String> {
                Err("not implemented".into())
            }
            fn list_dir(&self, _: &str) -> Result<Vec<String>, String> {
                Err("not implemented".into())
            }
            fn create_dir(&self, _: &str) -> Result<bool, String> {
                Err("not implemented".into())
            }
            fn file_exists(&self, _: &str) -> Result<bool, String> {
                Err("not implemented".into())
            }
            fn log(&self, message: &str) {
                let mut last = self.last_log.lock().expect("lock");
                *last = message.to_string();
            }
            fn now(&self) -> String {
                "2026-05-11T12:00:00-04:00".to_string()
            }
            fn http_request(
                &self,
                _url: &str,
                _method: &str,
                _headers_json: &str,
                _body: &str,
            ) -> Result<String, String> {
                Err("not implemented".into())
            }
        }

        let last_log = Arc::new(Mutex::new(String::new()));
        let cb = Arc::new(TestCallbacks {
            last_log: last_log.clone(),
        });

        // WAT module that:
        // 1. Calls read_file("test") which returns packed ptr/len
        // 2. Unpacks ptr and len from the i64
        // 3. Calls log(ptr, len) to log the received content
        // 4. Returns 0 on success
        let wat = r#"
            (module
                (import "az" "read_file" (func $read_file (param i32 i32) (result i64)))
                (import "az" "log" (func $log (param i32 i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "test")
                (global $bump (mut i32) (i32.const 1024))
                (func (export "alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get $size
                    i32.add
                    global.set $bump
                    local.get $ptr)
                (func (export "main") (result i32)
                    (local $result i64)
                    (local $ptr i32)
                    (local $len i32)
                    ;; Call read_file("test" at offset 0, len 4)
                    i32.const 0
                    i32.const 4
                    call $read_file
                    local.set $result
                    ;; Check for error
                    local.get $result
                    i64.const -1
                    i64.eq
                    if (result i32)
                        i32.const 1
                    else
                        ;; Unpack: ptr = high 32 bits, len = low 32 bits
                        local.get $result
                        i64.const 32
                        i64.shr_u
                        i32.wrap_i64
                        local.set $ptr
                        local.get $result
                        i32.wrap_i64
                        local.set $len
                        ;; Log the content we received
                        local.get $ptr
                        local.get $len
                        call $log
                        i32.const 0
                    end))
        "#;

        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_host(&wat::parse_str(wat).expect("valid WAT"), cb)
            .expect("should execute");
        assert!(result.success);

        let logged = last_log.lock().expect("lock");
        assert_eq!(
            *logged, "hello from host",
            "guest should have received and logged the host string"
        );
    }

    #[test]
    fn wasm_file_exists_host_function() {
        use std::sync::Arc;

        // Module that calls file_exists and returns the result
        let wat = r#"
            (module
                (import "az" "file_exists" (func $file_exists (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "test")
                (func (export "main") (result i32)
                    i32.const 0
                    i32.const 4
                    call $file_exists))
        "#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        // DenyAllHostCallbacks returns Err for file_exists, so we expect -1
        let result = engine
            .execute_with_host(
                &wat::parse_str(wat).expect("valid WAT"),
                Arc::new(DenyAllHostCallbacks),
            )
            .expect("should execute");
        assert!(result.success);
        // The function returns -1 (as i32, wraps to 4294967295 or shows as -1)
        assert!(
            result.output.contains("-1"),
            "file_exists with deny should return -1: {}",
            result.output
        );
    }

    /// Test execute_with_input: guest exports run(ptr, len) -> i64
    /// which echoes the input back as output.
    #[test]
    fn execute_with_input_echo() {
        use std::sync::Arc;

        // Build a WASM module that echoes run(ptr, len) input back as output.
        // Constructed via wasm-encoder to avoid WAT text parsing (wasmtime may
        // not include the `wat` crate in all build configurations).
        let wasm = build_echo_run_module();

        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_input(
                &wasm,
                Arc::new(DenyAllHostCallbacks),
                r#"{"action":"init","root":"/tmp/brain"}"#,
            )
            .expect("should execute with input");
        assert!(result.success);
        assert_eq!(
            result.output, r#"{"action":"init","root":"/tmp/brain"}"#,
            "run() should echo the input back"
        );
    }

    /// Test that execute_with_input falls back to main() when run is not exported.
    #[test]
    fn execute_with_input_fallback_to_main() {
        use std::sync::Arc;

        let wasm = minimal_wasm_module(); // exports main(), not run()
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_input(&wasm, Arc::new(DenyAllHostCallbacks), "ignored input")
            .expect("should fall back to main");
        assert!(result.success);
        assert!(result.output.contains("42"));
    }

    #[test]
    fn wasm_create_dir_host_function() {
        use std::sync::Arc;

        let wat = r#"
            (module
                (import "az" "create_dir" (func $create_dir (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "test")
                (func (export "main") (result i32)
                    i32.const 0
                    i32.const 4
                    call $create_dir))
        "#;
        let engine = WasmEngine::new(WasmConfig::default()).expect("engine");
        let result = engine
            .execute_with_host(
                &wat::parse_str(wat).expect("valid WAT"),
                Arc::new(DenyAllHostCallbacks),
            )
            .expect("should execute");
        assert!(result.success);
        // DenyAll returns 1 (error)
        assert!(result.output.contains("1"));
    }
}
