use agentzero_core::security::redaction::redact_text;
use agentzero_core::{AuditEvent, AuditSink};
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

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

        let payload = json!({
            "ts_ms": ts_ms,
            "stage": event.stage,
            "detail": event.detail,
        });

        let line = redact_text(&payload.to_string());
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .with_context(|| format!("failed to open audit log file {}", self.path.display()))?;
        file.write_all(line.as_bytes())
            .await
            .context("failed to write audit event")?;
        file.write_all(b"\n")
            .await
            .context("failed to write audit newline")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FileAuditSink;
    use agentzero_core::{AuditEvent, AuditSink};
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-audit-{}-{nanos}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn writes_audit_event_to_file() {
        let dir = temp_dir();
        let path = dir.join("audit.log");
        let sink = FileAuditSink::new(path.clone());

        sink.record(AuditEvent {
            stage: "tool_execute_start".to_string(),
            detail: json!({"tool":"shell"}),
        })
        .await
        .expect("audit write should succeed");

        let content = fs::read_to_string(&path).expect("audit file should be readable");
        assert!(content.contains("tool_execute_start"));

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn fails_when_audit_path_is_directory() {
        let dir = temp_dir();
        let sink = FileAuditSink::new(dir.clone());

        let result = sink
            .record(AuditEvent {
                stage: "tool_execute_start".to_string(),
                detail: json!({"tool":"shell"}),
            })
            .await;

        assert!(result.is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
