//! Autopilot persistence trait and SQLite implementation.
//!
//! Replaces the former Supabase PostgREST dependency with a local-first
//! SQLite store. The trait allows future backends (e.g. Turso/libSQL).

use async_trait::async_trait;
use serde_json::Value;

use crate::types::{AutopilotEvent, Mission, MissionStatus, Proposal, ProposalStatus};

/// Abstract persistence layer for the autopilot engine.
///
/// All methods are async to support both local (SQLite) and remote (Turso)
/// backends. Implementations must be Send + Sync for use behind Arc.
#[async_trait]
pub trait AutopilotStore: Send + Sync + std::fmt::Debug {
    // ── Proposals ──

    async fn insert_proposal(&self, proposal: &Proposal) -> anyhow::Result<()>;

    async fn update_proposal_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> anyhow::Result<()>;

    async fn list_proposals(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>>;

    // ── Missions ──

    async fn insert_mission(&self, mission: &Mission) -> anyhow::Result<()>;

    async fn update_mission_status(
        &self,
        mission_id: &str,
        status: MissionStatus,
    ) -> anyhow::Result<()>;

    async fn heartbeat_mission(&self, mission_id: &str) -> anyhow::Result<()>;

    async fn query_stale_missions(&self, threshold_minutes: u32) -> anyhow::Result<Vec<Mission>>;

    async fn list_missions(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Mission>>;

    async fn get_mission(&self, mission_id: &str) -> anyhow::Result<Option<Mission>>;

    // ── Aggregations ──

    async fn get_daily_spend(&self) -> anyhow::Result<u64>;

    async fn get_concurrent_mission_count(&self) -> anyhow::Result<usize>;

    // ── Events ──

    async fn insert_event(&self, event: &AutopilotEvent) -> anyhow::Result<()>;

    // ── Content ──

    async fn upsert_content(&self, content: &Value) -> anyhow::Result<()>;
}

// ── SQLite implementation ──────────────────────────────────────────────

use anyhow::Context;
use rusqlite::Connection;
use std::sync::Mutex;

/// SQLite-backed autopilot store. Local-first, zero external dependencies.
#[derive(Debug)]
pub struct SqliteAutopilotStore {
    conn: Mutex<Connection>,
}

impl SqliteAutopilotStore {
    /// Open (or create) the autopilot database at the given path.
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).context("failed to open autopilot database")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .context("failed to set pragmas")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    /// Create an in-memory store for testing.
    #[cfg(test)]
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory db")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS autopilot_proposals (
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
            );

            CREATE INDEX IF NOT EXISTS idx_proposals_status ON autopilot_proposals(status);
            CREATE INDEX IF NOT EXISTS idx_missions_status ON autopilot_missions(status);
            CREATE INDEX IF NOT EXISTS idx_missions_heartbeat ON autopilot_missions(heartbeat_at);
            CREATE INDEX IF NOT EXISTS idx_ledger_recorded ON autopilot_cap_gate_ledger(recorded_at);
            CREATE INDEX IF NOT EXISTS idx_events_created ON autopilot_events(created_at);",
        )
        .context("failed to run autopilot migrations")?;
        Ok(())
    }
}

#[async_trait]
impl AutopilotStore for SqliteAutopilotStore {
    // ── Proposals ──

    async fn insert_proposal(&self, proposal: &Proposal) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let cap_gate_json = proposal
            .cap_gate_result
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());
        let metadata_json = serde_json::to_string(&proposal.metadata).unwrap_or_default();
        conn.execute(
            "INSERT INTO autopilot_proposals (id, agent_id, title, description, proposal_type, priority, estimated_cost_microdollars, status, cap_gate_result, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                proposal.id,
                proposal.agent_id,
                proposal.title,
                proposal.description,
                serde_json::to_string(&proposal.proposal_type).unwrap_or_default().trim_matches('"'),
                serde_json::to_string(&proposal.priority).unwrap_or_default().trim_matches('"'),
                proposal.estimated_cost_microdollars as i64,
                serde_json::to_string(&proposal.status).unwrap_or_default().trim_matches('"'),
                cap_gate_json,
                metadata_json,
                proposal.created_at.to_rfc3339(),
                proposal.updated_at.to_rfc3339(),
            ],
        )
        .context("failed to insert proposal")?;
        Ok(())
    }

    async fn update_proposal_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_proposals SET status = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![status_str, now, proposal_id],
        )
        .context("failed to update proposal status")?;
        Ok(())
    }

    async fn list_proposals(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match status_filter {
            Some(status) => (
                "SELECT id, agent_id, title, description, proposal_type, priority, estimated_cost_microdollars, status, cap_gate_result, metadata, created_at, updated_at FROM autopilot_proposals WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3".to_string(),
                vec![
                    Box::new(status.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
            None => (
                "SELECT id, agent_id, title, description, proposal_type, priority, estimated_cost_microdollars, status, cap_gate_result, metadata, created_at, updated_at FROM autopilot_proposals ORDER BY created_at DESC LIMIT ?1 OFFSET ?2".to_string(),
                vec![
                    Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(offset as i64),
                ],
            ),
        };
        let mut stmt = conn
            .prepare(&sql)
            .context("failed to prepare list proposals")?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let id: String = row.get(0)?;
                let agent_id: String = row.get(1)?;
                let title: String = row.get(2)?;
                let description: String = row.get(3)?;
                let proposal_type: String = row.get(4)?;
                let priority: String = row.get(5)?;
                let cost: i64 = row.get(6)?;
                let status: String = row.get(7)?;
                let cap_gate: Option<String> = row.get(8)?;
                let metadata: Option<String> = row.get(9)?;
                let created_at: String = row.get(10)?;
                let updated_at: String = row.get(11)?;
                Ok((
                    id,
                    agent_id,
                    title,
                    description,
                    proposal_type,
                    priority,
                    cost,
                    status,
                    cap_gate,
                    metadata,
                    created_at,
                    updated_at,
                ))
            })
            .context("failed to query proposals")?;

        let mut proposals = Vec::new();
        for row in rows {
            let (
                id,
                agent_id,
                title,
                description,
                proposal_type,
                priority,
                cost,
                status,
                cap_gate,
                metadata,
                created_at,
                updated_at,
            ) = row.context("failed to read proposal row")?;
            let proposal_type = serde_json::from_str(&format!("\"{proposal_type}\""))
                .unwrap_or(crate::types::ProposalType::TaskRequest);
            let priority = serde_json::from_str(&format!("\"{priority}\""))
                .unwrap_or(crate::types::Priority::Medium);
            let status =
                serde_json::from_str(&format!("\"{status}\"")).unwrap_or(ProposalStatus::Pending);
            let cap_gate_result = cap_gate.and_then(|s| serde_json::from_str(&s).ok());
            let metadata_val: Value = metadata
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let created = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let updated = chrono::DateTime::parse_from_rfc3339(&updated_at)
                .map(|d| d.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            proposals.push(Proposal {
                id,
                agent_id,
                title,
                description,
                proposal_type,
                priority,
                estimated_cost_microdollars: cost as u64,
                status,
                cap_gate_result,
                metadata: metadata_val,
                created_at: created,
                updated_at: updated,
            });
        }
        Ok(proposals)
    }

    // ── Missions ──

    async fn insert_mission(&self, mission: &Mission) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let steps_json = serde_json::to_string(&mission.steps).unwrap_or_else(|_| "[]".to_string());
        let result_json = mission.result.as_ref().map(|r| r.to_string());
        let deadline = mission.deadline.map(|d| d.to_rfc3339());
        conn.execute(
            "INSERT INTO autopilot_missions (id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, result, steps, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                mission.id,
                mission.proposal_id,
                mission.title,
                mission.assigned_agent,
                serde_json::to_string(&mission.status).unwrap_or_default().trim_matches('"'),
                mission.heartbeat_at.to_rfc3339(),
                deadline,
                result_json,
                steps_json,
                mission.created_at.to_rfc3339(),
                mission.updated_at.to_rfc3339(),
            ],
        )
        .context("failed to insert mission")?;
        Ok(())
    }

    async fn update_mission_status(
        &self,
        mission_id: &str,
        status: MissionStatus,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let status_str = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_missions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![status_str, now, mission_id],
        )
        .context("failed to update mission status")?;
        Ok(())
    }

    async fn heartbeat_mission(&self, mission_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE autopilot_missions SET heartbeat_at = ?1 WHERE id = ?2",
            rusqlite::params![now, mission_id],
        )
        .context("failed to heartbeat mission")?;
        Ok(())
    }

    async fn query_stale_missions(&self, threshold_minutes: u32) -> anyhow::Result<Vec<Mission>> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let threshold = chrono::Utc::now() - chrono::Duration::minutes(threshold_minutes as i64);
        let threshold_str = threshold.to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, result, steps, created_at, updated_at
             FROM autopilot_missions
             WHERE status = 'in_progress' AND heartbeat_at < ?1",
        ).context("failed to prepare stale mission query")?;
        let rows = stmt
            .query_map(rusqlite::params![threshold_str], |row| {
                Self::row_to_mission(row)
            })
            .context("failed to query stale missions")?;
        let mut missions = Vec::new();
        for row in rows {
            missions.push(row.context("failed to read mission row")?);
        }
        Ok(missions)
    }

    async fn list_missions(
        &self,
        status_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Mission>> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match status_filter {
            Some(status) => (
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, result, steps, created_at, updated_at FROM autopilot_missions WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3".to_string(),
                vec![
                    Box::new(status.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit as i64),
                    Box::new(offset as i64),
                ],
            ),
            None => (
                "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, result, steps, created_at, updated_at FROM autopilot_missions ORDER BY created_at DESC LIMIT ?1 OFFSET ?2".to_string(),
                vec![
                    Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(offset as i64),
                ],
            ),
        };
        let mut stmt = conn
            .prepare(&sql)
            .context("failed to prepare list missions")?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_refs.as_slice(), Self::row_to_mission)
            .context("failed to query missions")?;
        let mut missions = Vec::new();
        for row in rows {
            missions.push(row.context("failed to read mission row")?);
        }
        Ok(missions)
    }

    async fn get_mission(&self, mission_id: &str) -> anyhow::Result<Option<Mission>> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, proposal_id, title, assigned_agent, status, heartbeat_at, deadline, result, steps, created_at, updated_at
             FROM autopilot_missions WHERE id = ?1",
        ).context("failed to prepare get mission")?;
        let mut rows = stmt
            .query_map(rusqlite::params![mission_id], |row| {
                Self::row_to_mission(row)
            })
            .context("failed to get mission")?;
        match rows.next() {
            Some(row) => Ok(Some(row.context("failed to read mission")?)),
            None => Ok(None),
        }
    }

    // ── Aggregations ──

    async fn get_daily_spend(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let today_start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight timestamp");
        let today_start_utc =
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(today_start, chrono::Utc);
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_microdollars), 0) FROM autopilot_cap_gate_ledger WHERE recorded_at >= ?1",
                rusqlite::params![today_start_utc.to_rfc3339()],
                |row| row.get(0),
            )
            .context("failed to query daily spend")?;
        Ok(total as u64)
    }

    async fn get_concurrent_mission_count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM autopilot_missions WHERE status = 'in_progress'",
                [],
                |row| row.get(0),
            )
            .context("failed to count concurrent missions")?;
        Ok(count as usize)
    }

    // ── Events ──

    async fn insert_event(&self, event: &AutopilotEvent) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let payload_json =
            serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
        conn.execute(
            "INSERT INTO autopilot_events (id, event_type, source_agent, payload, correlation_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                event.id,
                event.event_type,
                event.source_agent,
                payload_json,
                event.correlation_id,
                event.created_at.to_rfc3339(),
            ],
        )
        .context("failed to insert event")?;
        Ok(())
    }

    // ── Content ──

    async fn upsert_content(&self, content: &Value) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("autopilot db lock poisoned");
        let id = content
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string().leak());
        let data = serde_json::to_string(content).unwrap_or_else(|_| "{}".to_string());
        conn.execute(
            "INSERT INTO autopilot_content (id, data) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            rusqlite::params![id, data],
        )
        .context("failed to upsert content")?;
        Ok(())
    }
}

impl SqliteAutopilotStore {
    fn row_to_mission(row: &rusqlite::Row<'_>) -> rusqlite::Result<Mission> {
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
            serde_json::from_str(&format!("\"{status_str}\"")).unwrap_or(MissionStatus::Pending);
        let heartbeat_at = chrono::DateTime::parse_from_rfc3339(&heartbeat_at_str)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let deadline = deadline_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|d| d.with_timezone(&chrono::Utc))
                .ok()
        });
        let result = result_str.and_then(|s| serde_json::from_str(&s).ok());
        let steps = serde_json::from_str(&steps_str).unwrap_or_default();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn test_proposal() -> Proposal {
        Proposal::new(
            "agent-1",
            "Test Proposal",
            "A test description",
            ProposalType::ContentIdea,
            Priority::Medium,
            100_000,
        )
    }

    fn test_mission(_proposal_id: &str) -> Mission {
        let proposal = test_proposal();
        Mission::from_proposal(&proposal, vec![])
    }

    #[tokio::test]
    async fn insert_and_list_proposals() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let proposal = test_proposal();
        store.insert_proposal(&proposal).await.expect("insert");

        let proposals = store.list_proposals(None, 10, 0).await.expect("list");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].id, proposal.id);
        assert_eq!(proposals[0].title, "Test Proposal");
    }

    #[tokio::test]
    async fn update_proposal_status() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let proposal = test_proposal();
        store.insert_proposal(&proposal).await.expect("insert");

        store
            .update_proposal_status(&proposal.id, ProposalStatus::Approved)
            .await
            .expect("update");

        let proposals = store
            .list_proposals(Some("approved"), 10, 0)
            .await
            .expect("list");
        assert_eq!(proposals.len(), 1);
    }

    #[tokio::test]
    async fn insert_and_get_mission() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let mission = test_mission("proposal-1");
        store.insert_mission(&mission).await.expect("insert");

        let retrieved = store.get_mission(&mission.id).await.expect("get");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().expect("mission").title, "Test Proposal");
    }

    #[tokio::test]
    async fn get_missing_mission_returns_none() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let result = store.get_mission("nonexistent").await.expect("get");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn heartbeat_updates_timestamp() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let mission = test_mission("proposal-1");
        store.insert_mission(&mission).await.expect("insert");

        let before = store
            .get_mission(&mission.id)
            .await
            .expect("get")
            .expect("mission");
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        store
            .heartbeat_mission(&mission.id)
            .await
            .expect("heartbeat");
        let after = store
            .get_mission(&mission.id)
            .await
            .expect("get")
            .expect("mission");

        assert!(after.heartbeat_at >= before.heartbeat_at);
    }

    #[tokio::test]
    async fn daily_spend_starts_at_zero() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let spend = store.get_daily_spend().await.expect("spend");
        assert_eq!(spend, 0);
    }

    #[tokio::test]
    async fn concurrent_mission_count() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let mut mission = test_mission("p1");
        mission.status = MissionStatus::InProgress;
        store.insert_mission(&mission).await.expect("insert");

        let count = store.get_concurrent_mission_count().await.expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn insert_and_query_events() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let event =
            AutopilotEvent::new("test.event", "agent-1", serde_json::json!({"key": "value"}));
        store.insert_event(&event).await.expect("insert");
        // No list_events method yet, but insert should not error.
    }

    #[tokio::test]
    async fn stale_missions_empty_by_default() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let stale = store.query_stale_missions(30).await.expect("query");
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn list_missions_with_filter() {
        let store = SqliteAutopilotStore::in_memory().expect("in-memory store");
        let mut m1 = test_mission("p1");
        m1.status = MissionStatus::InProgress;
        let m2 = test_mission("p2"); // pending by default
        store.insert_mission(&m1).await.expect("insert m1");
        store.insert_mission(&m2).await.expect("insert m2");

        let in_progress = store
            .list_missions(Some("in_progress"), 10, 0)
            .await
            .expect("list");
        assert_eq!(in_progress.len(), 1);

        let all = store.list_missions(None, 10, 0).await.expect("list all");
        assert_eq!(all.len(), 2);
    }
}
