//! Autonomous company loop for AgentZero.
//!
//! Provides the autopilot engine: proposals, missions, cap gates, triggers,
//! reaction matrix, stale recovery, and local-first SQLite persistence.
//! Agents propose actions, the system auto-approves within constraints,
//! creates executable missions, workers execute steps, events trigger
//! reactions — all without human intervention.

pub mod cap_gate;
pub mod config;
pub mod reaction_matrix;
pub mod stale_recovery;
pub mod store;
pub mod tools;
pub mod trigger;
#[cfg(feature = "memory-turso")]
pub mod turso_store;
pub mod types;

pub use cap_gate::{CapGate, CapGateResult};
pub use config::AutopilotConfig;
pub use reaction_matrix::ReactionMatrix;
pub use stale_recovery::StaleRecovery;
pub use store::{AutopilotStore, SqliteAutopilotStore};
pub use trigger::TriggerEngine;
#[cfg(feature = "memory-turso")]
pub use turso_store::TursoAutopilotStore;
pub use types::*;
