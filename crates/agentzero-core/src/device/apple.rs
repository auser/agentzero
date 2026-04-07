//! Apple-platform GPU and NPU detection (macOS, iOS).
//!
//! Both probes are heuristic — we don't link against Metal or Core ML at
//! compile time (that would force every Apple build to add framework deps).
//! Instead we check for the presence of well-known system files that ship
//! with the relevant frameworks.

#![cfg(any(target_os = "macos", target_os = "ios"))]

use super::types::{DetectionConfidence, GpuType, NpuType};

/// Detect Metal availability on the current Apple host.
///
/// Every modern Mac and iOS device ships Metal as part of the OS, so we return
/// `Metal` with `High` confidence on Apple platforms by default.
pub fn detect_gpu() -> (GpuType, DetectionConfidence) {
    if std::path::Path::new("/System/Library/Frameworks/Metal.framework").exists() {
        return (GpuType::Metal, DetectionConfidence::High);
    }
    // iOS sandbox layout differs; presence of any Apple GPU is still safe to
    // assume on iOS targets, but we mark it Medium since we couldn't probe.
    (GpuType::Metal, DetectionConfidence::Medium)
}

/// Detect Core ML / Neural Engine availability.
///
/// CoreML.framework ships on every Mac running macOS 10.13+ and every iOS
/// device running iOS 11+, both far below our minimum supported versions.
pub fn detect_npu() -> (NpuType, DetectionConfidence) {
    if std::path::Path::new("/System/Library/Frameworks/CoreML.framework").exists() {
        return (NpuType::CoreML, DetectionConfidence::High);
    }
    (NpuType::CoreML, DetectionConfidence::Medium)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gpu_returns_metal_on_apple() {
        let (gpu, _) = detect_gpu();
        assert_eq!(gpu, GpuType::Metal);
    }

    #[test]
    fn detect_npu_returns_coreml_on_apple() {
        let (npu, _) = detect_npu();
        assert_eq!(npu, NpuType::CoreML);
    }
}
