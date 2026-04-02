//! Agent orchestration and runtime infrastructure.
//!
//! Wires together config, providers, tools, and the agent loop. Contains
//! the runtime execution builder, audit sinks, tool registration, and
//! the WASM plugin bridge.

pub mod audio;
pub mod audit;
pub mod cost_tracker;
pub mod insights;
pub mod mcp_server;
pub mod pattern_capture;
pub mod runtime;
pub mod sender_rate_limiter;
#[cfg(feature = "telemetry")]
pub mod telemetry;
pub mod tool_evolver;
pub mod tool_recipes;
pub mod tool_selection;
pub mod tools;
pub mod trajectory;

// Compatibility re-exports while crates are split out from infra.
pub use agentzero_providers as provider;
pub use agentzero_storage::memory;
pub use sender_rate_limiter::SenderRateLimiter;
pub use tool_selection::{AiToolSelector, AllToolSelector, KeywordToolSelector};
