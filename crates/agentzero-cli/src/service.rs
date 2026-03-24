use agentzero_storage::EncryptedJsonStore;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::path::Path;

const STATE_FILE: &str = "service-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceManager {
    store: EncryptedJsonStore,
}

impl ServiceManager {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir.as_ref(), STATE_FILE)?;
        Ok(Self { store })
    }

    pub fn install(&self) -> anyhow::Result<ServiceStatus> {
        let mut status = self.store.load_or_default::<ServiceStatus>()?;
        status.installed = true;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn start(&self) -> anyhow::Result<ServiceStatus> {
        let mut status = self.store.load_or_default::<ServiceStatus>()?;
        if !status.installed {
            bail!("service is not installed; run `agentzero service install`");
        }

        status.running = true;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn stop(&self) -> anyhow::Result<ServiceStatus> {
        let mut status = self.store.load_or_default::<ServiceStatus>()?;
        if !status.installed {
            bail!("service is not installed; run `agentzero service install`");
        }

        status.running = false;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn restart(&self) -> anyhow::Result<ServiceStatus> {
        let mut status = self.store.load_or_default::<ServiceStatus>()?;
        if !status.installed {
            bail!("service is not installed; run `agentzero service install`");
        }

        status.running = true;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn uninstall(&self) -> anyhow::Result<ServiceStatus> {
        let mut status = self.store.load_or_default::<ServiceStatus>()?;
        status.installed = false;
        status.running = false;
        self.store.save(&status)?;
        Ok(status)
    }

    pub fn status(&self) -> anyhow::Result<ServiceStatus> {
        self.store.load_or_default::<ServiceStatus>()
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceManager;
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
        let dir = std::env::temp_dir().join(format!(
            "agentzero-service-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn install_start_stop_status_success_path() {
        let dir = temp_dir();
        let manager = ServiceManager::new(&dir).expect("manager should be created");

        let status = manager.install().expect("install should succeed");
        assert!(status.installed);
        assert!(!status.running);

        let status = manager.start().expect("start should succeed");
        assert!(status.installed);
        assert!(status.running);

        let status = manager.status().expect("status should succeed");
        assert!(status.installed);
        assert!(status.running);

        let status = manager.stop().expect("stop should succeed");
        assert!(status.installed);
        assert!(!status.running);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn start_without_install_fails_negative_path() {
        let dir = temp_dir();
        let manager = ServiceManager::new(&dir).expect("manager should be created");

        let err = manager
            .start()
            .expect_err("start should fail when service is not installed");
        assert!(err.to_string().contains("service is not installed"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn restart_without_install_fails_negative_path() {
        let dir = temp_dir();
        let manager = ServiceManager::new(&dir).expect("manager should be created");

        let err = manager
            .restart()
            .expect_err("restart should fail when service is not installed");
        assert!(err.to_string().contains("service is not installed"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn uninstall_clears_install_and_running_success_path() {
        let dir = temp_dir();
        let manager = ServiceManager::new(&dir).expect("manager should be created");

        manager.install().expect("install should succeed");
        manager.start().expect("start should succeed");

        let status = manager.uninstall().expect("uninstall should succeed");
        assert!(!status.installed);
        assert!(!status.running);

        let _ = fs::remove_dir_all(dir);
    }
}
