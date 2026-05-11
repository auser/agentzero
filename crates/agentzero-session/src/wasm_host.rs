//! WasmHostCallbacks implementation backed by ToolExecutor + PolicyEngine.
//!
//! Bridges the WASM sandbox's host callback trait to the session's
//! policy-checked tool executor. Every host call goes through the
//! same policy and audit pipeline as built-in tools (ADR 0003).

use agentzero_sandbox::wasm::WasmHostCallbacks;
use agentzero_tracing::{info, warn};

use crate::tool_exec::ToolExecutor;

/// Host callbacks backed by a `ToolExecutor` with policy enforcement.
///
/// Each host function delegates to the corresponding `ToolExecutor` method,
/// which validates paths, checks policy, and emits audit events.
pub struct SessionHostCallbacks {
    executor: ToolExecutor,
}

impl SessionHostCallbacks {
    /// Create callbacks backed by the given tool executor.
    pub fn new(executor: ToolExecutor) -> Self {
        Self { executor }
    }
}

impl WasmHostCallbacks for SessionHostCallbacks {
    fn read_file(&self, path: &str) -> Result<String, String> {
        info!(host_call = "read_file", path = path, "WASM guest calling read_file");
        self.executor
            .read_file(path)
            .map(|result| result.output)
            .map_err(|e| {
                warn!(host_call = "read_file", path = path, error = %e, "read_file denied or failed");
                e.to_string()
            })
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        info!(host_call = "write_file", path = path, bytes = content.len(), "WASM guest calling write_file");
        self.executor
            .write_file(path, content)
            .map(|result| result.success)
            .map_err(|e| {
                warn!(host_call = "write_file", path = path, error = %e, "write_file denied or failed");
                e.to_string()
            })
    }

    fn log(&self, message: &str) {
        info!(host_call = "log", "WASM guest log: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{Capability, DataClassification};
    use agentzero_policy::{PolicyEngine, PolicyRule};

    fn callbacks_with_read_allowed() -> SessionHostCallbacks {
        let policy = PolicyEngine::with_rules(vec![PolicyRule::allow(
            Capability::FileRead,
            DataClassification::Private,
        )]);
        SessionHostCallbacks::new(ToolExecutor::new(policy))
    }

    fn callbacks_deny_all() -> SessionHostCallbacks {
        SessionHostCallbacks::new(ToolExecutor::new(PolicyEngine::deny_by_default()))
    }

    #[test]
    fn read_file_succeeds_with_allowed_policy() {
        let cb = callbacks_with_read_allowed();
        let result = cb.read_file("Cargo.toml");
        assert!(result.is_ok());
        assert!(result.expect("should read").contains("[package]"));
    }

    #[test]
    fn read_file_denied_by_policy() {
        let cb = callbacks_deny_all();
        let result = cb.read_file("Cargo.toml");
        assert!(result.is_err());
        assert!(result.expect_err("should deny").contains("denied"));
    }

    #[test]
    fn read_file_blocks_sensitive_paths() {
        let cb = callbacks_with_read_allowed();
        // .agentzero/ should be blocked
        let dir = std::env::temp_dir().join("az-wasm-host-test");
        let az_dir = dir.join(".agentzero");
        std::fs::create_dir_all(&az_dir).ok();
        let file = az_dir.join("policy.yml");
        std::fs::write(&file, "version = 1").ok();

        let result = cb.read_file(file.to_str().expect("path"));
        assert!(result.is_err());

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_file_denied_by_default_policy() {
        let cb = callbacks_deny_all();
        let result = cb.write_file("/tmp/test-wasm-write.txt", "hello");
        assert!(result.is_err());
    }

    #[test]
    fn log_does_not_panic() {
        let cb = callbacks_deny_all();
        cb.log("test message from WASM guest");
        // Just verify no panic
    }
}
