use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareBoard {
    pub id: String,
    pub display_name: String,
    pub architecture: String,
    pub memory_kb: u32,
}

/// ID used by `discover_boards()` for the live host detected via
/// `agentzero_core::device::detect()`.
pub const LIVE_HOST_BOARD_ID: &str = "live-host";

pub fn discover_boards() -> Vec<HardwareBoard> {
    let mut boards = vec![live_host_board()];
    boards.extend([
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
    ]);
    boards
}

/// Probe the running host via `agentzero_core::device::detect()` and return
/// a `HardwareBoard` describing it. Used as the first entry in
/// `discover_boards()` so the LLM has visibility into the real hardware,
/// not just the simulator stubs.
fn live_host_board() -> HardwareBoard {
    let caps = agentzero_core::device::detect();
    let arch = std::env::consts::ARCH.to_string();
    let memory_kb = u32::try_from(caps.total_memory_mb.saturating_mul(1024)).unwrap_or(u32::MAX);
    let display_name = format!(
        "Live host ({} cores, {} MiB, gpu={:?})",
        caps.cpu_cores, caps.total_memory_mb, caps.gpu
    );
    HardwareBoard {
        id: LIVE_HOST_BOARD_ID.to_string(),
        display_name,
        architecture: arch,
        memory_kb,
    }
}

pub fn board_info(id: &str) -> anyhow::Result<HardwareBoard> {
    discover_boards()
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| anyhow!("unknown hardware board id: {id}"))
}

#[cfg(test)]
mod tests {
    use super::{board_info, discover_boards, LIVE_HOST_BOARD_ID};

    #[test]
    fn discover_boards_returns_known_targets_success_path() {
        let boards = discover_boards();
        assert!(!boards.is_empty());
        assert!(boards.iter().any(|b| b.id == "sim-stm32"));
    }

    #[test]
    fn discover_boards_includes_live_host() {
        let boards = discover_boards();
        let live = boards
            .iter()
            .find(|b| b.id == LIVE_HOST_BOARD_ID)
            .expect("live host board should be present");
        assert!(live.memory_kb > 0, "live host should report nonzero memory");
        assert!(!live.architecture.is_empty());
    }

    #[test]
    fn board_info_rejects_unknown_id_negative_path() {
        let err = board_info("missing-board").expect_err("unknown board should fail");
        assert!(err.to_string().contains("unknown hardware board id"));
    }
}
