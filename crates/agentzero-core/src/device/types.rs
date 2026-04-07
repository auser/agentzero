//! Device capability types — the shared vocabulary every platform detector
//! and every backend selector uses to reason about the host.

use serde::{Deserialize, Serialize};

/// Runtime hardware profile of the current host.
///
/// Populated by [`crate::device::detect()`], consumed by backend selection
/// (Candle, llama.cpp) and by tools that surface hardware information to
/// the LLM (e.g. the hardware discovery tool).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    /// Number of logical CPU cores.
    pub cpu_cores: usize,
    /// Total system memory in MiB (mebibytes).
    pub total_memory_mb: u64,
    /// Detected GPU backend available for acceleration.
    pub gpu: GpuType,
    /// Detected NPU backend available for acceleration.
    pub npu: NpuType,
    /// Current thermal state. Defaults to `Nominal` on platforms without probes.
    pub thermal: ThermalState,
    /// Battery percentage (0–100) if available and relevant, else `None`.
    pub battery_pct: Option<u8>,
    /// How much to trust the memory figure. Mobile platforms often report
    /// an inflated value; certain sandboxes under-report.
    pub memory_confidence: DetectionConfidence,
}

/// GPU backend family available on the host. These correspond to the backends
/// Candle can currently target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuType {
    /// Apple Metal (macOS / iOS / iPadOS).
    Metal,
    /// NVIDIA CUDA (Linux, Windows).
    Cuda,
    /// Vulkan compute (cross-platform; reserved for future use).
    Vulkan,
    /// No dedicated GPU acceleration available — CPU fallback.
    None,
}

/// Neural processing unit backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NpuType {
    /// Apple Neural Engine via Core ML.
    CoreML,
    /// Android Neural Networks API.
    Nnapi,
    /// No NPU detected (or not yet probed).
    None,
}

/// Thermal pressure state. Backends can down-shift batch sizes or switch to
/// CPU when the host is under sustained load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThermalState {
    /// Normal operating temperature.
    Nominal,
    /// Mild thermal pressure; consider lighter workloads.
    Fair,
    /// Throttling likely imminent.
    Serious,
    /// Thermal throttling active.
    Critical,
}

/// How strongly to trust a detection result. Informs fallback logic:
/// `Low`-confidence GPU detection defers to explicit config, `High`-confidence
/// overrides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DetectionConfidence {
    /// Authoritative — probed a real API and got a real answer.
    High,
    /// Heuristic — inferred from sidecar signals (e.g. tool presence).
    Medium,
    /// Stub / placeholder — backend-specific probe not implemented yet.
    Low,
}

impl HardwareCapabilities {
    /// Construct a conservative fallback value when all detectors fail.
    /// Used when the platform layer returns an error so upstream code never
    /// sees an uninitialized `HardwareCapabilities`.
    pub fn unknown() -> Self {
        Self {
            cpu_cores: 1,
            total_memory_mb: 0,
            gpu: GpuType::None,
            npu: NpuType::None,
            thermal: ThermalState::Nominal,
            battery_pct: None,
            memory_confidence: DetectionConfidence::Low,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_has_safe_defaults() {
        let caps = HardwareCapabilities::unknown();
        assert_eq!(caps.gpu, GpuType::None);
        assert_eq!(caps.npu, NpuType::None);
        assert_eq!(caps.memory_confidence, DetectionConfidence::Low);
        assert!(caps.cpu_cores >= 1);
    }

    #[test]
    fn serde_roundtrip() {
        let caps = HardwareCapabilities {
            cpu_cores: 8,
            total_memory_mb: 16384,
            gpu: GpuType::Metal,
            npu: NpuType::CoreML,
            thermal: ThermalState::Nominal,
            battery_pct: Some(87),
            memory_confidence: DetectionConfidence::High,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: HardwareCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, back);
    }
}
