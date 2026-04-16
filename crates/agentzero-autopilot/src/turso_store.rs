//! `TursoAutopilotStore` — dispatching autopilot store.
//!
//! Routes to `SqliteAutopilotStore` for local/in-memory paths and to
//! `RemoteTursoInner` for remote Turso/libSQL endpoints.
//!
//! Gated behind the `memory-turso` feature flag.

#![cfg(feature = "memory-turso")]

use crate::store::{AutopilotStore, SqliteAutopilotStore};
use crate::types::{
    AutopilotEvent, Mission, MissionStatus, MissionStep, Priority, Proposal, ProposalStatus,
    ProposalType,
};
use async_trait::async_trait;
use libsql::{Builder, Connection};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Inner: remote libsql-backed store ─────────────────────────────────────────

/// Internal store that talks to a remote Turso/libSQL endpoint.
/// Only constructed when `url` is a remote libsql URL or `auth_token` is non-empty.
struct RemoteTursoInner {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for RemoteTursoInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTursoInner")
            .field("conn", &"<libsql::Connection>")
            .finish()
    }
}

impl RemoteTursoInner {
    async fn open(url: &str, auth_token: &str) -> anyhow::Result<Self> {
        let db = if auth_token.is_empty() {
            Builder::new_local(url).build().await?
        } else {
            Builder::new_remote(url.to_string(), auth_token.to_string())
                .build()
                .await?
        };
        let conn = db.connect()?;
        let inner = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        inner.run_migrations().await?;
        Ok(inner)
    }

    async fn run_migrations(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;

            CREATE TABLE IF NOT EXISTS autopilot_proposals (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                proposal_type TEXT NOT NULL,
                priority TEXT NOT NULL,
                estimated_cost_microdollars INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                cap_gate_result TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilot_missions (
                id TEXT PRIMARY KEY,
                proposal_id TEXT NOT NULL,
                title TEXT NOT NULL,
                assigned_agent TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                heartbeat_at TEXT NOT NULL,
                deadline TEXT,
                result TEXT,
                steps TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilot_events (
                id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                source_agent TEXT NOT NULL,
                payload TEXT NOT NULL DEFAULT '{}',
                correlation_id TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilot_cap_gate_ledger (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                cost_microdollars INTEGER NOT NULL,
                mission_id TEXT,
                recorded_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilot_content (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL DEFAULT '{}'
            );",
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl AutopilotStore for RemoteTursoInner {
    // ── Proposals ──

    async fn insert_proposal(&self, proposal: &Proposal) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let cap_gate_json = proposal
            .cap_gate_result
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());
        let metadata_json = serde_json::to_string(&proposal.metadata).unwrap_or_default();
        let proposal_type_str = serde_json::to_string(&proposal.proposal_type)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let priority_str = serde_json::to_string(&proposal.priority)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let status_str = serde_json::to_string(&proposal.status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let created_at = proposal.created_at.to_rfc3339();
        let updated_at = proposal.updated_at.to_rfc3339();
        let cap_gate_value = cap_gate_json
            .map(libsql::Value::Text)
            .unwrap_or(libsql::Value::Null);
        conn.execute(
            "INSERT INTO autopilot_proposals \
             (id, agent_id, title, description, proposal_type, priority, \
              estimated_cost_microdollars, status, cap_gate_result, metadata, \
              created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            libsql::params![
                proposal.id.clone(),
                proposal.agent_id.clone(),
                proposal.title.clone(),
                proposal.description.clone(),
                proposal_type_str,
                priority_str,
                proposal.estimated_cost_microdollars as i64,
                status_str,
                cap_gate_value,
                metadata_json,
                created_at,
                updated_at,
            ],
        )
        .await?;
        Ok(())
    }

    async fn update_proposal_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_proposals SET status = ?1, updated_at = ?2 WHERE id = ?3",
            libsql::params![status_str, now, proposal_id.to_string()],
        )
        .await?;
        Ok(())
    }

    async fn list_proposals(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>> {
        let conn = self.conn.lock().await;
        let mut rows = if let Some(s) = status_filter {
            conn.query(
                "SELECT id, agent_id, title, description, proposal_type, priority, \
                 estimated_cost_microdollars, status, cap_gate_result, metadata, \
                 created_at, updated_at \
                 FROM autopilot_proposals WHERE status = ?1 \
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                libsql::params![s.to_string(), limit as i64, offset as i64],
            )
            .await?
        } else {
            conn.query(
                "SELECT id, agent_id, title, description, proposal_type, priority, \
                 estimated_cost_microdollars, status, cap_gate_result, metadata, \
                 created_at, updated_at \
                 FROM autopilot_proposals ORDER BY created_at DESC \
                 LIMIT ?1 OFFSET ?2",
                libsql::params![limit as i64, offset as i64],
            )
            .await?
        };

        let mut proposals = Vec::new();
        while let Some(row) = rows.next().await? {
            proposals.push(row_to_proposal(&row)?);
        }
        Ok(proposals)
    }

    // ── Missions ──

    async fn insert_mission(&self, mission: &Mission) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let steps_json = serde_json::to_string(&mission.steps).unwrap_or_else(|_| "[]".to_string());
        let result_json = mission.result.as_ref().map(|r| r.to_string());
        let deadline = mission.deadline.map(|d| d.to_rfc3339());
        let status_str = serde_json::to_string(&mission.status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let heartbeat_at = mission.heartbeat_at.to_rfc3339();
        let created_at = mission.created_at.to_rfc3339();
        let updated_at = mission.updated_at.to_rfc3339();
        let deadline_value = deadline
            .map(libsql::Value::Text)
            .unwrap_or(libsql::Value::Null);
        let result_value = result_json
            .map(libsql::Value::Text)
            .unwrap_or(libsql::Value::Null);
        conn.execute(
            "INSERT INTO autopilot_missions \
             (id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, \
              result, steps, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            libsql::params![
                mission.id.clone(),
                mission.proposal_id.clone(),
                mission.title.clone(),
                mission.assigned_agent.clone(),
                status_str,
                heartbeat_at,
                deadline_value,
                result_value,
                steps_json,
                created_at,
                updated_at,
            ],
        )
        .await?;
        Ok(())
    }

    async fn update_mission_status(
        &self,
        mission_id: &str,
        status: MissionStatus,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_missions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            libsql::params![status_str, now, mission_id.to_string()],
        )
        .await?;
        Ok(())
    }

    async fn heartbeat_mission(&self, mission_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_missions SET heartbeat_at = ?1, updated_at = ?1 WHERE id = ?2",
            libsql::params![now, mission_id.to_string()],
        )
        .await?;
        Ok(())
    }

    async fn query_stale_missions(&self, threshold_minutes: u32) -> anyhow::Result<Vec<Mission>> {
        let conn = self.conn.lock().await;
        let threshold = chrono::Utc::now() - chrono::Duration::minutes(threshold_minutes as i64);
        let threshold_str = threshold.to_rfc3339();
        let mut rows = conn
            .query(
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, \
                 deadline, result, steps, created_at, updated_at \
                 FROM autopilot_missions \
                 WHERE status = 'in_progress' AND heartbeat_at < ?1",
                libsql::params![threshold_str],
            )
            .await?;

        let mut missions = Vec::new();
        while let Some(row) = rows.next().await? {
            missions.push(row_to_mission(&row)?);
        }
        Ok(missions)
    }

    async fn list_missions(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Mission>> {
        let conn = self.conn.lock().await;
        let mut rows = if let Some(s) = status_filter {
            conn.query(
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, \
                 deadline, result, steps, created_at, updated_at \
                 FROM autopilot_missions WHERE status = ?1 \
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                libsql::params![s.to_string(), limit as i64, offset as i64],
            )
            .await?
        } else {
            conn.query(
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, \
                 deadline, result, steps, created_at, updated_at \
                 FROM autopilot_missions ORDER BY created_at DESC \
                 LIMIT ?1 OFFSET ?2",
                libsql::params![limit as i64, offset as i64],
            )
            .await?
        };

        let mut missions = Vec::new();
        while let Some(row) = rows.next().await? {
            missions.push(row_to_mission(&row)?);
        }
        Ok(missions)
    }

    async fn get_mission(&self, mission_id: &str) -> anyhow::Result<Option<Mission>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, \
                 deadline, result, steps, created_at, updated_at \
                 FROM autopilot_missions WHERE id = ?1",
                libsql::params![mission_id.to_string()],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_mission(&row)?))
        } else {
            Ok(None)
        }
    }

    // ── Aggregations ──

    async fn get_daily_spend(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().await;
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight timestamp");
        let today_start_utc =
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(today_start, chrono::Utc);
        let mut rows = conn
            .query(
                "SELECT COALESCE(SUM(cost_microdollars), 0) \
                 FROM autopilot_cap_gate_ledger WHERE recorded_at >= ?1",
                libsql::params![today_start_utc.to_rfc3339()],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(row.get::<i64>(0)? as u64)
        } else {
            Ok(0)
        }
    }

    async fn get_concurrent_mission_count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM autopilot_missions WHERE status = 'in_progress'",
                (),
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(row.get::<i64>(0)? as usize)
        } else {
            Ok(0)
        }
    }

    // ── Events ──

    async fn insert_event(&self, event: &AutopilotEvent) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let payload_json =
            serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
        let correlation_id_value = event
            .correlation_id
            .clone()
            .map(libsql::Value::Text)
            .unwrap_or(libsql::Value::Null);
        conn.execute(
            "INSERT INTO autopilot_events \
             (id, event_type, source_agent, payload, correlation_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            libsql::params![
                event.id.clone(),
                event.event_type.clone(),
                event.source_agent.clone(),
                payload_json,
                correlation_id_value,
                event.created_at.to_rfc3339(),
            ],
        )
        .await?;
        Ok(())
    }

    // ── Content ──

    async fn upsert_content(&self, content: &Value) -> anyhow::Result<()> {
        let id = content
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let data = serde_json::to_string(content).unwrap_or_else(|_| "{}".to_string());
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO autopilot_content (id, data) VALUES (?1, ?2) \
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            libsql::params![id, data],
        )
        .await?;
        Ok(())
    }
}

// ── Public facade ─────────────────────────────────────────────────────────────

/// Dispatching autopilot store.
///
/// - Local / in-memory paths → delegates to `SqliteAutopilotStore` (rusqlite).
/// - Remote (`libsql://` or `https://`, or non-empty `auth_token`) →
///   delegates to `RemoteTursoInner` (libsql).
pub enum TursoAutopilotStore {
    Local(SqliteAutopilotStore),
    Remote(RemoteTursoInner),
}

impl std::fmt::Debug for TursoAutopilotStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(s) => f
                .debug_tuple("TursoAutopilotStore::Local")
                .field(s)
                .finish(),
            Self::Remote(_) => f
                .debug_struct("TursoAutopilotStore::Remote")
                .field("conn", &"<libsql::Connection>")
                .finish(),
        }
    }
}

impl TursoAutopilotStore {
    /// Open a store at the given URL.
    ///
    /// - Local file paths use `SqliteAutopilotStore` (rusqlite).
    /// - Remote URLs (`libsql://`, `https://`) or non-empty `auth_token` use
    ///   `RemoteTursoInner` (libsql).
    pub async fn open(url: &str, auth_token: &str) -> anyhow::Result<Self> {
        if !auth_token.is_empty() || url.starts_with("libsql://") || url.starts_with("https://") {
            Ok(Self::Remote(RemoteTursoInner::open(url, auth_token).await?))
        } else {
            Ok(Self::Local(SqliteAutopilotStore::open(
                std::path::Path::new(url),
            )?))
        }
    }

    /// In-memory store for tests — backed by rusqlite, no threading conflict.
    #[cfg(test)]
    pub async fn in_memory() -> anyhow::Result<Self> {
        Ok(Self::Local(SqliteAutopilotStore::in_memory()?))
    }
}

#[async_trait]
impl AutopilotStore for TursoAutopilotStore {
    async fn insert_proposal(&self, proposal: &Proposal) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.insert_proposal(proposal).await,
            Self::Remote(s) => s.insert_proposal(proposal).await,
        }
    }

    async fn update_proposal_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.update_proposal_status(proposal_id, status).await,
            Self::Remote(s) => s.update_proposal_status(proposal_id, status).await,
        }
    }

    async fn list_proposals(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>> {
        match self {
            Self::Local(s) => s.list_proposals(status_filter, limit, offset).await,
            Self::Remote(s) => s.list_proposals(status_filter, limit, offset).await,
        }
    }

    async fn insert_mission(&self, mission: &Mission) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.insert_mission(mission).await,
            Self::Remote(s) => s.insert_mission(mission).await,
        }
    }

    async fn update_mission_status(
        &self,
        mission_id: &str,
        status: MissionStatus,
    ) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.update_mission_status(mission_id, status).await,
            Self::Remote(s) => s.update_mission_status(mission_id, status).await,
        }
    }

    async fn heartbeat_mission(&self, mission_id: &str) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.heartbeat_mission(mission_id).await,
            Self::Remote(s) => s.heartbeat_mission(mission_id).await,
        }
    }

    async fn query_stale_missions(&self, threshold_minutes: u32) -> anyhow::Result<Vec<Mission>> {
        match self {
            Self::Local(s) => s.query_stale_missions(threshold_minutes).await,
            Self::Remote(s) => s.query_stale_missions(threshold_minutes).await,
        }
    }

    async fn list_missions(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Mission>> {
        match self {
            Self::Local(s) => s.list_missions(status_filter, limit, offset).await,
            Self::Remote(s) => s.list_missions(status_filter, limit, offset).await,
        }
    }

    async fn get_mission(&self, mission_id: &str) -> anyhow::Result<Option<Mission>> {
        match self {
            Self::Local(s) => s.get_mission(mission_id).await,
            Self::Remote(s) => s.get_mission(mission_id).await,
        }
    }

    async fn get_daily_spend(&self) -> anyhow::Result<u64> {
        match self {
            Self::Local(s) => s.get_daily_spend().await,
            Self::Remote(s) => s.get_daily_spend().await,
        }
    }

    async fn get_concurrent_mission_count(&self) -> anyhow::Result<usize> {
        match self {
            Self::Local(s) => s.get_concurrent_mission_count().await,
            Self::Remote(s) => s.get_concurrent_mission_count().await,
        }
    }

    async fn insert_event(&self, event: &AutopilotEvent) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.insert_event(event).await,
            Self::Remote(s) => s.insert_event(event).await,
        }
    }

    async fn upsert_content(&self, content: &Value) -> anyhow::Result<()> {
        match self {
            Self::Local(s) => s.upsert_content(content).await,
            Self::Remote(s) => s.upsert_content(content).await,
        }
    }
}

// ── Row helpers ───────────────────────────────────────────────────────────────
// Used only by RemoteTursoInner — parse libsql rows into domain types.

fn row_to_proposal(row: &libsql::Row) -> anyhow::Result<Proposal> {
    let id: String = row.get(0)?;
    let agent_id: String = row.get(1)?;
    let title: String = row.get(2)?;
    let description: String = row.get(3)?;
    let proposal_type_str: String = row.get(4)?;
    let priority_str: String = row.get(5)?;
    let cost: i64 = row.get(6)?;
    let status_str: String = row.get(7)?;
    let cap_gate: Option<String> = row.get(8)?;
    let metadata_str: Option<String> = row.get(9)?;
    let created_at_str: String = row.get(10)?;
    let updated_at_str: String = row.get(11)?;

    let proposal_type = serde_json::from_str(&format!("\"{}\"", proposal_type_str))
        .unwrap_or(ProposalType::TaskRequest);
    let priority =
        serde_json::from_str(&format!("\"{}\"", priority_str)).unwrap_or(Priority::Medium);
    let status =
        serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or(ProposalStatus::Pending);
    let cap_gate_result = cap_gate.and_then(|s| serde_json::from_str(&s).ok());
    let metadata: Value = metadata_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    Ok(Proposal {
        id,
        agent_id,
        title,
        description,
        proposal_type,
        priority,
        estimated_cost_microdollars: cost as u64,
        status,
        cap_gate_result,
        metadata,
        created_at,
        updated_at,
    })
}

fn row_to_mission(row: &libsql::Row) -> anyhow::Result<Mission> {
    let id: String = row.get(0)?;
    let proposal_id: String = row.get(1)?;
    let title: String = row.get(2)?;
    let assigned_agent: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    let heartbeat_at_str: String = row.get(5)?;
    let deadline_str: Option<String> = row.get(6)?;
    let result_str: Option<String> = row.get(7)?;
    let steps_str: String = row.get(8)?;
    let created_at_str: String = row.get(9)?;
    let updated_at_str: String = row.get(10)?;

    let status =
        serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or(MissionStatus::Pending);
    let heartbeat_at = chrono::DateTime::parse_from_rfc3339(&heartbeat_at_str)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let deadline = deadline_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|d| d.with_timezone(&chrono::Utc))
            .ok()
    });
    let result = result_str.and_then(|s| serde_json::from_str(&s).ok());
    let steps: Vec<MissionStep> = serde_json::from_str(&steps_str).unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    Ok(Mission {
        id,
        proposal_id,
        title,
        assigned_agent,
        status,
        heartbeat_at,
        deadline,
        result,
        steps,
        created_at,
        updated_at,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AutopilotEvent, Priority, ProposalType};

    fn test_proposal() -> Proposal {
        Proposal::new(
            "test-agent",
            "Test Proposal",
            "A test proposal",
            ProposalType::TaskRequest,
            Priority::Medium,
            1_000_000,
        )
    }

    fn test_mission(proposal: &Proposal) -> Mission {
        Mission::from_proposal(proposal, vec![])
    }

    #[tokio::test]
    async fn create_and_read_proposal_roundtrip() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store.insert_proposal(&proposal).await.expect("insert");
        let proposals = store.list_proposals(None, 10, 0).await.expect("list");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].id, proposal.id);
        assert_eq!(proposals[0].title, proposal.title);
        assert_eq!(proposals[0].agent_id, proposal.agent_id);
    }

    #[tokio::test]
    async fn update_proposal_status_roundtrip() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store.insert_proposal(&proposal).await.expect("insert");

        store
            .update_proposal_status(&proposal.id, ProposalStatus::Approved)
            .await
            .expect("update status");

        let proposals = store
            .list_proposals(Some("approved"), 10, 0)
            .await
            .expect("list");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].status, ProposalStatus::Approved);
    }

    #[tokio::test]
    async fn mission_insert_and_get() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store
            .insert_proposal(&proposal)
            .await
            .expect("insert proposal");
        let mission = test_mission(&proposal);
        store
            .insert_mission(&mission)
            .await
            .expect("insert mission");

        let fetched = store
            .get_mission(&mission.id)
            .await
            .expect("get")
            .expect("mission exists");
        assert_eq!(fetched.id, mission.id);
        assert_eq!(fetched.proposal_id, proposal.id);
        assert_eq!(fetched.assigned_agent, mission.assigned_agent);
    }

    #[tokio::test]
    async fn mission_status_update() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store
            .insert_proposal(&proposal)
            .await
            .expect("insert proposal");
        let mission = test_mission(&proposal);
        store
            .insert_mission(&mission)
            .await
            .expect("insert mission");

        store
            .update_mission_status(&mission.id, MissionStatus::InProgress)
            .await
            .expect("update status");

        let fetched = store
            .get_mission(&mission.id)
            .await
            .expect("get")
            .expect("mission exists");
        assert_eq!(fetched.status, MissionStatus::InProgress);
    }

    #[tokio::test]
    async fn get_missing_mission_returns_none() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let result = store.get_mission("nonexistent-id").await.expect("get");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn stale_missions_query() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store
            .insert_proposal(&proposal)
            .await
            .expect("insert proposal");

        // Create a mission that is in_progress but has a very old heartbeat.
        let mut mission = test_mission(&proposal);
        mission.status = MissionStatus::InProgress;
        mission.heartbeat_at = chrono::Utc::now() - chrono::Duration::hours(2);
        store.insert_mission(&mission).await.expect("insert");

        // threshold = 30 minutes: heartbeat is 2 hours old, so it should be stale.
        let stale = store.query_stale_missions(30).await.expect("query stale");
        assert!(!stale.is_empty(), "expected at least one stale mission");
        assert_eq!(stale[0].id, mission.id);
    }

    #[tokio::test]
    async fn stale_missions_empty_when_fresh() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store
            .insert_proposal(&proposal)
            .await
            .expect("insert proposal");

        // A freshly created in_progress mission should not be stale with a 30-min threshold.
        let mut mission = test_mission(&proposal);
        mission.status = MissionStatus::InProgress;
        store.insert_mission(&mission).await.expect("insert");

        let stale = store.query_stale_missions(30).await.expect("query stale");
        assert!(stale.is_empty(), "fresh mission should not be stale");
    }

    #[tokio::test]
    async fn daily_spend_starts_at_zero() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let spend = store.get_daily_spend().await.expect("daily spend");
        assert_eq!(spend, 0);
    }

    #[tokio::test]
    async fn concurrent_mission_count() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let proposal = test_proposal();
        store
            .insert_proposal(&proposal)
            .await
            .expect("insert proposal");

        // No running missions yet.
        assert_eq!(
            store.get_concurrent_mission_count().await.expect("count"),
            0
        );

        // Insert one in_progress mission.
        let mission = test_mission(&proposal);
        store
            .insert_mission(&mission)
            .await
            .expect("insert mission");
        store
            .update_mission_status(&mission.id, MissionStatus::InProgress)
            .await
            .expect("update");

        assert_eq!(
            store.get_concurrent_mission_count().await.expect("count"),
            1
        );
    }

    #[tokio::test]
    async fn insert_and_query_event() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let event = AutopilotEvent::new(
            "proposal.created",
            "editor-agent",
            serde_json::json!({"proposal_id": "p-1"}),
        );
        store.insert_event(&event).await.expect("insert event");
        // No list_events on the trait, just verify insert doesn't error.
    }

    #[tokio::test]
    async fn upsert_content_roundtrip() {
        let store = TursoAutopilotStore::in_memory().await.expect("in_memory");
        let content = serde_json::json!({
            "id": "content-1",
            "type": "article",
            "body": "Hello world"
        });
        store.upsert_content(&content).await.expect("upsert");
        // Second upsert should not error (ON CONFLICT DO UPDATE).
        let updated = serde_json::json!({
            "id": "content-1",
            "type": "article",
            "body": "Updated body"
        });
        store.upsert_content(&updated).await.expect("upsert again");
    }
}
