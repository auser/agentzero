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

/// Build an [`AutopilotStore`] from the provided settings.
///
/// Selection logic (in order):
/// 1. When the `memory-turso` feature is enabled **and** `turso_url` is
///    non-empty, open a [`TursoAutopilotStore`]:
///    - Local file paths → delegates to `SqliteAutopilotStore` (rusqlite, sync).
///    - Remote URLs (`libsql://` / `https://`) or non-empty `turso_auth_token` →
///      `RemoteTursoInner` (libsql async driver).
/// 2. Otherwise (default build) open a [`SqliteAutopilotStore`] at `db_path`.
///
/// # Arguments
/// * `db_path`          — Where to store the SQLite database when not using Turso.
/// * `turso_url`        — Turso/libSQL URL. Ignored when the feature is disabled.
/// * `turso_auth_token` — Auth token for remote Turso databases.  Pass `""` for
///   local file paths or when the feature is disabled.
pub async fn build_autopilot_store(
    db_path: &std::path::Path,
    turso_url: &str,
    turso_auth_token: &str,
) -> anyhow::Result<Box<dyn AutopilotStore>> {
    #[cfg(feature = "memory-turso")]
    if !turso_url.is_empty() {
        return Ok(Box::new(
            TursoAutopilotStore::open(turso_url, turso_auth_token).await?,
        ));
    }
    // Suppress unused-variable warnings on non-turso builds.
    let _ = turso_url;
    let _ = turso_auth_token;
    Ok(Box::new(SqliteAutopilotStore::open(db_path)?))
}
