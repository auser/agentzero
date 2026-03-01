use agentzero_storage::EncryptedJsonStore;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const HOOKS_FILE: &str = "hooks-state.json";

const DEFAULT_HOOKS: &[&str] = &[
    "before_run",
    "after_run",
    "before_provider_call",
    "after_provider_call",
    "before_tool_call",
    "after_tool_call",
    "before_plugin_call",
    "after_plugin_call",
    "before_memory_write",
    "after_memory_write",
    "before_response_emit",
    "after_response_emit",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookState {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HookStorage {
    hooks: BTreeMap<String, bool>,
}

#[derive(Debug, Clone)]
pub struct HookStore {
    store: EncryptedJsonStore,
}

impl HookStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            store: EncryptedJsonStore::in_config_dir(data_dir.as_ref(), HOOKS_FILE)?,
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<HookState>> {
        let storage: HookStorage = self.store.load_or_default()?;
        let mut out = Vec::with_capacity(DEFAULT_HOOKS.len());
        for name in DEFAULT_HOOKS {
            let enabled = *storage.hooks.get(*name).unwrap_or(&false);
            out.push(HookState {
                name: (*name).to_string(),
                enabled,
            });
        }
        Ok(out)
    }

    pub fn enable(&self, name: &str) -> anyhow::Result<HookState> {
        self.set(name, true)
    }

    pub fn disable(&self, name: &str) -> anyhow::Result<HookState> {
        self.set(name, false)
    }

    pub fn test(&self, name: &str) -> anyhow::Result<String> {
        ensure_known_hook(name)?;
        let state = self
            .list()?
            .into_iter()
            .find(|hook| hook.name == name)
            .context("hook lookup failed")?;
        Ok(format!(
            "hook `{}` test: {}",
            state.name,
            if state.enabled { "enabled" } else { "disabled" }
        ))
    }

    fn set(&self, name: &str, enabled: bool) -> anyhow::Result<HookState> {
        ensure_known_hook(name)?;
        let mut storage: HookStorage = self.store.load_or_default()?;
        storage.hooks.insert(name.to_string(), enabled);
        self.store.save(&storage)?;
        Ok(HookState {
            name: name.to_string(),
            enabled,
        })
    }
}

fn ensure_known_hook(name: &str) -> anyhow::Result<()> {
    if DEFAULT_HOOKS.contains(&name) {
        Ok(())
    } else {
        bail!("unknown hook `{name}`")
    }
}

#[cfg(test)]
mod tests {
    use super::HookStore;
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
        let dir = std::env::temp_dir().join(format!("agentzero-hooks-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn enable_disable_and_test_success_path() {
        let dir = temp_dir();
        let store = HookStore::new(&dir).expect("store should create");

        let enabled = store.enable("before_run").expect("enable should succeed");
        assert!(enabled.enabled);

        let output = store.test("before_run").expect("test should succeed");
        assert!(output.contains("enabled"));

        let disabled = store.disable("before_run").expect("disable should succeed");
        assert!(!disabled.enabled);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn enable_unknown_hook_fails_negative_path() {
        let dir = temp_dir();
        let store = HookStore::new(&dir).expect("store should create");
        let err = store
            .enable("unknown")
            .expect_err("unknown hook should fail");
        assert!(err.to_string().contains("unknown hook"));
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn list_includes_plugin_hook_points_success_path() {
        let dir = temp_dir();
        let store = HookStore::new(&dir).expect("store should create");

        let hooks = store.list().expect("list should succeed");
        let names = hooks.into_iter().map(|h| h.name).collect::<Vec<_>>();
        assert!(names.contains(&"before_plugin_call".to_string()));
        assert!(names.contains(&"after_plugin_call".to_string()));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
