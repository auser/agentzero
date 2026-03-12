//! Agent orchestration and runtime infrastructure.
//!
//! Wires together config, providers, tools, and the agent loop. Contains
//! the runtime execution builder, audit sinks, tool registration, and
//! the WASM plugin bridge.

pub mod audio;
pub mod audit;
pub mod cost_tracker;
pub mod runtime;
#[cfg(feature = "telemetry")]
pub mod telemetry;
pub mod tools;

// Compatibility re-exports while crates are split out from infra.
pub use agentzero_providers as provider;
pub use agentzero_storage::memory;
