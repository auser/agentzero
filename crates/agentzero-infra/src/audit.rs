use agentzero_core::security::redaction::redact_text;
use agentzero_core::{AuditEvent, AuditSink};
use agentzero_storage::crypto::sha256_hex;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Wraps an inner `AuditSink` and stamps every event with a monotonically
/// increasing sequence number and a fixed session ID before forwarding.
pub struct SequencedAuditSink {
    inner: Box<dyn AuditSink>,
    counter: Arc<AtomicU64>,
    session_id: String,
}

impl SequencedAuditSink {
    pub fn new(inner: Box<dyn AuditSink>, session_id: String) -> Self {
        Self {
            inner,
            counter: Arc::new(AtomicU64::new(1)),
            session_id,
        }
    }

    /// Returns a clone of the counter for sharing with hook sinks or sub-components.
    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.counter)
    }

    /// Returns the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[async_trait]
impl AuditSink for SequencedAuditSink {
    async fn record(&self, mut event: AuditEvent) -> anyhow::Result<()> {
        event.seq = self.counter.fetch_add(1, Ordering::Relaxed);
        event.session_id.clone_from(&self.session_id);
        self.inner.record(event).await
    }
}

pub struct FileAuditSink {
    path: PathBuf,
}

impl FileAuditSink {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl AuditSink for FileAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_millis();

        // Build the core payload (without hash) first, then hash it for
        // tamper detection.  The hash covers everything except ts_ms (which
        // is non-deterministic and must not be part of the integrity check).
        let core = json!({
            "seq": event.seq,
            "session_id": event.session_id,
            "stage": event.stage,
            "detail": event.detail.to_value(),
        });
        let content_hash = sha256_hex(core.to_string().as_bytes());
        let payload = json!({
            "ts_ms": ts_ms,
            "seq": event.seq,
            "session_id": event.session_id,
            "stage": event.stage,
            "detail": event.detail.to_value(),
            "content_hash": content_hash,
        });

        let mut line = redact_text(&payload.to_string());
        line.push('\n');
        let path = self.path.clone();
        // Use spawn_blocking with std::fs for truly atomic O_APPEND writes.
        // tokio async writes may split across syscalls, breaking atomicity.
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to open audit log file {}", path.display()))?;
            file.write_all(line.as_bytes())
                .context("failed to write audit event")?;
            Ok(())
        })
        .await
        .context("audit write task panicked")??;
        Ok(())
    }
}

/// Encrypted audit sink — each JSON line is encrypted with XChaCha20Poly1305
/// before writing.  One encrypted envelope per line (base64-encoded).
///
/// Use for production deployments where audit logs may be stored on shared
/// infrastructure.  The `FileAuditSink` remains available for local dev.
pub struct EncryptedFileAuditSink {
    path: PathBuf,
    key: [u8; 32],
}

impl EncryptedFileAuditSink {
    pub fn new(path: PathBuf, key: [u8; 32]) -> Self {
        Self { path, key }
    }
}

#[async_trait]
impl AuditSink for EncryptedFileAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before unix epoch")?
            .as_millis();

        let payload = json!({
            "ts_ms": ts_ms,
            "seq": event.seq,
            "session_id": event.session_id,
            "stage": event.stage,
            "detail": event.detail.to_value(),
        });

        let plaintext = redact_text(&payload.to_string());
        let key = self.key;
        let encrypted = agentzero_storage::crypto::encrypt_json(key, plaintext.as_bytes())
            .context("failed to encrypt audit event")?;

        // encrypt_json returns pretty-printed JSON — compact it to one line.
        let envelope: serde_json::Value = serde_json::from_slice(&encrypted)
            .context("encrypted envelope should be valid JSON")?;
        let mut line =
            serde_json::to_string(&envelope).context("failed to re-serialize envelope")?;
        line.push('\n');

        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to open audit log file {}", path.display()))?;
            file.write_all(line.as_bytes())
                .context("failed to write encrypted audit event")?;
            Ok(())
        })
        .await
        .context("encrypted audit write task panicked")??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FileAuditSink;
    use agentzero_core::{AuditEvent, AuditSink};
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn writes_audit_event_to_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("audit.log");
        let sink = FileAuditSink::new(path.clone());

        sink.record(AuditEvent {
            seq: 0,
            session_id: String::new(),
            stage: "tool_execute_start".to_string(),
            detail: json!({"tool":"shell"}).into(),
        })
        .await
        .expect("audit write should succeed");

        let content = fs::read_to_string(&path).expect("audit file should be readable");
        assert!(content.contains("tool_execute_start"));
    }

    #[tokio::test]
    async fn sequenced_sink_stamps_monotonic_seq() {
        use super::SequencedAuditSink;

        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("audit.log");
        let inner = Box::new(FileAuditSink::new(path.clone())) as Box<dyn AuditSink>;
        let sink = SequencedAuditSink::new(inner, "test-session-1".to_string());

        for _ in 0..5 {
            sink.record(AuditEvent {
                seq: 0,
                session_id: String::new(),
                stage: "tool_call".to_string(),
                detail: json!({"tool": "shell"}).into(),
            })
            .await
            .expect("sequenced audit write should succeed");
        }

        let content = fs::read_to_string(&path).expect("audit file should be readable");
        let lines: Vec<serde_json::Value> = content
            .lines()
            .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
            .collect();

        assert_eq!(lines.len(), 5);
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(
                line["seq"],
                (i as u64) + 1,
                "seq should be monotonic starting at 1"
            );
            assert_eq!(line["session_id"], "test-session-1");
        }
    }

    #[tokio::test]
    async fn sequenced_sink_concurrent_ordering() {
        use super::SequencedAuditSink;
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("audit.log");
        let inner = Box::new(FileAuditSink::new(path.clone())) as Box<dyn AuditSink>;
        let sink = Arc::new(SequencedAuditSink::new(
            inner,
            "concurrent-session".to_string(),
        ));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let sink = Arc::clone(&sink);
            handles.push(tokio::spawn(async move {
                sink.record(AuditEvent {
                    seq: 0,
                    session_id: String::new(),
                    stage: "concurrent_call".to_string(),
                    detail: json!({}).into(),
                })
                .await
                .expect("concurrent audit write should succeed");
            }));
        }
        for h in handles {
            h.await.expect("task should complete");
        }

        let content = fs::read_to_string(&path).expect("audit file should be readable");
        let mut seqs: Vec<u64> = content
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).expect("valid JSON");
                v["seq"].as_u64().expect("seq should be u64")
            })
            .collect();

        assert_eq!(seqs.len(), 10);
        seqs.sort();
        seqs.dedup();
        assert_eq!(seqs.len(), 10, "all sequence numbers should be unique");
        assert_eq!(seqs[0], 1);
        assert_eq!(seqs[9], 10);
    }

    #[tokio::test]
    async fn fails_when_audit_path_is_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let sink = FileAuditSink::new(dir.path().to_path_buf());

        let result = sink
            .record(AuditEvent {
                seq: 0,
                session_id: String::new(),
                stage: "tool_execute_start".to_string(),
                detail: json!({"tool":"shell"}).into(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn encrypted_sink_round_trips() {
        use super::EncryptedFileAuditSink;

        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("audit.enc.log");
        let key = [42_u8; 32];
        let sink = EncryptedFileAuditSink::new(path.clone(), key);

        sink.record(AuditEvent {
            seq: 1,
            session_id: "enc-test".to_string(),
            stage: "tool_execute_start".to_string(),
            detail: json!({"tool":"shell"}).into(),
        })
        .await
        .expect("encrypted audit write should succeed");

        // Raw file should NOT contain plaintext stage name.
        let raw = fs::read_to_string(&path).expect("file should be readable");
        assert!(
            !raw.contains("tool_execute_start"),
            "plaintext should not appear in encrypted log"
        );

        // Decrypt and verify.
        let line = raw.lines().next().expect("should have one line");
        let envelope: serde_json::Value =
            serde_json::from_str(line).expect("line should be valid JSON envelope");
        assert!(envelope.get("v").is_some(), "envelope should have version");
        assert!(
            envelope.get("nonce").is_some(),
            "envelope should have nonce"
        );

        // Full round-trip via decrypt_json.
        let envelope_bytes = serde_json::to_vec(&envelope).expect("re-serialize envelope");
        let plaintext = agentzero_storage::crypto::decrypt_json(key, &envelope_bytes)
            .expect("decryption should succeed");
        let event: serde_json::Value =
            serde_json::from_slice(&plaintext).expect("decrypted should be valid JSON");
        assert_eq!(event["stage"], "tool_execute_start");
        assert_eq!(event["session_id"], "enc-test");
    }
}
