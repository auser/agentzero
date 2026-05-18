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
        info!(
            host_call = "read_file",
            path = path,
            "WASM guest calling read_file"
        );
        self.executor
            .read_file(path)
            .map(|result| result.output)
            .map_err(|e| {
                warn!(host_call = "read_file", path = path, error = %e, "read_file denied or failed");
                e.to_string()
            })
    }

    fn write_file(&self, path: &str, content: &str) -> Result<bool, String> {
        info!(
            host_call = "write_file",
            path = path,
            bytes = content.len(),
            "WASM guest calling write_file"
        );
        self.executor
            .write_file(path, content)
            .map(|result| result.success)
            .map_err(|e| {
                warn!(host_call = "write_file", path = path, error = %e, "write_file denied or failed");
                e.to_string()
            })
    }

    fn append_file(&self, path: &str, content: &str) -> Result<bool, String> {
        info!(
            host_call = "append_file",
            path = path,
            bytes = content.len(),
            "WASM guest calling append_file"
        );
        self.executor
            .append_file(path, content)
            .map(|result| result.success)
            .map_err(|e| {
                warn!(host_call = "append_file", path = path, error = %e, "append_file denied or failed");
                e.to_string()
            })
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>, String> {
        info!(
            host_call = "list_dir",
            path = path,
            "WASM guest calling list_dir"
        );
        self.executor
            .list_dir(path)
            .map(|result| {
                // ToolExecutor returns "type\tname" lines; extract just names
                result
                    .output
                    .lines()
                    .filter_map(|line| line.split('\t').nth(1))
                    .map(String::from)
                    .collect()
            })
            .map_err(|e| {
                warn!(host_call = "list_dir", path = path, error = %e, "list_dir denied or failed");
                e.to_string()
            })
    }

    fn create_dir(&self, path: &str) -> Result<bool, String> {
        info!(
            host_call = "create_dir",
            path = path,
            "WASM guest calling create_dir"
        );
        self.executor
            .create_dir(path)
            .map(|result| result.success)
            .map_err(|e| {
                warn!(host_call = "create_dir", path = path, error = %e, "create_dir denied or failed");
                e.to_string()
            })
    }

    fn file_exists(&self, path: &str) -> Result<bool, String> {
        info!(
            host_call = "file_exists",
            path = path,
            "WASM guest calling file_exists"
        );
        self.executor
            .file_exists(path)
            .map(|result| result.output == "true")
            .map_err(|e| {
                warn!(host_call = "file_exists", path = path, error = %e, "file_exists denied or failed");
                e.to_string()
            })
    }

    fn log(&self, message: &str) {
        info!(host_call = "log", "WASM guest log: {message}");
    }

    fn now(&self) -> String {
        chrono::Local::now().to_rfc3339()
    }

    fn http_request(
        &self,
        url: &str,
        method: &str,
        _headers_json: &str,
        _body: &str,
    ) -> Result<String, String> {
        info!(
            host_call = "http_request",
            url = url,
            method = method,
            "WASM guest calling http_request"
        );
        // Delegate to ToolExecutor's network request policy check.
        // For now, deny all — full implementation requires the policy engine
        // to check Capability::NetworkRequest and validate the URL against
        // the SandboxNetworkPolicy allowlist.
        warn!(
            host_call = "http_request",
            url = url,
            "http_request not yet wired to ToolExecutor — denied by default"
        );
        Err("http_request: network requests require NetworkRequest capability (not yet implemented in ToolExecutor)".into())
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

    fn callbacks_with_readwrite_allowed() -> SessionHostCallbacks {
        let policy = PolicyEngine::with_rules(vec![
            PolicyRule::allow(Capability::FileRead, DataClassification::Private),
            PolicyRule::allow(Capability::FileWrite, DataClassification::Private),
        ]);
        SessionHostCallbacks::new(ToolExecutor::new(policy))
    }

    #[test]
    fn append_file_denied_by_default() {
        let cb = callbacks_deny_all();
        let result = cb.append_file("/tmp/az-wasm-append.txt", "data");
        assert!(result.is_err());
    }

    #[test]
    fn list_dir_succeeds_with_allowed_policy() {
        let cb = callbacks_with_read_allowed();
        let result = cb.list_dir(".");
        assert!(result.is_ok());
        let entries = result.expect("should list");
        assert!(entries.iter().any(|e| e == "Cargo.toml"));
    }

    #[test]
    fn list_dir_denied_by_default() {
        let cb = callbacks_deny_all();
        let result = cb.list_dir(".");
        assert!(result.is_err());
    }

    #[test]
    fn create_dir_denied_by_default() {
        let cb = callbacks_deny_all();
        let result = cb.create_dir("/tmp/az-wasm-mkdir");
        assert!(result.is_err());
    }

    #[test]
    fn file_exists_succeeds_with_allowed_policy() {
        let cb = callbacks_with_read_allowed();
        let result = cb.file_exists("Cargo.toml");
        assert!(result.is_ok());
        assert!(result.expect("should check"), "Cargo.toml should exist");
    }

    #[test]
    fn file_exists_returns_false_for_missing() {
        let cb = callbacks_with_read_allowed();
        let result = cb.file_exists("nonexistent-xyz.txt");
        assert!(result.is_ok());
        assert!(!result.expect("should check"), "file should not exist");
    }

    #[test]
    fn file_exists_denied_by_default() {
        let cb = callbacks_deny_all();
        let result = cb.file_exists("Cargo.toml");
        assert!(result.is_err());
    }

    #[test]
    fn now_returns_valid_iso8601() {
        let cb = callbacks_deny_all();
        let ts = cb.now();
        // Should parse as RFC 3339 / ISO 8601
        assert!(
            chrono::DateTime::parse_from_rfc3339(&ts).is_ok(),
            "now() should return valid ISO 8601: {ts}"
        );
    }

    #[test]
    fn append_file_creates_and_appends() {
        let cb = callbacks_with_readwrite_allowed();
        let dir = std::env::temp_dir().join("az-wasm-host-append");
        std::fs::create_dir_all(&dir).ok();
        let file = dir.join("test-append.txt");
        std::fs::remove_file(&file).ok();

        let r1 = cb.append_file(file.to_str().expect("path"), "line1\n");
        assert!(r1.is_ok());
        let r2 = cb.append_file(file.to_str().expect("path"), "line2\n");
        assert!(r2.is_ok());

        let content = std::fs::read_to_string(&file).expect("read");
        assert_eq!(content, "line1\nline2\n");

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_dir_and_file_exists() {
        let cb = callbacks_with_readwrite_allowed();
        let dir = std::env::temp_dir().join("az-wasm-host-mkdir-test");
        std::fs::remove_dir_all(&dir).ok();

        let r = cb.create_dir(dir.to_str().expect("path"));
        assert!(r.is_ok());

        let exists = cb.file_exists(dir.to_str().expect("path"));
        assert!(exists.is_ok());
        assert!(exists.expect("should check"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_dir_returns_entry_names_not_paths() {
        let cb = callbacks_with_read_allowed();
        let result = cb.list_dir(".").expect("should list");
        // Should be just names, not full paths
        for entry in &result {
            assert!(
                !entry.contains('/'),
                "entry should be name only, got: {entry}"
            );
        }
    }
}
