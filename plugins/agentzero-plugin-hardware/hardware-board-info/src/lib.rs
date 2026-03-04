use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("hardware_board_info", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let board_id = req.get("board").and_then(|v| v.as_str());

    match board_id {
        Some(id) => {
            let id = id.trim();
            if id.is_empty() {
                return ToolOutput::error("board id must not be empty");
            }
            match find_board(id) {
                Some(b) => ToolOutput::success(
                    json!({
                        "id": b.id,
                        "display_name": b.display_name,
                        "architecture": b.architecture,
                        "memory_kb": b.memory_kb,
                    })
                    .to_string(),
                ),
                None => ToolOutput::error(format!("unknown hardware board id: {id}")),
            }
        }
        None => {
            let boards: Vec<serde_json::Value> = BOARDS
                .iter()
                .map(|b| {
                    json!({
                        "id": b.id,
                        "display_name": b.display_name,
                        "architecture": b.architecture,
                        "memory_kb": b.memory_kb,
                    })
                })
                .collect();
            ToolOutput::success(serde_json::to_string_pretty(&boards).unwrap_or_default())
        }
    }
}

struct Board {
    id: &'static str,
    display_name: &'static str,
    architecture: &'static str,
    memory_kb: u32,
}

static BOARDS: &[Board] = &[
    Board {
        id: "sim-stm32",
        display_name: "Simulated STM32 Board",
        architecture: "arm-cortex-m",
        memory_kb: 256,
    },
    Board {
        id: "sim-rpi",
        display_name: "Simulated Raspberry Pi",
        architecture: "arm64",
        memory_kb: 1_048_576,
    },
];

fn find_board(id: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|b| b.id == id)
}
