//! Integration tests for the integrations plugin pack (composio, pushover).
//!
//! These tests require:
//! 1. The `wasm-runtime` feature enabled on `agentzero-plugins`
//! 2. The plugins pre-built at `plugins/agentzero-plugin-integrations/*/target/wasm32-wasip1/release/*.wasm`

#[cfg(feature = "wasm-runtime")]
mod integrations_plugins {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir).join("..").join("..")
    }

    fn composio_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-integrations/composio/target/wasm32-wasip1/release/composio_plugin.wasm",
        )
    }

    fn pushover_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-integrations/pushover/target/wasm32-wasip1/release/pushover_plugin.wasm",
        )
    }

    fn default_policy() -> WasmIsolationPolicy {
        WasmIsolationPolicy {
            max_execution_ms: 5_000,
            max_module_bytes: 5 * 1024 * 1024,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
            allow_fs_read: false,
            allowed_host_calls: vec!["az_log".to_string()],
        }
    }

    fn default_options() -> WasmV2Options {
        WasmV2Options {
            workspace_root: "/tmp/test-workspace".to_string(),
            capabilities: vec![],
        }
    }

    fn make_container(id: &str, wasm_path: PathBuf) -> WasmPluginContainer {
        WasmPluginContainer {
            id: id.to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        }
    }

    fn skip_if_not_built(path: &Path) -> bool {
        if !path.exists() {
            eprintln!("SKIP: plugin not built at {}", path.display());
            return true;
        }
        false
    }

    // ---- composio tests ----

    #[test]
    fn composio_valid_request() {
        let wasm_path = composio_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("composio", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"github.create_issue","params":{"repo":"test/repo","title":"Bug"},"api_key":"test-key-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("composio should execute");

        assert!(
            result.error.is_none(),
            "should not error: {:?}",
            result.error
        );
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["mode"], "dry-run");
        assert_eq!(
            output["endpoint"],
            "https://backend.composio.dev/api/v1/actions/execute"
        );
        assert_eq!(output["method"], "POST");
        assert_eq!(output["body"]["action"], "github.create_issue");
    }

    #[test]
    fn composio_missing_action() {
        let wasm_path = composio_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("composio", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"api_key":"test-key-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("action"));
    }

    #[test]
    fn composio_missing_api_key() {
        let wasm_path = composio_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("composio", wasm_path);

        // No api_key and no COMPOSIO_API_KEY env var
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"test_action"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("API key"));
    }

    // ---- pushover tests ----

    #[test]
    fn pushover_valid_request() {
        let wasm_path = pushover_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("pushover", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"message":"Hello world","title":"Test","priority":1,"token":"test-token-1234567890","user":"test-user-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("pushover should execute");

        assert!(
            result.error.is_none(),
            "should not error: {:?}",
            result.error
        );
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["mode"], "dry-run");
        assert_eq!(
            output["endpoint"],
            "https://api.pushover.net/1/messages.json"
        );
        assert_eq!(output["method"], "POST");
        assert_eq!(output["form_data"]["message"], "Hello world");
        assert_eq!(output["form_data"]["priority"], "1");
    }

    #[test]
    fn pushover_missing_message() {
        let wasm_path = pushover_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("pushover", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"token":"test-token-1234567890","user":"test-user-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("message"));
    }

    #[test]
    fn pushover_invalid_priority() {
        let wasm_path = pushover_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("pushover", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"message":"test","priority":5,"token":"test-token-1234567890","user":"test-user-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("priority"));
    }

    #[test]
    fn pushover_missing_token() {
        let wasm_path = pushover_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("pushover", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"message":"test","user":"test-user-1234567890"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("token"));
    }
}
