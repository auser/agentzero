//! Cross-platform CPU and memory detection.
//!
//! When the `hardware-detect` feature is enabled, uses the `sysinfo` crate
//! for accurate readings. When disabled, returns safe fallback values so
//! callers don't need any `#[cfg]` gates.

use super::types::DetectionConfidence;

/// Returns the number of logical CPU cores. Falls back to 1 when `sysinfo`
/// is not available.
#[cfg(feature = "hardware-detect")]
pub fn detect_cpu_cores() -> usize {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::new());
    sys.cpus().len().max(1)
}

#[cfg(not(feature = "hardware-detect"))]
pub fn detect_cpu_cores() -> usize {
    1
}

/// Returns total system memory in MiB and a confidence level.
///
/// When `sysinfo` is available, reports physical memory in MiB with `High`
/// confidence. Without it, returns `(0, Low)` so callers fall back to
/// explicit configuration.
#[cfg(feature = "hardware-detect")]
pub fn detect_memory_mb() -> (u64, DetectionConfidence) {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory(); // bytes
    if total == 0 {
        (0, DetectionConfidence::Low)
    } else {
        (total / (1024 * 1024), DetectionConfidence::High)
    }
}

#[cfg(not(feature = "hardware-detect"))]
pub fn detect_memory_mb() -> (u64, DetectionConfidence) {
    (0, DetectionConfidence::Low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_cores_at_least_one() {
        assert!(detect_cpu_cores() >= 1);
    }

    #[cfg(feature = "hardware-detect")]
    #[test]
    fn memory_nonzero_on_test_host() {
        // Any machine that can run cargo test has nonzero RAM.
        let (mb, conf) = detect_memory_mb();
        assert!(mb > 0, "expected nonzero memory");
        assert_eq!(conf, DetectionConfidence::High);
    }

    #[cfg(not(feature = "hardware-detect"))]
    #[test]
    fn memory_fallback_returns_low_confidence() {
        let (mb, conf) = detect_memory_mb();
        assert_eq!(mb, 0);
        assert_eq!(conf, DetectionConfidence::Low);
    }
}
