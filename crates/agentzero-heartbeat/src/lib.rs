use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatRecord {
    pub component: String,
    pub last_seen_epoch_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct HeartbeatStore {
    data_dir: std::path::PathBuf,
}

impl HeartbeatStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
        }
    }

    pub fn touch(&self, component: &str, now_epoch_seconds: u64) -> anyhow::Result<()> {
        let store = self.store_for_component(component)?;
        store.save(&HeartbeatRecord {
            component: component.to_string(),
            last_seen_epoch_seconds: now_epoch_seconds,
        })
    }

    pub fn get(&self, component: &str) -> anyhow::Result<Option<HeartbeatRecord>> {
        let store = self.store_for_component(component)?;
        store.load_optional()
    }

    fn store_for_component(&self, component: &str) -> anyhow::Result<EncryptedJsonStore> {
        let safe = sanitize_component(component);
        EncryptedJsonStore::in_config_dir(&self.data_dir, &format!("heartbeat-{safe}.json"))
    }
}

fn sanitize_component(component: &str) -> String {
    component
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::HeartbeatStore;
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
        let dir = std::env::temp_dir().join(format!("agentzero-heartbeat-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn touch_then_get_round_trip_success_path() {
        let dir = temp_dir();
        let store = HeartbeatStore::new(&dir);

        store
            .touch("daemon", 123)
            .expect("touch should write heartbeat");
        let record = store
            .get("daemon")
            .expect("get should succeed")
            .expect("heartbeat should exist");
        assert_eq!(record.component, "daemon");
        assert_eq!(record.last_seen_epoch_seconds, 123);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn get_returns_none_when_missing_negative_path() {
        let dir = temp_dir();
        let store = HeartbeatStore::new(&dir);

        let record = store.get("channels").expect("get should succeed");
        assert!(record.is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
