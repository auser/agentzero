//! Linux GPU detection — currently focused on NVIDIA / CUDA.
//!
//! We don't link against CUDA at compile time. Instead we check for the
//! kernel module proc directory (`/proc/driver/nvidia/`) and, as a secondary
//! signal, the `nvidia-smi` binary. Either signal is enough for `Medium`
//! confidence; both together give `High`.

#![cfg(target_os = "linux")]

use super::types::{DetectionConfidence, GpuType};

/// Detect GPU backend on Linux.
pub fn detect_gpu() -> (GpuType, DetectionConfidence) {
    let kernel_module = std::path::Path::new("/proc/driver/nvidia").exists();
    let smi_present = which_nvidia_smi().is_some();

    match (kernel_module, smi_present) {
        (true, true) => (GpuType::Cuda, DetectionConfidence::High),
        (true, false) | (false, true) => (GpuType::Cuda, DetectionConfidence::Medium),
        (false, false) => (GpuType::None, DetectionConfidence::High),
    }
}

/// Look for `nvidia-smi` on the user's `PATH`. We do NOT execute it — only
/// check that it exists, to avoid spawning a subprocess on every detect().
fn which_nvidia_smi() -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("nvidia-smi");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gpu_does_not_panic_on_linux() {
        // We can't assert a specific GPU on CI; just confirm the call succeeds.
        let (_, conf) = detect_gpu();
        // Linux always returns at least Medium confidence (either way).
        assert_ne!(conf, DetectionConfidence::Low);
    }
}
