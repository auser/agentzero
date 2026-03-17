//! Autonomous company loop for AgentZero.
//!
//! Provides the autopilot engine: proposals, missions, cap gates, triggers,
//! reaction matrix, stale recovery, and Supabase integration. Agents propose
//! actions, the system auto-approves within constraints, creates executable
//! missions, workers execute steps, events trigger reactions — all without
//! human intervention.

pub mod cap_gate;
pub mod config;
pub mod loop_runner;
pub mod reaction_matrix;
pub mod stale_recovery;
pub mod supabase;
pub mod tools;
pub mod trigger;
pub mod types;

pub use cap_gate::{CapGate, CapGateResult};
pub use config::AutopilotConfig;
pub use loop_runner::AutopilotLoop;
pub use reaction_matrix::ReactionMatrix;
pub use stale_recovery::StaleRecovery;
pub use supabase::SupabaseClient;
pub use trigger::TriggerEngine;
pub use types::*;
