//! Tool-loop detection — re-exported from `agentzero_core::loop_detection`.
//!
//! The canonical implementation lives in `agentzero-core` so the `Agent` struct
//! can integrate loop detection directly into its tool-call loop. This module
//! re-exports the types for backward compatibility.

pub use agentzero_core::loop_detection::{LoopDetectionConfig, ToolLoopDetector};
