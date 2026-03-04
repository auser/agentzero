use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("hardware_memory_read", execute);

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
    if !is_known_board(board) {
        return ToolOutput::error(format!("unknown hardware board id: {board}"));
    }

    let address_str = match req.get("address").and_then(|v| v.as_str()) {
        Some(a) => a.trim(),
        None => return ToolOutput::error("address field is required"),
    };
    if address_str.is_empty() {
        return ToolOutput::error("address must not be empty");
    }

    let addr_hex = address_str
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let address = match u64::from_str_radix(addr_hex, 16) {
        Ok(a) => a,
        Err(_) => return ToolOutput::error(format!("invalid hex address: {address_str}")),
    };

    let length = req
        .get("length")
        .and_then(|v| v.as_u64())
        .unwrap_or(64)
        .clamp(1, 256) as usize;

    // Simulated read: deterministic data based on address
    let data: Vec<u8> = (0..length)
        .map(|i| ((address.wrapping_add(i as u64)) & 0xFF) as u8)
        .collect();

    let hex_dump = format_hex_dump(address, &data);

    ToolOutput::success(
        json!({
            "board": board,
            "address": format!("0x{:08X}", address),
            "length": length,
            "mode": "simulated",
            "hex_dump": hex_dump,
        })
        .to_string(),
    )
}

fn is_known_board(id: &str) -> bool {
    matches!(id, "sim-stm32" | "sim-rpi")
}

fn format_hex_dump(base_addr: u64, data: &[u8]) -> String {
    let mut lines = Vec::new();
    for (i, chunk) in data.chunks(16).enumerate() {
        let addr = base_addr + (i * 16) as u64;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(format!("{addr:08X}  {:<48}  |{ascii}|", hex.join(" ")));
    }
    lines.join("\n")
}
