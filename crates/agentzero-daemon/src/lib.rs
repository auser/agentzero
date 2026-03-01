use agentzero_storage::EncryptedJsonStore;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = "daemon_state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonStatus {
    pub running: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub started_at_epoch_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DaemonManager {
    store: EncryptedJsonStore,
}

impl DaemonManager {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir.as_ref(), STATE_FILE)?;
        Ok(Self { store })
    }

    pub fn mark_started(&self, host: String, port: u16) -> anyhow::Result<DaemonStatus> {
        let mut status = self.store.load_or_default::<DaemonStatus>()?;
        if status.running {
            bail!("daemon is already running");
        }

        status.running = true;
        status.host = Some(host);
        status.port = Some(port);
        status.started_at_epoch_seconds = Some(current_epoch_seconds());
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn mark_stopped(&self) -> anyhow::Result<DaemonStatus> {
        let mut status = self.store.load_or_default::<DaemonStatus>()?;
        if !status.running {
            bail!("daemon is not running");
        }

        status.running = false;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn status(&self) -> anyhow::Result<DaemonStatus> {
        self.store.load_or_default::<DaemonStatus>()
    }
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::DaemonManager;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-daemon-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn mark_started_then_stopped_success_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");

        let started = manager
            .mark_started("127.0.0.1".to_string(), 8080)
            .expect("mark_started should succeed");
        assert!(started.running);
        assert_eq!(started.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(started.port, Some(8080));
        assert!(started.started_at_epoch_seconds.is_some());

        let stopped = manager.mark_stopped().expect("mark_stopped should succeed");
        assert!(!stopped.running);
        assert_eq!(stopped.host.as_deref(), Some("127.0.0.1"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn mark_started_rejects_double_start_negative_path() {
        let dir = temp_dir();
        let manager = DaemonManager::new(&dir).expect("manager should be created");
        manager
            .mark_started("127.0.0.1".to_string(), 8080)
            .expect("first start should succeed");

        let err = manager
            .mark_started("127.0.0.1".to_string(), 8080)
            .expect_err("second start should fail");
        assert!(err.to_string().contains("already running"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
