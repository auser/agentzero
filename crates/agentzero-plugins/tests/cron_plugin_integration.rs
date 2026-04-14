//! Integration tests for the cron plugin pack (cron_manager + schedule).
//!
//! These tests require:
//! 1. The `wasm-runtime` feature enabled on `agentzero-plugins`
//! 2. The plugins pre-built at `plugins/agentzero-plugin-cron/*/target/wasm32-wasip1/release/*.wasm`

#[cfg(feature = "wasm-runtime")]
mod cron_plugins {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir).join("..").join("..")
    }

    fn cron_manager_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-cron/cron-manager/target/wasm32-wasip1/release/cron_manager_plugin.wasm",
        )
    }

    fn schedule_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-cron/schedule/target/wasm32-wasip1/release/schedule_plugin.wasm",
        )
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
            sanitize_input: false,
            storage_namespace: String::new(),
        }
    }

    fn temp_workspace() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cron-test-{}-{}-{}",
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

    fn make_container(id: &str, wasm_path: PathBuf) -> WasmPluginContainer {
        WasmPluginContainer {
            id: id.to_string(),
            module_path: wasm_path,
            entrypoint: "az_tool_execute".to_string(),
            max_execution_ms: 5_000,
            max_memory_mb: 64,
            allow_network: false,
            allow_fs_write: true,
        }
    }

    fn skip_if_not_built(path: &Path) -> bool {
        if !path.exists() {
            eprintln!("SKIP: plugin not built at {}", path.display());
            return true;
        }
        false
    }

    // ---- cron_manager tests ----
    // These tests require pre-built WASM plugins and are excluded from workspace test runs.
    // Run with: cargo test -p agentzero-plugins -- --ignored

    #[test]
    #[ignore]
    fn cron_manager_add_and_list() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        // Add a task
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"backup","schedule":"0 * * * *","command":"echo backup"}"#,
                &options,
                &fs_policy(),
            )
            .expect("add should work");
        assert!(result.error.is_none(), "add error: {:?}", result.error);
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["status"], "added");
        assert_eq!(output["id"], "backup");

        // List tasks
        let result = runtime
            .execute_v2_with_policy(&container, r#"{"action":"list"}"#, &options, &fs_policy())
            .expect("list should work");
        assert!(result.error.is_none());
        let tasks: Vec<serde_json::Value> = serde_json::from_str(&result.output).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], "backup");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn cron_manager_remove() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        // Add then remove
        runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"temp","schedule":"* * * * *","command":"echo temp"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"remove","id":"temp"}"#,
                &options,
                &fs_policy(),
            )
            .expect("remove should work");
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["status"], "removed");

        // List should be empty
        let result = runtime
            .execute_v2_with_policy(&container, r#"{"action":"list"}"#, &options, &fs_policy())
            .unwrap();
        assert_eq!(result.output, "no cron tasks");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn cron_manager_update() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        // Add then update
        runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"job","schedule":"0 0 * * *","command":"echo old"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"update","id":"job","command":"echo new"}"#,
                &options,
                &fs_policy(),
            )
            .expect("update should work");
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["status"], "updated");
        assert_eq!(output["command"], "echo new");
        assert_eq!(output["schedule"], "0 0 * * *"); // unchanged

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn cron_manager_pause_resume() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"svc","schedule":"*/5 * * * *","command":"echo svc"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        // Pause
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"pause","id":"svc"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["status"], "paused");
        assert_eq!(output["enabled"], false);

        // Resume
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"resume","id":"svc"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["status"], "resumed");
        assert_eq!(output["enabled"], true);

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn cron_manager_duplicate_id() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"dup","schedule":"* * * * *","command":"echo 1"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"add","id":"dup","schedule":"0 * * * *","command":"echo 2"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("already exists"));

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn cron_manager_missing_action() {
        let wasm_path = cron_manager_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("cron_manager", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(&container, "{}", &options, &fs_policy())
            .unwrap();
        assert!(result.error.is_some());
        assert!(result.error.as_deref().unwrap().contains("action"));

        std::fs::remove_dir_all(&ws).ok();
    }

    // ---- schedule plugin tests ----

    #[test]
    #[ignore]
    fn schedule_parse_every_5_minutes() {
        let wasm_path = schedule_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("schedule", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"parse","schedule":"every 5 minutes"}"#,
                &options,
                &fs_policy(),
            )
            .expect("parse should work");
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["cron_expression"], "*/5 * * * *");
        assert_eq!(output["is_natural_language"], true);

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn schedule_parse_daily_at_9am() {
        let wasm_path = schedule_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("schedule", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"parse","schedule":"daily at 9am"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["cron_expression"], "0 9 * * *");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn schedule_parse_weekly_on_monday() {
        let wasm_path = schedule_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("schedule", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"parse","schedule":"weekly on monday"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["cron_expression"], "0 0 * * 1");

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn schedule_parse_passthrough_cron() {
        let wasm_path = schedule_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("schedule", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"parse","schedule":"*/10 * * * *"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["cron_expression"], "*/10 * * * *");
        assert_eq!(output["is_natural_language"], false);

        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    #[ignore]
    fn schedule_create_delegates() {
        let wasm_path = schedule_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let ws = temp_workspace();
        let runtime = WasmPluginRuntime::new();
        let container = make_container("schedule", wasm_path);
        let options = WasmV2Options {
            workspace_root: ws.to_string_lossy().to_string(),
            capabilities: vec![],
        };

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"action":"create","id":"daily-backup","schedule":"every day at 2:30pm","command":"backup.sh"}"#,
                &options,
                &fs_policy(),
            )
            .unwrap();
        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["delegation"], "cron_manager");
        assert_eq!(output["prepared_input"]["action"], "add");
        assert_eq!(output["prepared_input"]["schedule"], "30 14 * * *");

        std::fs::remove_dir_all(&ws).ok();
    }
}
