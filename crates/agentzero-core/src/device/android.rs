//! Android NPU stub.
//!
//! NNAPI probing requires JNI access into the running JVM, which AgentZero
//! doesn't yet have on Android targets. We return a `Low`-confidence stub so
//! callers know to fall back to explicit configuration.

#![cfg(target_os = "android")]

use super::types::{DetectionConfidence, NpuType};

/// Stub NNAPI detection. Returns `(NpuType::Nnapi, Low)` so backend selectors
/// know NNAPI *might* be available but should not auto-select it.
pub fn detect_npu() -> (NpuType, DetectionConfidence) {
    (NpuType::Nnapi, DetectionConfidence::Low)
}
