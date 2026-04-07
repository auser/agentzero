//! Runtime hardware capability detection.
//!
//! The [`detect()`] entry point inspects the host and returns a
//! [`HardwareCapabilities`] struct that backend selection (Candle, llama.cpp)
//! and tools (hardware discovery) can consume. All probes are best-effort
//! and feature-light: we don't link CUDA or Metal at compile time, just
//! check sidecar signals (filesystem markers, binaries on `PATH`).

pub mod common;
pub mod types;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "android")]
pub mod android;

pub use types::{DetectionConfidence, GpuType, HardwareCapabilities, NpuType, ThermalState};

/// Probe the host and return its capability profile.
///
/// The platform-specific probes never panic; on any failure they return
/// `(GpuType::None / NpuType::None, DetectionConfidence::Low)` and the
/// caller falls back to explicit configuration.
pub fn detect() -> HardwareCapabilities {
    let cpu_cores = common::detect_cpu_cores();
    let (total_memory_mb, memory_confidence) = common::detect_memory_mb();

    let (gpu, _gpu_conf) = detect_gpu_for_target();
    let (npu, _npu_conf) = detect_npu_for_target();

    HardwareCapabilities {
        cpu_cores,
        total_memory_mb,
        gpu,
        npu,
        thermal: ThermalState::Nominal,
        battery_pct: None,
        memory_confidence,
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn detect_gpu_for_target() -> (GpuType, DetectionConfidence) {
    apple::detect_gpu()
}

#[cfg(target_os = "linux")]
fn detect_gpu_for_target() -> (GpuType, DetectionConfidence) {
    linux::detect_gpu()
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux")))]
fn detect_gpu_for_target() -> (GpuType, DetectionConfidence) {
    (GpuType::None, DetectionConfidence::High)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn detect_npu_for_target() -> (NpuType, DetectionConfidence) {
    apple::detect_npu()
}

#[cfg(target_os = "android")]
fn detect_npu_for_target() -> (NpuType, DetectionConfidence) {
    android::detect_npu()
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
fn detect_npu_for_target() -> (NpuType, DetectionConfidence) {
    (NpuType::None, DetectionConfidence::High)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_nonzero_cpu_and_memory() {
        let caps = detect();
        assert!(caps.cpu_cores >= 1, "expected at least one CPU core");
        assert!(caps.total_memory_mb > 0, "expected nonzero memory");
        assert_eq!(caps.memory_confidence, DetectionConfidence::High);
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn detect_apple_gpu_and_npu() {
        let caps = detect();
        assert_eq!(caps.gpu, GpuType::Metal);
        assert_eq!(caps.npu, NpuType::CoreML);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_linux_gpu_returns_known_value() {
        let caps = detect();
        // Either Cuda (if NVIDIA detected) or None — never an unknown value.
        assert!(matches!(caps.gpu, GpuType::Cuda | GpuType::None));
    }
}
