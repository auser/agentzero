//! Integration tests for the reference notepad plugin.
//!
//! Requires:
//! 1. The `wasm-runtime` feature enabled on `agentzero-plugins`
//! 2. The plugin pre-built:
//!    cd plugins/agentzero-plugin-reference/notepad
//!    cargo build --target wasm32-wasip1 --release

#[cfg(feature = "wasm-runtime")]
mod notepad_plugin {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn notepad_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-reference/notepad/\
             target/wasm32-wasip1/release/notepad_plugin.wasm",
        )
    }

    fn skip_if_not_built(path: &Path) -> bool {
        if !path.exists() {
            eprintln!(
                "SKIP: plugin not built at {}. \
                 Build with: cd plugins/agentzero-plugin-reference/notepad && \
                 cargo build --target wasm32-wasip1 --release",
                path.display()
            );
            return true;
        }
        false
    }

    fn fs_policy() -> WasmIsolationPolicy {
        WasmIsolationPolicy {
            max_execution_ms: 5_000,
            max_module_bytes: 5 * 1024 * 1024,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: true,
            allow_fs_read: true,
            allowed_host_calls: vec!["az_log".to_string()],
            require_signed: false,
            allowed_host_tools: Vec::new(),
            overlay_mode: agentzero_plugins::overlay::OverlayMode::default(),
        }
    }

    fn temp_workspace() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-notepad-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            count
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_container(wasm_path: PathBuf) -> WasmPluginContainer {
        WasmPluginContainer {
            id: "notepad".to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: true,
        }
    }

    #[test]
    #[ignore]
    fn write_and_read_roundtrip() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        // Write
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"write","note_id":"hello","content":"Hello, world!"}"#,
                &options,
                &fs_policy(),
            )
            .expect("write should succeed");
        assert!(result.error.is_none(), "write error: {:?}", result.error);
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["status"], "written");
        assert_eq!(v["note_id"], "hello");
        assert_eq!(v["bytes"], 13);

        // Read back
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"read","note_id":"hello"}"#,
                &options,
                &fs_policy(),
            )
            .expect("read should succeed");
        assert!(result.error.is_none());
        assert_eq!(result.output, "Hello, world!");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn list_empty_and_populated() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        // List before any notes
        let result = runtime
            .execute_v2_with_policy(&container, r#"{"action":"list"}"#, &options, &fs_policy())
            .unwrap();
        assert!(result.error.is_none());
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["notes"], serde_json::json!([]));

        // Write two notes then list
        for id in &["beta", "alpha"] {
            runtime
                .execute_v2_with_policy(
                    &container,
                    &format!(r#"{{"action":"write","note_id":"{id}","content":"note {id}"}}"#),
                    &options,
                    &fs_policy(),
                )
                .unwrap();
        }

        let result = runtime
            .execute_v2_with_policy(&container, r#"{"action":"list"}"#, &options, &fs_policy())
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let notes: Vec<&str> = v["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        // Sorted alphabetically
        assert_eq!(notes, vec!["alpha", "beta"]);

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn delete_existing_note() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"write","note_id":"temp","content":"bye"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"delete","note_id":"temp"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none(), "clean delete should have no error");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["status"], "deleted");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn delete_nonexistent_returns_warning() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"delete","note_id":"ghost"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        // with_warning: output is non-empty AND error is set (as the warning)
        assert!(
            !result.output.is_empty(),
            "output should describe not_found state"
        );
        assert!(
            result.error.is_some(),
            "warning should be present for missing note"
        );
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["status"], "not_found");
        assert!(result.error.as_deref().unwrap().contains("ghost"));

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn read_missing_note_returns_error() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"read","note_id":"no-such-note"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_some());
        assert!(result.output.is_empty());

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn path_traversal_rejected() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        for bad_id in &["../escape", "sub/dir", "a\\b"] {
            let input = format!(r#"{{"action":"read","note_id":"{bad_id}"}}"#);
            let result = runtime
                .execute_v2_with_policy(&container, &input, &options, &fs_policy())
                .unwrap();
            assert!(
                result.error.is_some(),
                "bad id '{bad_id}' should be rejected"
            );
        }

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn unknown_action_returns_error() {
        let wasm = notepad_wasm();
        if skip_if_not_built(&wasm) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container(wasm);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"frobnicate"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("frobnicate"));

        std::fs::remove_dir_all(&ws).ok();
    }
}
