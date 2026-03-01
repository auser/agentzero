use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmIsolationPolicy {
    pub max_execution_ms: u64,
    pub max_module_bytes: u64,
    pub max_memory_mb: u32,
    pub allow_network: bool,
    pub allow_fs_write: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WasmExecutionRequest {
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WasmExecutionResult {
    pub status_code: i32,
}

impl WasmPluginRuntime {
    pub fn new() -> Self {
        Self
    }

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
                let timed_out = started.elapsed() >= Duration::from_millis(effective_timeout_ms);
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

impl Default for WasmPluginRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WasmExecutionRequest, WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime,
    };
    use serde_json::json;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("disallowed-import-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("allowlisted-import-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("oversized-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("execute-ok-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("execute-missing-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("memory-limit-{unique}.wasm"));
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
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("timeout-{unique}.wasm"));
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
}
