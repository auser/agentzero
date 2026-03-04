use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("hardware_memory_map", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let board = match req.get("board").and_then(|v| v.as_str()) {
        Some(b) => b.trim(),
        None => return ToolOutput::error("board field is required"),
    };

    if board.is_empty() {
        return ToolOutput::error("board must not be empty");
    }

    // Validate board exists
    if !is_known_board(board) {
        return ToolOutput::error(format!("unknown hardware board id: {board}"));
    }

    let map = memory_map_for(board);
    ToolOutput::success(serde_json::to_string_pretty(&map).unwrap_or_default())
}

fn is_known_board(id: &str) -> bool {
    matches!(id, "sim-stm32" | "sim-rpi")
}

fn memory_map_for(board_id: &str) -> serde_json::Value {
    match board_id {
        "sim-stm32" => json!({
            "board": "sim-stm32",
            "regions": [
                {"name": "flash", "start": "0x08000000", "end": "0x0803FFFF", "size_kb": 256, "access": "rx"},
                {"name": "sram", "start": "0x20000000", "end": "0x2000FFFF", "size_kb": 64, "access": "rwx"},
                {"name": "peripherals", "start": "0x40000000", "end": "0x5FFFFFFF", "size_kb": null, "access": "rw"},
            ]
        }),
        "sim-rpi" => json!({
            "board": "sim-rpi",
            "regions": [
                {"name": "sdram", "start": "0x00000000", "end": "0x3FFFFFFF", "size_kb": 1048576, "access": "rwx"},
                {"name": "peripherals", "start": "0xFE000000", "end": "0xFEFFFFFF", "size_kb": null, "access": "rw"},
                {"name": "gpu_memory", "start": "0xC0000000", "end": "0xFFFFFFFF", "size_kb": null, "access": "rw"},
            ]
        }),
        _ => json!({
            "board": board_id,
            "regions": [],
            "note": "no memory map available for this board"
        }),
    }
}
