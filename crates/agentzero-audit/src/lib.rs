//! Audit event logging for AgentZero.
//!
//! Every meaningful action emits a structured audit event (ADR 0003).
//! Raw secrets must never appear in audit logs.

mod encrypted;
mod sink;

use std::io::Write;
use std::path::{Path, PathBuf};

use agentzero_core::AuditEvent;
use thiserror::Error;

pub use encrypted::EncryptedAuditLogger;
pub use sink::{AuditSink, InMemorySink};

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("failed to initialize audit log: {0}")]
    InitFailed(String),
    #[error("failed to write audit event: {0}")]
    WriteFailed(#[from] std::io::Error),
    #[error("failed to serialize audit event: {0}")]
    SerializeFailed(#[from] serde_json::Error),
}

/// Audit logger that writes structured JSONL events to a session log file.
pub struct AuditLogger {
    path: PathBuf,
}

impl AuditLogger {
    /// Create a new audit logger writing to the given directory.
    ///
    /// The session file will be created as `<session_id>.jsonl` inside `dir`.
    pub fn new(dir: &Path, session_id: &str) -> Result<Self, AuditError> {
        std::fs::create_dir_all(dir).map_err(|e| AuditError::InitFailed(e.to_string()))?;
        let path = dir.join(format!("{session_id}.jsonl"));
        Ok(Self { path })
    }

    /// Record an audit event.
    pub fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let line = serde_json::to_string(event)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Read all events from the log file.
    pub fn read_all(&self) -> Result<Vec<AuditEvent>, AuditError> {
        let content = std::fs::read_to_string(&self.path).map_err(AuditError::WriteFailed)?;
        let mut events = Vec::new();
        for line in content.lines() {
            if !line.trim().is_empty() {
                let event: AuditEvent = serde_json::from_str(line)?;
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Read the last N events from the log file.
    pub fn tail(&self, count: usize) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.read_all()?;
        let start = all.len().saturating_sub(count);
        Ok(all[start..].to_vec())
    }

    /// Return the path to the audit log file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AuditSink for AuditLogger {
    fn record(&self, event: &AuditEvent) -> Result<(), String> {
        AuditLogger::record(self, event).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{
        Capability, DataClassification, ExecutionId, PolicyDecision, RuntimeTier, SessionId,
    };
    use chrono::Utc;

    fn sample_event() -> AuditEvent {
        AuditEvent {
            execution_id: ExecutionId::new(),
            session_id: SessionId::from_string("test-session"),
            timestamp: Utc::now(),
            action: "file_read".into(),
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            decision: PolicyDecision::Allow,
            reason: "policy allows private file reads".into(),
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
            tool_id: None,
            redactions_applied: vec![],
            approval_scope: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn logger_creates_file_and_records_event() {
        let dir = temp_dir("audit-test");
        let logger = AuditLogger::new(&dir, "test-session").expect("logger should initialize");
        let event = sample_event();
        logger.record(&event).expect("record should succeed");

        let content = std::fs::read_to_string(logger.path()).expect("file should exist");
        assert!(content.contains("file_read"));
        assert!(content.contains("test-session"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn logger_reads_all_events() {
        let dir = temp_dir("audit-read-all");
        let logger = AuditLogger::new(&dir, "test-session").expect("logger should initialize");

        for _ in 0..3 {
            logger.record(&sample_event()).expect("should record");
        }

        let events = logger.read_all().expect("should read all");
        assert_eq!(events.len(), 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn logger_tail_returns_last_n() {
        let dir = temp_dir("audit-tail");
        let logger = AuditLogger::new(&dir, "test-session").expect("logger should initialize");

        for _ in 0..5 {
            logger.record(&sample_event()).expect("should record");
        }

        let events = logger.tail(2).expect("should tail");
        assert_eq!(events.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn logger_tail_with_more_than_available() {
        let dir = temp_dir("audit-tail-overflow");
        let logger = AuditLogger::new(&dir, "test-session").expect("logger should initialize");

        logger.record(&sample_event()).expect("should record");

        let events = logger.tail(100).expect("should tail");
        assert_eq!(events.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
