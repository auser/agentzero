use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

// --- hardware_board_info ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct BoardInfoInput {
    /// Optional board ID to get detailed info for. Omit to list all boards.
    #[serde(default)]
    board: Option<String>,
}

/// Query connected board information.
///
/// Operations:
/// - With no `board`: list all discovered boards
/// - With `board`: get detailed info for a specific board ID
#[tool(
    name = "hardware_board_info",
    description = "List discovered hardware boards or get detailed info for a specific board."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct HardwareBoardInfoTool;

#[async_trait]
impl Tool for HardwareBoardInfoTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(BoardInfoInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: BoardInfoInput = serde_json::from_str(input)
            .context("hardware_board_info expects JSON: {\"board\"?}")?;

        match req.board {
            Some(id) => {
                if id.trim().is_empty() {
                    return Err(anyhow!("board id must not be empty"));
                }
                let board = crate::hardware::board_info(&id)?;
                let output = json!({
                    "id": board.id,
                    "display_name": board.display_name,
                    "architecture": board.architecture,
                    "memory_kb": board.memory_kb,
                })
                .to_string();
                Ok(ToolResult { output })
            }
            None => {
                let boards = crate::hardware::discover_boards();
                let entries: Vec<serde_json::Value> = boards
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
                Ok(ToolResult {
                    output: serde_json::to_string_pretty(&entries)
                        .unwrap_or_else(|_| "[]".to_string()),
                })
            }
        }
    }
}

// --- hardware_memory_map ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MemoryMapInput {
    /// The board ID to get the memory map for
    board: String,
}

/// Read hardware memory map layout for a board.
///
/// Returns flash and RAM address ranges based on known datasheets.
#[tool(
    name = "hardware_memory_map",
    description = "Get the flash and RAM memory map layout for a hardware board."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct HardwareMemoryMapTool;

#[async_trait]
impl Tool for HardwareMemoryMapTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MemoryMapInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MemoryMapInput =
            serde_json::from_str(input).context("hardware_memory_map expects JSON: {\"board\"}")?;

        if req.board.trim().is_empty() {
            return Err(anyhow!("board must not be empty"));
        }

        // Validate the board exists
        crate::hardware::board_info(&req.board)?;

        let map = memory_map_for(&req.board);
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".to_string()),
        })
    }
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

// --- hardware_memory_read ---

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct MemoryReadInput {
    /// The board ID to read memory from
    board: String,
    /// Hex address to read (e.g. 0x20000000)
    address: String,
    /// Number of bytes to read (1-256, default 64)
    #[serde(default = "default_read_length")]
    length: usize,
}

fn default_read_length() -> usize {
    64
}

/// Read hardware memory at a given address.
///
/// In simulation mode, returns representative data for the requested region.
/// With real hardware (future), reads via debug probe.
#[tool(
    name = "hardware_memory_read",
    description = "Read memory from a hardware board at a given address."
)]
#[derive(Debug, Default, Clone, Copy)]
pub struct HardwareMemoryReadTool;

#[async_trait]
impl Tool for HardwareMemoryReadTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(MemoryReadInput::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: MemoryReadInput = serde_json::from_str(input)
            .context("hardware_memory_read expects JSON: {\"board\", \"address\", \"length\"?}")?;

        if req.board.trim().is_empty() {
            return Err(anyhow!("board must not be empty"));
        }
        if req.address.trim().is_empty() {
            return Err(anyhow!("address must not be empty"));
        }

        // Validate board exists
        crate::hardware::board_info(&req.board)?;

        // Parse hex address
        let addr_str = req
            .address
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let address = u64::from_str_radix(addr_str, 16)
            .map_err(|_| anyhow!("invalid hex address: {}", req.address))?;

        let length = req.length.clamp(1, 256);

        // Simulated read: produce deterministic data based on address
        let data: Vec<u8> = (0..length)
            .map(|i| ((address.wrapping_add(i as u64)) & 0xFF) as u8)
            .collect();

        let hex_dump = format_hex_dump(address, &data);

        let output = json!({
            "board": req.board,
            "address": format!("0x{:08X}", address),
            "length": length,
            "mode": "simulated",
            "hex_dump": hex_dump,
        })
        .to_string();

        Ok(ToolResult { output })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    // --- board info tests ---

    #[tokio::test]
    async fn board_info_list_all() {
        let tool = HardwareBoardInfoTool;
        let result = tool
            .execute(r#"{}"#, &test_ctx())
            .await
            .expect("list should succeed");
        assert!(result.output.contains("sim-stm32"));
        assert!(result.output.contains("sim-rpi"));
    }

    #[tokio::test]
    async fn board_info_specific_board() {
        let tool = HardwareBoardInfoTool;
        let result = tool
            .execute(r#"{"board": "sim-stm32"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["id"], "sim-stm32");
        assert_eq!(v["architecture"], "arm-cortex-m");
    }

    #[tokio::test]
    async fn board_info_unknown_board_fails() {
        let tool = HardwareBoardInfoTool;
        let err = tool
            .execute(r#"{"board": "nonexistent"}"#, &test_ctx())
            .await
            .expect_err("unknown board should fail");
        assert!(err.to_string().contains("unknown hardware board"));
    }

    // --- memory map tests ---

    #[tokio::test]
    async fn memory_map_stm32() {
        let tool = HardwareMemoryMapTool;
        let result = tool
            .execute(r#"{"board": "sim-stm32"}"#, &test_ctx())
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["board"], "sim-stm32");
        let regions = v["regions"].as_array().unwrap();
        assert!(!regions.is_empty());
        assert!(regions.iter().any(|r| r["name"] == "flash"));
        assert!(regions.iter().any(|r| r["name"] == "sram"));
    }

    #[tokio::test]
    async fn memory_map_unknown_board_fails() {
        let tool = HardwareMemoryMapTool;
        let err = tool
            .execute(r#"{"board": "no-such-board"}"#, &test_ctx())
            .await
            .expect_err("unknown board should fail");
        assert!(err.to_string().contains("unknown hardware board"));
    }

    #[tokio::test]
    async fn memory_map_empty_board_fails() {
        let tool = HardwareMemoryMapTool;
        let err = tool
            .execute(r#"{"board": ""}"#, &test_ctx())
            .await
            .expect_err("empty board should fail");
        assert!(err.to_string().contains("board must not be empty"));
    }

    // --- memory read tests ---

    #[tokio::test]
    async fn memory_read_success() {
        let tool = HardwareMemoryReadTool;
        let result = tool
            .execute(
                r#"{"board": "sim-stm32", "address": "0x20000000", "length": 32}"#,
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["board"], "sim-stm32");
        assert_eq!(v["mode"], "simulated");
        assert_eq!(v["length"], 32);
        assert!(v["hex_dump"].as_str().unwrap().contains("20000000"));
    }

    #[tokio::test]
    async fn memory_read_invalid_hex_fails() {
        let tool = HardwareMemoryReadTool;
        let err = tool
            .execute(
                r#"{"board": "sim-stm32", "address": "not_hex"}"#,
                &test_ctx(),
            )
            .await
            .expect_err("invalid hex should fail");
        assert!(err.to_string().contains("invalid hex address"));
    }

    #[tokio::test]
    async fn memory_read_unknown_board_fails() {
        let tool = HardwareMemoryReadTool;
        let err = tool
            .execute(r#"{"board": "fake", "address": "0x00"}"#, &test_ctx())
            .await
            .expect_err("unknown board should fail");
        assert!(err.to_string().contains("unknown hardware board"));
    }

    #[tokio::test]
    async fn memory_read_clamps_length() {
        let tool = HardwareMemoryReadTool;
        let result = tool
            .execute(
                r#"{"board": "sim-stm32", "address": "0x08000000", "length": 9999}"#,
                &test_ctx(),
            )
            .await
            .expect("should clamp, not fail");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["length"], 256);
    }
}
