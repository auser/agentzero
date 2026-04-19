use agentzero_storage::EncryptedJsonStore;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;

const TASKS_FILE: &str = "cron-tasks.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CronTask {
    pub id: String,
    pub schedule: String,
    pub command: String,
    pub enabled: bool,
    pub last_run_epoch_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CronStore {
    store: EncryptedJsonStore,
}

impl CronStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            store: EncryptedJsonStore::in_config_dir(data_dir.as_ref(), TASKS_FILE)?,
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<CronTask>> {
        self.store.load_or_default()
    }

    pub fn add(&self, id: &str, schedule: &str, command: &str) -> anyhow::Result<CronTask> {
        let mut tasks = self.list()?;
        if tasks.iter().any(|task| task.id == id) {
            bail!("task `{id}` already exists");
        }
        if id.trim().is_empty() || schedule.trim().is_empty() || command.trim().is_empty() {
            bail!("id, schedule, and command must be non-empty");
        }
        let task = CronTask {
            id: id.to_string(),
            schedule: schedule.to_string(),
            command: command.to_string(),
            enabled: true,
            last_run_epoch_seconds: None,
        };
        tasks.push(task.clone());
        self.store.save(&tasks)?;
        Ok(task)
    }

    pub fn update(
        &self,
        id: &str,
        schedule: Option<&str>,
        command: Option<&str>,
    ) -> anyhow::Result<CronTask> {
        let mut tasks = self.list()?;
        let task = tasks
            .iter_mut()
            .find(|task| task.id == id)
            .with_context(|| format!("task `{id}` not found"))?;

        if let Some(schedule) = schedule {
            if schedule.trim().is_empty() {
                bail!("schedule must be non-empty when provided");
            }
            task.schedule = schedule.to_string();
        }

        if let Some(command) = command {
            if command.trim().is_empty() {
                bail!("command must be non-empty when provided");
            }
            task.command = command.to_string();
        }

        let updated = task.clone();
        self.store.save(&tasks)?;
        Ok(updated)
    }

    pub fn pause(&self, id: &str) -> anyhow::Result<CronTask> {
        self.set_enabled(id, false)
    }

    pub fn resume(&self, id: &str) -> anyhow::Result<CronTask> {
        self.set_enabled(id, true)
    }

    pub fn remove(&self, id: &str) -> anyhow::Result<()> {
        let mut tasks = self.list()?;
        let before = tasks.len();
        tasks.retain(|task| task.id != id);
        if tasks.len() == before {
            bail!("task `{id}` not found");
        }
        self.store.save(&tasks)?;
        Ok(())
    }

    /// Mark a task as having just run, updating its `last_run_epoch_seconds`.
    pub fn mark_last_run(&self, id: &str, epoch_secs: u64) -> anyhow::Result<()> {
        let mut tasks = self.list()?;
        let task = tasks
            .iter_mut()
            .find(|task| task.id == id)
            .with_context(|| format!("task `{id}` not found"))?;
        task.last_run_epoch_seconds = Some(epoch_secs);
        self.store.save(&tasks)?;
        Ok(())
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<CronTask> {
        let mut tasks = self.list()?;
        let task = tasks
            .iter_mut()
            .find(|task| task.id == id)
            .with_context(|| format!("task `{id}` not found"))?;
        task.enabled = enabled;
        let updated = task.clone();
        self.store.save(&tasks)?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::CronStore;
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
            "agentzero-cron-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn add_update_pause_resume_remove_success_path() {
        let dir = temp_dir();
        let store = CronStore::new(&dir).expect("store should create");

        let task = store
            .add("backup", "0 * * * *", "agentzero status")
            .expect("add should succeed");
        assert!(task.enabled);

        let updated = store
            .update("backup", Some("*/5 * * * *"), None)
            .expect("update should succeed");
        assert_eq!(updated.schedule, "*/5 * * * *");

        let paused = store.pause("backup").expect("pause should succeed");
        assert!(!paused.enabled);

        let resumed = store.resume("backup").expect("resume should succeed");
        assert!(resumed.enabled);

        store.remove("backup").expect("remove should succeed");
        assert!(store.list().expect("list should succeed").is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_duplicate_id_fails_negative_path() {
        let dir = temp_dir();
        let store = CronStore::new(&dir).expect("store should create");
        store
            .add("backup", "0 * * * *", "agentzero status")
            .expect("first add should succeed");
        let err = store
            .add("backup", "0 * * * *", "agentzero status")
            .expect_err("duplicate add should fail");
        assert!(err.to_string().contains("already exists"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persistence_round_trip() {
        let dir = temp_dir();
        {
            let store = CronStore::new(&dir).expect("store");
            store.add("job-a", "0 * * * *", "cmd-a").expect("add");
            store.add("job-b", "*/5 * * * *", "cmd-b").expect("add");
        }
        // Reopen store from same dir — tasks should persist.
        let store = CronStore::new(&dir).expect("reopen");
        let tasks = store.list().expect("list");
        assert_eq!(tasks.len(), 2);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn remove_nonexistent_fails() {
        let dir = temp_dir();
        let store = CronStore::new(&dir).expect("store");
        let err = store.remove("ghost").expect_err("should fail");
        assert!(err.to_string().contains("not found"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn list_empty_returns_empty() {
        let dir = temp_dir();
        let store = CronStore::new(&dir).expect("store");
        let tasks = store.list().expect("list");
        assert!(tasks.is_empty());
        fs::remove_dir_all(dir).ok();
    }
}
