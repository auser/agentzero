//! Integration test: build a real SDK plugin → load via runtime → execute.
//!
//! This test requires:
//! 1. The `wasm-runtime` feature enabled on `agentzero-plugins`
//! 2. The sample plugin pre-built at `tests/fixtures/sample-plugin/target/wasm32-wasip1/release/sample_plugin.wasm`
//!
//! The sample plugin is built during development with:
//!   cd crates/agentzero-plugins/tests/fixtures/sample-plugin
//!   RUSTC=$(rustup which rustc) cargo build --target wasm32-wasip1 --release

#[cfg(feature = "wasm-runtime")]
mod sdk_integration {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::path::PathBuf;

    fn sample_wasm_path() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir)
            .join("tests/fixtures/sample-plugin/target/wasm32-wasip1/release/sample_plugin.wasm")
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

    #[test]
    #[ignore]
    fn sdk_plugin_executes_successfully() {
        let wasm_path = sample_wasm_path();
        if !wasm_path.exists() {
            eprintln!(
                "SKIP: sample plugin not built at {}. Build with:\n\
                 cd crates/agentzero-plugins/tests/fixtures/sample-plugin\n\
                 RUSTC=$(rustup which rustc) cargo build --target wasm32-wasip1 --release",
                wasm_path.display()
            );
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "sample_plugin".to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = default_policy();
        let options = WasmV2Options {
            workspace_root: "/tmp/test-workspace".to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(&container, r#"{"name":"AgentZero"}"#, &options, &policy)
            .expect("SDK plugin execution should succeed");

        assert!(
            result.output.contains("Hello, AgentZero!"),
            "output should contain greeting, got: {}",
            result.output
        );
        assert!(
            result.output.contains("workspace=/tmp/test-workspace"),
            "output should contain workspace root, got: {}",
            result.output
        );
        assert!(result.error.is_none(), "should have no error");
    }

    #[test]
    #[ignore]
    fn sdk_plugin_handles_empty_input() {
        let wasm_path = sample_wasm_path();
        if !wasm_path.exists() {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "sample_plugin".to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = default_policy();
        let options = WasmV2Options {
            workspace_root: "/tmp".to_string(),
            capabilities: vec![],
        };

        // Pass empty JSON object — plugin should default to "world"
        let result = runtime
            .execute_v2_with_policy(&container, "{}", &options, &policy)
            .expect("SDK plugin should handle empty input");

        assert!(
            result.output.contains("Hello, world!"),
            "should use default name, got: {}",
            result.output
        );
    }

    #[test]
    #[ignore]
    fn sdk_plugin_handles_invalid_json_input() {
        let wasm_path = sample_wasm_path();
        if !wasm_path.exists() {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = WasmPluginContainer {
            id: "sample_plugin".to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: false,
        };
        let policy = default_policy();
        let options = WasmV2Options {
            workspace_root: "/tmp".to_string(),
            capabilities: vec![],
        };

        // Pass non-JSON string — plugin's handler returns error via ToolOutput
        let result = runtime
            .execute_v2_with_policy(&container, "not json at all", &options, &policy)
            .expect("SDK plugin should not crash on invalid input");

        assert!(
            result.error.is_some(),
            "should have error for invalid input, got output: {}",
            result.output
        );
    }
}
