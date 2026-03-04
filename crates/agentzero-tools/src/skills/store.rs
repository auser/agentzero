use super::skillforge::validate_skill_name;
use agentzero_storage::EncryptedJsonStore;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SKILLS_FILE: &str = "skills-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRecord {
    pub name: String,
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct SkillStore {
    store: EncryptedJsonStore,
}

impl SkillStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            store: EncryptedJsonStore::in_config_dir(data_dir.as_ref(), SKILLS_FILE)?,
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<SkillRecord>> {
        self.store.load_or_default()
    }

    pub fn install(&self, name: &str, source: &str) -> anyhow::Result<SkillRecord> {
        validate_skill_name(name)?;
        if source.trim().is_empty() {
            bail!("skill source cannot be empty");
        }

        let mut skills = self.list()?;
        if skills.iter().any(|skill| skill.name == name) {
            bail!("skill `{name}` already installed");
        }

        let record = SkillRecord {
            name: name.to_string(),
            source: source.to_string(),
            enabled: true,
        };
        skills.push(record.clone());
        self.store.save(&skills)?;
        Ok(record)
    }

    pub fn get(&self, name: &str) -> anyhow::Result<SkillRecord> {
        let skills = self.list()?;
        skills
            .into_iter()
            .find(|skill| skill.name == name)
            .with_context(|| format!("skill `{name}` is not installed"))
    }

    pub fn remove(&self, name: &str) -> anyhow::Result<()> {
        let mut skills = self.list()?;
        let previous_len = skills.len();
        skills.retain(|skill| skill.name != name);
        if skills.len() == previous_len {
            bail!("skill `{name}` is not installed");
        }
        self.store.save(&skills)?;
        Ok(())
    }

    pub fn test(&self, name: &str) -> anyhow::Result<String> {
        let skills = self.list()?;
        let skill = skills
            .iter()
            .find(|skill| skill.name == name)
            .with_context(|| format!("skill `{name}` is not installed"))?;

        Ok(format!(
            "skill `{}` test: source={}, status={}",
            skill.name,
            skill.source,
            if skill.enabled { "enabled" } else { "disabled" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::SkillStore;
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
            "agentzero-skills-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn install_list_test_remove_success_path() {
        let dir = temp_dir();
        let store = SkillStore::new(&dir).expect("store should create");

        let installed = store
            .install("my_skill", "local")
            .expect("install should succeed");
        assert_eq!(installed.name, "my_skill");

        let listed = store.list().expect("list should succeed");
        assert_eq!(listed.len(), 1);

        let output = store.test("my_skill").expect("test should succeed");
        assert!(output.contains("status=enabled"));

        store.remove("my_skill").expect("remove should succeed");
        assert!(store.list().expect("list should succeed").is_empty());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn install_duplicate_fails_negative_path() {
        let dir = temp_dir();
        let store = SkillStore::new(&dir).expect("store should create");
        store
            .install("my_skill", "local")
            .expect("first install should succeed");

        let err = store
            .install("my_skill", "local")
            .expect_err("duplicate install should fail");
        assert!(err.to_string().contains("already installed"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
