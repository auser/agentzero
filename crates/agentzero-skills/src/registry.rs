//! Skill registry and lockfile management.
//!
//! Tracks installed skills with versions, checksums, and permissions.
//! The lockfile at `.agentzero/skills.lock` ensures reproducible installs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry IO error: {0}")]
    IoError(String),
    #[error("registry parse error: {0}")]
    ParseError(String),
    #[error("skill not found: {0}")]
    NotFound(String),
}

/// A locked skill entry with version and integrity info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSkill {
    pub name: String,
    pub version: String,
    pub source: String,
    pub runtime: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub checksum: Option<String>,
}

/// Skill lockfile tracking installed skills.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillLockfile {
    pub version: u32,
    pub skills: BTreeMap<String, LockedSkill>,
}

impl SkillLockfile {
    /// Load lockfile from disk.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self {
                version: 1,
                skills: BTreeMap::new(),
            });
        }
        let content =
            std::fs::read_to_string(path).map_err(|e| RegistryError::IoError(e.to_string()))?;
        toml::from_str(&content).map_err(|e| RegistryError::ParseError(e.to_string()))
    }

    /// Save lockfile to disk.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| RegistryError::ParseError(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| RegistryError::IoError(e.to_string()))
    }

    /// Register a skill in the lockfile.
    pub fn register(&mut self, skill: LockedSkill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Remove a skill from the lockfile.
    pub fn remove(&mut self, name: &str) -> Option<LockedSkill> {
        self.skills.remove(name)
    }

    /// Check if a skill is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }
}

/// Scan the skills/ directory and build a registry of installed skills.
pub fn scan_installed(skills_dir: &Path) -> Result<Vec<LockedSkill>, RegistryError> {
    let mut skills = Vec::new();

    if !skills_dir.exists() {
        return Ok(skills);
    }

    let entries =
        std::fs::read_dir(skills_dir).map_err(|e| RegistryError::IoError(e.to_string()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let (version, runtime, permissions) = parse_skill_metadata(&skill_md);

        let source = if path.join(".git").exists() {
            "git".into()
        } else {
            "local".into()
        };

        skills.push(LockedSkill {
            name,
            version,
            source,
            runtime,
            permissions,
            checksum: None,
        });
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Parse basic metadata from SKILL.md frontmatter.
fn parse_skill_metadata(path: &Path) -> (String, String, Vec<String>) {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut version = "0.1.0".to_string();
    let mut runtime = "none".to_string();
    let mut permissions = Vec::new();

    // Simple frontmatter parsing
    if let Some(after_prefix) = content.strip_prefix("---") {
        if let Some(end) = after_prefix.find("---") {
            let frontmatter = &after_prefix[..end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("version:") {
                    version = v.trim().to_string();
                }
                if let Some(r) = line.strip_prefix("runtime:") {
                    runtime = r.trim().to_string();
                }
                if line.starts_with("- read") || line.starts_with("- write") {
                    permissions.push(line.trim_start_matches("- ").to_string());
                }
            }
        }
    }

    (version, runtime, permissions)
}

/// Default lockfile path for a project.
pub fn lockfile_path(project_root: &Path) -> PathBuf {
    project_root.join(".agentzero/skills.lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentzero-registry-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn lockfile_roundtrip() {
        let dir = temp_dir("lockfile");
        fs::create_dir_all(&dir).expect("should create dir");
        let path = dir.join("skills.lock");

        let mut lockfile = SkillLockfile {
            version: 1,
            skills: BTreeMap::new(),
        };
        lockfile.register(LockedSkill {
            name: "repo-security-audit".into(),
            version: "0.1.0".into(),
            source: "local".into(),
            runtime: "none".into(),
            permissions: vec!["read".into()],
            checksum: None,
        });

        lockfile.save(&path).expect("should save");
        let loaded = SkillLockfile::load(&path).expect("should load");
        assert_eq!(loaded.skills.len(), 1);
        assert!(loaded.contains("repo-security-audit"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_installed_skills() {
        let dir = temp_dir("scan");
        let skills_dir = dir.join("skills");
        let skill_path = skills_dir.join("test-skill");
        fs::create_dir_all(&skill_path).expect("should create");
        fs::write(
            skill_path.join("SKILL.md"),
            "---\nname: test-skill\nruntime: none\n---\n# Test\n",
        )
        .expect("should write");

        let skills = scan_installed(&skills_dir).expect("should scan");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test-skill");
        assert_eq!(skills[0].runtime, "none");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_lockfile_returns_empty() {
        let lockfile =
            SkillLockfile::load(Path::new("/nonexistent/skills.lock")).expect("should succeed");
        assert!(lockfile.skills.is_empty());
    }

    #[test]
    fn register_and_remove() {
        let mut lockfile = SkillLockfile::default();
        lockfile.register(LockedSkill {
            name: "test".into(),
            version: "1.0".into(),
            source: "git".into(),
            runtime: "wasm".into(),
            permissions: vec![],
            checksum: Some("sha256:abc".into()),
        });
        assert!(lockfile.contains("test"));
        lockfile.remove("test");
        assert!(!lockfile.contains("test"));
    }
}
