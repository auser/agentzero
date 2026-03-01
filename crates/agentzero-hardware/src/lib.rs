use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareBoard {
    pub id: String,
    pub display_name: String,
    pub architecture: String,
    pub memory_kb: u32,
}

pub fn discover_boards() -> Vec<HardwareBoard> {
    vec![
        HardwareBoard {
            id: "sim-stm32".to_string(),
            display_name: "Simulated STM32 Board".to_string(),
            architecture: "arm-cortex-m".to_string(),
            memory_kb: 256,
        },
        HardwareBoard {
            id: "sim-rpi".to_string(),
            display_name: "Simulated Raspberry Pi".to_string(),
            architecture: "arm64".to_string(),
            memory_kb: 1024 * 1024,
        },
    ]
}

pub fn board_info(id: &str) -> anyhow::Result<HardwareBoard> {
    discover_boards()
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| anyhow!("unknown hardware board id: {id}"))
}

#[cfg(test)]
mod tests {
    use super::{board_info, discover_boards};

    #[test]
    fn discover_boards_returns_known_targets_success_path() {
        let boards = discover_boards();
        assert!(!boards.is_empty());
        assert!(boards.iter().any(|b| b.id == "sim-stm32"));
    }

    #[test]
    fn board_info_rejects_unknown_id_negative_path() {
        let err = board_info("missing-board").expect_err("unknown board should fail");
        assert!(err.to_string().contains("unknown hardware board id"));
    }
}
