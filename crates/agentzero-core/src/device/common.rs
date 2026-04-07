//! Cross-platform CPU and memory detection via the `sysinfo` crate.

use super::types::DetectionConfidence;

/// Returns the number of logical CPU cores. Falls back to 1 on platforms
/// where `sysinfo` cannot determine the count.
pub fn detect_cpu_cores() -> usize {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::new());
    sys.cpus().len().max(1)
}

/// Returns total system memory in MiB and a confidence level.
///
/// `sysinfo` reports physical memory in bytes; we convert to MiB. Confidence
/// is `High` on every supported desktop platform; mobile platforms (iOS/Android)
/// will override this in their own platform module if needed.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_cores_at_least_one() {
        assert!(detect_cpu_cores() >= 1);
    }

    #[test]
    fn memory_nonzero_on_test_host() {
        // Any machine that can run cargo test has nonzero RAM.
        let (mb, conf) = detect_memory_mb();
        assert!(mb > 0, "expected nonzero memory");
        assert_eq!(conf, DetectionConfidence::High);
    }
}
