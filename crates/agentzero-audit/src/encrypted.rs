//! Encrypted audit logger.
//!
//! Wraps the standard JSONL audit logger with AES-256-GCM encryption.
//! Each line is encrypted individually so the file can be appended to
//! without re-encrypting the entire log.

use std::io::Write;
use std::path::{Path, PathBuf};

use agentzero_core::crypto;
use agentzero_core::AuditEvent;
use agentzero_tracing::info;

use crate::sink::AuditSink;
use crate::AuditError;

/// Encrypted audit logger that writes AES-256-GCM encrypted JSONL.
///
/// Each line is independently encrypted and base64-encoded, so the file
/// can be appended to without re-encrypting previous events.
pub struct EncryptedAuditLogger {
    path: PathBuf,
    passphrase: String,
}

impl EncryptedAuditLogger {
    /// Create a new encrypted audit logger.
    pub fn new(dir: &Path, session_id: &str, passphrase: String) -> Result<Self, AuditError> {
        std::fs::create_dir_all(dir).map_err(|e| AuditError::InitFailed(e.to_string()))?;
        let path = dir.join(format!("{session_id}.jsonl.enc"));
        info!(path = %path.display(), "encrypted audit logger initialized");
        Ok(Self { path, passphrase })
    }

    /// Record an encrypted audit event.
    pub fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let json = serde_json::to_string(event)?;
        let encrypted = crypto::encrypt_string(&json, &self.passphrase)
            .map_err(|e| AuditError::InitFailed(format!("encryption failed: {e}")))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{encrypted}")?;
        Ok(())
    }

    /// Read and decrypt all events from the log file.
    pub fn read_all(&self) -> Result<Vec<AuditEvent>, AuditError> {
        let content = std::fs::read_to_string(&self.path).map_err(AuditError::WriteFailed)?;
        let mut events = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let json = crypto::decrypt_string(line, &self.passphrase)
                .map_err(|e| AuditError::InitFailed(format!("decryption failed: {e}")))?;
            let event: AuditEvent = serde_json::from_str(&json)?;
            events.push(event);
        }
        Ok(events)
    }

    /// Read the last N events.
    pub fn tail(&self, count: usize) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.read_all()?;
        let start = all.len().saturating_sub(count);
        Ok(all[start..].to_vec())
    }

    /// Return the path to the encrypted log file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AuditSink for EncryptedAuditLogger {
    fn record(&self, event: &AuditEvent) -> Result<(), String> {
        EncryptedAuditLogger::record(self, event).map_err(|e| e.to_string())
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
            session_id: SessionId::from_string("test-enc-session"),
            timestamp: Utc::now(),
            action: "encrypted_test".into(),
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            decision: PolicyDecision::Allow,
            reason: "test".into(),
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
            tool_id: None,
            redactions_applied: vec![],
            approval_scope: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-enc-audit-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn encrypted_roundtrip() {
        let dir = temp_dir("roundtrip");
        let logger =
            EncryptedAuditLogger::new(&dir, "test", "my-passphrase".into()).expect("should create");
        logger.record(&sample_event()).expect("should record");
        logger.record(&sample_event()).expect("should record");

        let events = logger.read_all().expect("should decrypt");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].action, "encrypted_test");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn encrypted_file_is_not_plaintext() {
        let dir = temp_dir("not-plaintext");
        let logger =
            EncryptedAuditLogger::new(&dir, "test", "secret".into()).expect("should create");
        logger.record(&sample_event()).expect("should record");

        let raw = std::fs::read_to_string(logger.path()).expect("should read");
        assert!(!raw.contains("encrypted_test"));
        assert!(!raw.contains("test-enc-session"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn encrypted_tail() {
        let dir = temp_dir("tail");
        let logger = EncryptedAuditLogger::new(&dir, "test", "pass".into()).expect("should create");
        for _ in 0..5 {
            logger.record(&sample_event()).expect("should record");
        }
        let events = logger.tail(2).expect("should tail");
        assert_eq!(events.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
