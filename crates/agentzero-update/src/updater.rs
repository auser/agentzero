use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateState {
    pub current_version: String,
    pub last_checked_epoch_secs: Option<u64>,
    pub last_target_version: Option<String>,
    pub last_applied_epoch_secs: Option<u64>,
    pub previous_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyUpdateResult {
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackUpdateResult {
    pub from_version: String,
    pub to_version: String,
}

pub fn load_state(
    state_path: impl AsRef<Path>,
    current_version: &str,
) -> anyhow::Result<UpdateState> {
    let store = state_store(state_path.as_ref())?;
    let mut state = store
        .load_optional::<UpdateState>()
        .with_context(|| format!("failed to parse update state at {}", store.path().display()))?
        .unwrap_or_else(|| UpdateState {
            current_version: current_version.to_string(),
            last_checked_epoch_secs: None,
            last_target_version: None,
            last_applied_epoch_secs: None,
            previous_versions: vec![],
        });
    if state.current_version.trim().is_empty() {
        state.current_version = current_version.to_string();
    }
    Ok(state)
}

pub fn save_state(state_path: impl AsRef<Path>, state: &UpdateState) -> anyhow::Result<()> {
    let store = state_store(state_path.as_ref())?;
    store
        .save(state)
        .with_context(|| format!("failed to write update state at {}", store.path().display()))
}

pub fn check_for_updates(
    state_path: impl AsRef<Path>,
    current_version: &str,
    latest_version_override: Option<&str>,
) -> anyhow::Result<UpdateCheckResult> {
    let mut state = load_state(state_path.as_ref(), current_version)?;
    let latest = latest_version_override
        .map(str::to_string)
        .unwrap_or_else(|| current_version.to_string());
    state.last_checked_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: latest.clone(),
        up_to_date: latest == current_version,
    })
}

pub fn apply_update(
    state_path: impl AsRef<Path>,
    current_version: &str,
    target_version: &str,
) -> anyhow::Result<ApplyUpdateResult> {
    if target_version.trim().is_empty() {
        return Err(anyhow!("target version cannot be empty"));
    }

    let mut state = load_state(state_path.as_ref(), current_version)?;
    if state.current_version == target_version {
        return Err(anyhow!(
            "target version already applied: {}",
            target_version
        ));
    }

    state.previous_versions.push(state.current_version.clone());
    state.current_version = target_version.to_string();
    state.last_target_version = Some(target_version.to_string());
    state.last_applied_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(ApplyUpdateResult {
        from_version: state
            .previous_versions
            .last()
            .cloned()
            .unwrap_or_else(|| current_version.to_string()),
        to_version: target_version.to_string(),
    })
}

pub fn rollback_update(
    state_path: impl AsRef<Path>,
    current_version: &str,
) -> anyhow::Result<RollbackUpdateResult> {
    let mut state = load_state(state_path.as_ref(), current_version)?;
    let Some(previous) = state.previous_versions.pop() else {
        return Err(anyhow!("no previous version available for rollback"));
    };

    let from = state.current_version.clone();
    state.current_version = previous.clone();
    state.last_target_version = Some(previous.clone());
    state.last_applied_epoch_secs = Some(now_epoch_secs());
    save_state(state_path, &state)?;

    Ok(RollbackUpdateResult {
        from_version: from,
        to_version: previous,
    })
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_secs()
}

fn state_store(state_path: &Path) -> anyhow::Result<EncryptedJsonStore> {
    let parent = state_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = state_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid update state path: {}", state_path.display()))?;
    EncryptedJsonStore::in_config_dir(parent, file_name)
}

#[cfg(test)]
mod tests {
    use super::{apply_update, check_for_updates, load_state, rollback_update};
    use std::fs;

    #[test]
    fn update_apply_and_rollback_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");

        let check =
            check_for_updates(&state_path, "0.1.0", Some("0.2.0")).expect("check should succeed");
        assert!(!check.up_to_date);

        let applied = apply_update(&state_path, "0.1.0", "0.2.0").expect("apply should succeed");
        assert_eq!(applied.from_version, "0.1.0");
        assert_eq!(applied.to_version, "0.2.0");

        let rolled = rollback_update(&state_path, "0.2.0").expect("rollback should succeed");
        assert_eq!(rolled.from_version, "0.2.0");
        assert_eq!(rolled.to_version, "0.1.0");

        let state = load_state(&state_path, "0.1.0").expect("state should load");
        assert_eq!(state.current_version, "0.1.0");
    }

    #[test]
    fn rollback_without_history_fails_negative_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");
        let err = rollback_update(&state_path, "0.1.0")
            .expect_err("rollback without history should fail");
        assert!(err.to_string().contains("no previous version"));
    }

    #[test]
    fn update_state_is_encrypted_at_rest_success_path() {
        let tmp = tempfile::tempdir().expect("temp dir should be created");
        let state_path = tmp.path().join("state.json");

        check_for_updates(&state_path, "0.1.0", Some("0.2.0")).expect("check should create state");
        apply_update(&state_path, "0.1.0", "0.2.0").expect("apply should update state");

        let on_disk = fs::read_to_string(&state_path).expect("state file should be readable");
        assert!(!on_disk.contains("0.2.0"));
        assert!(!on_disk.contains("current_version"));
    }
}
