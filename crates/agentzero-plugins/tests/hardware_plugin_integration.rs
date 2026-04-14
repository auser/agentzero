//! Integration tests for the hardware plugin pack (3 WASM plugins).
//!
//! These tests require:
//! 1. The `wasm-runtime` feature enabled on `agentzero-plugins`
//! 2. The hardware plugins pre-built at `plugins/agentzero-plugin-hardware/*/target/wasm32-wasip1/release/*.wasm`
//!
//! Build all hardware plugins with:
//!   for d in plugins/agentzero-plugin-hardware/hardware-*; do
//!     (cd "$d" && RUSTC=$(rustup which rustc) cargo build --target wasm32-wasip1 --release)
//!   done

#[cfg(feature = "wasm-runtime")]
mod hardware_plugins {
    use agentzero_plugins::wasm::{
        WasmIsolationPolicy, WasmPluginContainer, WasmPluginRuntime, WasmV2Options,
    };
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest_dir).join("..").join("..")
    }

    fn board_info_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-hardware/hardware-board-info/target/wasm32-wasip1/release/hardware_board_info.wasm",
        )
    }

    fn memory_map_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-hardware/hardware-memory-map/target/wasm32-wasip1/release/hardware_memory_map.wasm",
        )
    }

    fn memory_read_wasm() -> PathBuf {
        workspace_root().join(
            "plugins/agentzero-plugin-hardware/hardware-memory-read/target/wasm32-wasip1/release/hardware_memory_read.wasm",
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
            require_signed: false,
            allowed_host_tools: Vec::new(),
            overlay_mode: agentzero_plugins::overlay::OverlayMode::default(),
            sanitize_input: false,
            storage_namespace: String::new(),
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
            eprintln!(
                "SKIP: hardware plugin not built at {}. Build with:\n\
                 cd {} && RUSTC=$(rustup which rustc) cargo build --target wasm32-wasip1 --release",
                path.display(),
                path.parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            return true;
        }
        false
    }

    // ---- hardware_board_info tests ----

    #[test]
    #[ignore]
    fn board_info_list_all() {
        let wasm_path = board_info_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_board_info", wasm_path);

        let result = runtime
            .execute_v2_with_policy(&container, "{}", &default_options(), &default_policy())
            .expect("board_info should execute");

        assert!(
            result.error.is_none(),
            "should not error: {:?}",
            result.error
        );

        let boards: serde_json::Value =
            serde_json::from_str(&result.output).expect("output should be valid JSON");
        let arr = boards.as_array().expect("output should be an array");
        assert_eq!(arr.len(), 2, "should have 2 boards");

        let ids: Vec<&str> = arr
            .iter()
            .filter_map(|b| b.get("id").and_then(|v| v.as_str()))
            .collect();
        assert!(ids.contains(&"sim-stm32"), "should contain sim-stm32");
        assert!(ids.contains(&"sim-rpi"), "should contain sim-rpi");
    }

    #[test]
    #[ignore]
    fn board_info_query_specific() {
        let wasm_path = board_info_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_board_info", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-stm32"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("board_info should execute");

        assert!(result.error.is_none());
        let info: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(info["id"], "sim-stm32");
        assert_eq!(info["architecture"], "arm-cortex-m");
        assert_eq!(info["memory_kb"], 256);
    }

    #[test]
    #[ignore]
    fn board_info_unknown_board() {
        let wasm_path = board_info_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_board_info", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"nonexistent"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(
            result.error.is_some(),
            "should have error for unknown board"
        );
        assert!(
            result
                .error
                .as_deref()
                .unwrap()
                .contains("unknown hardware board id"),
            "error should mention unknown board"
        );
    }

    // ---- hardware_memory_map tests ----

    #[test]
    #[ignore]
    fn memory_map_stm32() {
        let wasm_path = memory_map_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_map", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-stm32"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("memory_map should execute");

        assert!(result.error.is_none());
        let map: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(map["board"], "sim-stm32");

        let regions = map["regions"].as_array().expect("should have regions");
        assert_eq!(regions.len(), 3, "stm32 has flash + sram + peripherals");

        let names: Vec<&str> = regions
            .iter()
            .filter_map(|r| r.get("name").and_then(|v| v.as_str()))
            .collect();
        assert!(names.contains(&"flash"));
        assert!(names.contains(&"sram"));
        assert!(names.contains(&"peripherals"));
    }

    #[test]
    #[ignore]
    fn memory_map_rpi() {
        let wasm_path = memory_map_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_map", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-rpi"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("memory_map should execute");

        assert!(result.error.is_none());
        let map: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(map["board"], "sim-rpi");

        let regions = map["regions"].as_array().expect("should have regions");
        assert_eq!(regions.len(), 3, "rpi has sdram + peripherals + gpu_memory");
    }

    #[test]
    #[ignore]
    fn memory_map_missing_board() {
        let wasm_path = memory_map_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_map", wasm_path);

        let result = runtime
            .execute_v2_with_policy(&container, "{}", &default_options(), &default_policy())
            .expect("should not crash");

        assert!(result.error.is_some(), "should error when board is missing");
        assert!(result.error.as_deref().unwrap().contains("board"));
    }

    // ---- hardware_memory_read tests ----

    #[test]
    #[ignore]
    fn memory_read_basic() {
        let wasm_path = memory_read_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_read", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-stm32","address":"0x08000000","length":32}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("memory_read should execute");

        assert!(
            result.error.is_none(),
            "should not error: {:?}",
            result.error
        );
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["board"], "sim-stm32");
        assert_eq!(output["address"], "0x08000000");
        assert_eq!(output["length"], 32);
        assert_eq!(output["mode"], "simulated");
        assert!(
            output["hex_dump"].as_str().is_some(),
            "should have hex_dump"
        );
    }

    #[test]
    #[ignore]
    fn memory_read_default_length() {
        let wasm_path = memory_read_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_read", wasm_path);

        // Omit length — should default to 64
        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-rpi","address":"0x00000000"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("memory_read should execute");

        assert!(result.error.is_none());
        let output: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(output["length"], 64, "default length should be 64");
    }

    #[test]
    #[ignore]
    fn memory_read_invalid_address() {
        let wasm_path = memory_read_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_read", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"sim-stm32","address":"not_hex"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some(), "should error on invalid hex");
        assert!(
            result.error.as_deref().unwrap().contains("invalid hex"),
            "error should mention invalid hex"
        );
    }

    #[test]
    #[ignore]
    fn memory_read_unknown_board() {
        let wasm_path = memory_read_wasm();
        if skip_if_not_built(&wasm_path) {
            return;
        }

        let runtime = WasmPluginRuntime::new();
        let container = make_container("hardware_memory_read", wasm_path);

        let result = runtime
            .execute_v2_with_policy(
                &container,
                r#"{"board":"nonexistent","address":"0x0"}"#,
                &default_options(),
                &default_policy(),
            )
            .expect("should not crash");

        assert!(result.error.is_some());
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("unknown hardware board id"));
    }
}
