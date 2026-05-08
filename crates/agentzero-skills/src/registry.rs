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
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("integrity check failed for skill '{skill}': expected {expected}, got {actual}")]
    IntegrityFailed {
        skill: String,
        expected: String,
        actual: String,
    },
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
    /// SHA-256 hash of extracted directory contents (sorted file paths + contents).
    /// Used for runtime integrity verification since the original tarball is not kept.
    #[serde(default)]
    pub dir_checksum: Option<String>,
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
        let meta = parse_skill_metadata(&skill_md);
        let version = meta.version;
        let runtime = meta.runtime;
        let permissions = meta.permissions;

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
            dir_checksum: None,
        });
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Parsed skill metadata from SKILL.md frontmatter.
struct SkillMetadata {
    version: String,
    runtime: String,
    permissions: Vec<String>,
    entrypoint: Option<String>,
}

/// Parse basic metadata from SKILL.md frontmatter.
fn parse_skill_metadata(path: &Path) -> SkillMetadata {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut meta = SkillMetadata {
        version: "0.1.0".to_string(),
        runtime: "none".to_string(),
        permissions: Vec::new(),
        entrypoint: None,
    };

    // Simple frontmatter parsing
    if let Some(after_prefix) = content.strip_prefix("---") {
        if let Some(end) = after_prefix.find("---") {
            let frontmatter = &after_prefix[..end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("version:") {
                    meta.version = v.trim().to_string();
                }
                if let Some(r) = line.strip_prefix("runtime:") {
                    meta.runtime = r.trim().to_string();
                }
                if let Some(e) = line.strip_prefix("entrypoint:") {
                    meta.entrypoint = Some(e.trim().to_string());
                }
                if line.starts_with("- read")
                    || line.starts_with("- write")
                    || line.starts_with("- shell")
                    || line.starts_with("- network")
                {
                    meta.permissions
                        .push(line.trim_start_matches("- ").to_string());
                }
            }
        }
    }

    meta
}

/// Build a `SkillManifest` from a skill directory containing SKILL.md.
///
/// Parses the SKILL.md frontmatter for metadata and checks for a `.wasm`
/// module file alongside the manifest.
pub fn load_manifest(skill_dir: &Path) -> Result<crate::SkillManifest, RegistryError> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(RegistryError::NotFound(format!(
            "no SKILL.md in {}",
            skill_dir.display()
        )));
    }

    let name = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = parse_skill_metadata(&skill_md);

    let runtime = match meta.runtime.as_str() {
        "wasm" => crate::SkillRuntime::Wasm,
        "host_supervised" => crate::SkillRuntime::HostSupervised,
        "mvm" => crate::SkillRuntime::Mvm,
        _ => crate::SkillRuntime::InstructionOnly,
    };

    let permissions = meta
        .permissions
        .into_iter()
        .map(|p| {
            let capability = match p.as_str() {
                "read" => agentzero_core::Capability::FileRead,
                "write" => agentzero_core::Capability::FileWrite,
                "shell" => agentzero_core::Capability::ShellCommand,
                "network" => agentzero_core::Capability::NetworkRequest,
                _ => agentzero_core::Capability::FileRead,
            };
            crate::SkillPermission {
                capability,
                reason: format!("declared in SKILL.md: {p}"),
            }
        })
        .collect();

    let source = Some(crate::SkillPackageRef::Local {
        path: skill_dir.to_string_lossy().to_string(),
    });

    Ok(crate::SkillManifest {
        id: agentzero_core::SkillId::from_string(&name),
        name: name.clone(),
        version: meta.version,
        description: name,
        runtime,
        permissions,
        source,
        entrypoint: meta.entrypoint,
    })
}

/// Find the `.wasm` module file in a skill directory, if any.
pub fn find_wasm_module(skill_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(skill_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            return Some(path);
        }
    }
    None
}

/// Default lockfile path for a project.
pub fn lockfile_path(project_root: &Path) -> PathBuf {
    project_root.join(".agentzero/skills.lock")
}

/// Compute a deterministic SHA-256 hash of a directory's contents.
///
/// Walks all files in sorted order by relative path, feeding each file's
/// relative path and contents into a single SHA-256 hasher. This produces
/// a stable hash that changes when any file is added, removed, or modified.
///
/// Returns a string like `sha256:a1b2c3...`.
pub fn compute_directory_checksum(dir: &Path) -> Result<String, RegistryError> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    let mut entries = Vec::new();

    collect_files(dir, dir, &mut entries)?;
    entries.sort();

    for rel_path in &entries {
        let full_path = dir.join(rel_path);
        let contents = std::fs::read(&full_path).map_err(|e| {
            RegistryError::IoError(format!("failed to read {}: {e}", full_path.display()))
        })?;
        // Hash relative path, content length, and content together for unambiguous framing
        hasher.update(rel_path.as_bytes());
        hasher.update((contents.len() as u64).to_le_bytes());
        hasher.update(&contents);
    }

    let hash = hasher.finalize();
    Ok(format!("sha256:{}", hex_encode(&hash)))
}

/// Recursively collect all file paths relative to `base` under `dir`.
fn collect_files(base: &Path, dir: &Path, entries: &mut Vec<String>) -> Result<(), RegistryError> {
    let read_dir = std::fs::read_dir(dir).map_err(|e| RegistryError::IoError(e.to_string()))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| RegistryError::IoError(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, entries)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| RegistryError::IoError(e.to_string()))?;
            entries.push(rel.to_string_lossy().to_string());
        }
    }
    Ok(())
}

/// Encode bytes as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Cached verification metadata to avoid re-hashing on every run.
///
/// Stores the last-verified epoch seconds per skill name in a JSON sidecar
/// file at `.agentzero/skills.lock.meta`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationCache {
    pub last_verified: BTreeMap<String, u64>,
}

impl VerificationCache {
    /// Path to the verification cache sidecar file.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(".agentzero/skills.lock.meta")
    }

    /// Load from disk, returning empty cache if file doesn't exist.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            std::fs::read_to_string(path).map_err(|e| RegistryError::IoError(e.to_string()))?;
        serde_json::from_str(&content).map_err(|e| RegistryError::ParseError(e.to_string()))
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| RegistryError::IoError(e.to_string()))
    }

    /// Record that a skill was verified at the current time.
    pub fn mark_verified(&mut self, skill_name: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_verified.insert(skill_name.to_string(), now);
    }

    /// Check if a skill directory has been modified since last verification.
    ///
    /// Returns `true` if we should skip re-hashing (mtime is not newer).
    pub fn is_fresh(&self, skill_name: &str, skill_dir: &Path) -> bool {
        let last = match self.last_verified.get(skill_name) {
            Some(ts) => *ts,
            None => return false,
        };

        match newest_mtime(skill_dir) {
            Ok(mtime) => mtime <= last,
            Err(_) => false,
        }
    }
}

/// Get the newest modification time (as epoch seconds) from any file in a directory.
fn newest_mtime(dir: &Path) -> Result<u64, RegistryError> {
    let mut newest: u64 = 0;
    let read_dir = std::fs::read_dir(dir).map_err(|e| RegistryError::IoError(e.to_string()))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| RegistryError::IoError(e.to_string()))?;
        let path = entry.path();

        let mtime = if path.is_dir() {
            newest_mtime(&path)?
        } else {
            path.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };

        if mtime > newest {
            newest = mtime;
        }
    }

    Ok(newest)
}

impl SkillLockfile {
    /// Verify the integrity of an installed skill against its lockfile entry.
    ///
    /// Computes the directory checksum and compares against the stored `dir_checksum`.
    /// Returns `Ok(())` if the check passes or if no `dir_checksum` is recorded (legacy entry).
    pub fn verify_skill(&self, skill_name: &str, skill_dir: &Path) -> Result<(), RegistryError> {
        let entry = match self.skills.get(skill_name) {
            Some(e) => e,
            None => {
                // Not in lockfile — nothing to verify against
                return Ok(());
            }
        };

        let expected = match &entry.dir_checksum {
            Some(cs) => cs,
            None => {
                // Legacy entry without dir_checksum — skip with no error
                return Ok(());
            }
        };

        let actual = compute_directory_checksum(skill_dir)?;
        if actual != *expected {
            return Err(RegistryError::IntegrityFailed {
                skill: skill_name.to_string(),
                expected: expected.clone(),
                actual,
            });
        }

        Ok(())
    }
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
            dir_checksum: None,
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
    fn load_manifest_instruction_only() {
        let dir = temp_dir("manifest-inst");
        let skill_dir = dir.join("my-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nversion: 1.0.0\nruntime: none\n---\n# My Skill\n",
        )
        .expect("should write");

        let manifest = load_manifest(&skill_dir).expect("should load");
        assert_eq!(manifest.name, "my-skill");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.runtime, crate::SkillRuntime::InstructionOnly);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_manifest_wasm_runtime() {
        let dir = temp_dir("manifest-wasm");
        let skill_dir = dir.join("wasm-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nversion: 0.2.0\nruntime: wasm\n- read\n---\n# WASM Skill\n",
        )
        .expect("should write");
        fs::write(skill_dir.join("module.wasm"), b"fake wasm").expect("should write");

        let manifest = load_manifest(&skill_dir).expect("should load");
        assert_eq!(manifest.runtime, crate::SkillRuntime::Wasm);

        let wasm_path = find_wasm_module(&skill_dir);
        assert!(wasm_path.is_some());
        assert!(wasm_path
            .expect("should find")
            .to_string_lossy()
            .contains("module.wasm"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_manifest_missing_dir_fails() {
        let result = load_manifest(Path::new("/nonexistent/skill"));
        assert!(result.is_err());
    }

    #[test]
    fn find_wasm_module_returns_none_when_absent() {
        let dir = temp_dir("no-wasm");
        let skill_dir = dir.join("plain-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# No WASM").expect("should write");

        assert!(find_wasm_module(&skill_dir).is_none());

        fs::remove_dir_all(&dir).ok();
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
            dir_checksum: None,
        });
        assert!(lockfile.contains("test"));
        lockfile.remove("test");
        assert!(!lockfile.contains("test"));
    }

    #[test]
    fn directory_checksum_is_deterministic() {
        let dir = temp_dir("checksum-det");
        let skill_dir = dir.join("my-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").expect("should write");
        fs::write(skill_dir.join("patterns.toml"), "# patterns\n").expect("should write");

        let c1 = compute_directory_checksum(&skill_dir).expect("should compute");
        let c2 = compute_directory_checksum(&skill_dir).expect("should compute");
        assert_eq!(c1, c2);
        assert!(c1.starts_with("sha256:"));
        assert_eq!(c1.len(), 7 + 64);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn directory_checksum_changes_on_modification() {
        let dir = temp_dir("checksum-mod");
        let skill_dir = dir.join("my-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Original\n").expect("should write");

        let before = compute_directory_checksum(&skill_dir).expect("should compute");
        fs::write(skill_dir.join("SKILL.md"), "# Modified\n").expect("should write");
        let after = compute_directory_checksum(&skill_dir).expect("should compute");

        assert_ne!(before, after);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_skill_passes_when_unchanged() {
        let dir = temp_dir("verify-pass");
        let skill_dir = dir.join("good-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Good Skill\n").expect("should write");

        let checksum = compute_directory_checksum(&skill_dir).expect("should compute");
        let mut lockfile = SkillLockfile::default();
        lockfile.register(LockedSkill {
            name: "good-skill".into(),
            version: "1.0".into(),
            source: "local".into(),
            runtime: "none".into(),
            permissions: vec![],
            checksum: None,
            dir_checksum: Some(checksum),
        });

        assert!(lockfile.verify_skill("good-skill", &skill_dir).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_skill_fails_on_tamper() {
        let dir = temp_dir("verify-fail");
        let skill_dir = dir.join("bad-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Original\n").expect("should write");

        let checksum = compute_directory_checksum(&skill_dir).expect("should compute");
        let mut lockfile = SkillLockfile::default();
        lockfile.register(LockedSkill {
            name: "bad-skill".into(),
            version: "1.0".into(),
            source: "local".into(),
            runtime: "none".into(),
            permissions: vec![],
            checksum: None,
            dir_checksum: Some(checksum),
        });

        // Tamper with the file
        fs::write(skill_dir.join("SKILL.md"), "# Tampered!\n").expect("should write");

        let result = lockfile.verify_skill("bad-skill", &skill_dir);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("integrity check failed"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_skill_skips_legacy_entry() {
        let dir = temp_dir("verify-legacy");
        let skill_dir = dir.join("legacy-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Legacy\n").expect("should write");

        let mut lockfile = SkillLockfile::default();
        lockfile.register(LockedSkill {
            name: "legacy-skill".into(),
            version: "1.0".into(),
            source: "local".into(),
            runtime: "none".into(),
            permissions: vec![],
            checksum: None,
            dir_checksum: None, // No dir_checksum = legacy
        });

        // Should pass even though no dir_checksum exists
        assert!(lockfile.verify_skill("legacy-skill", &skill_dir).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verification_cache_roundtrip() {
        let dir = temp_dir("vcache-rt");
        fs::create_dir_all(&dir).expect("should create");
        let path = dir.join("skills.lock.meta");

        let mut cache = VerificationCache::default();
        cache.mark_verified("test-skill");
        assert!(cache.last_verified.contains_key("test-skill"));

        cache.save(&path).expect("should save");
        let loaded = VerificationCache::load(&path).expect("should load");
        assert!(loaded.last_verified.contains_key("test-skill"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verification_cache_freshness() {
        let dir = temp_dir("vcache-fresh");
        let skill_dir = dir.join("my-skill");
        fs::create_dir_all(&skill_dir).expect("should create");
        fs::write(skill_dir.join("SKILL.md"), "# Test\n").expect("should write");

        let mut cache = VerificationCache::default();

        // Not verified yet — should not be fresh
        assert!(!cache.is_fresh("my-skill", &skill_dir));

        // Mark as verified
        cache.mark_verified("my-skill");

        // Should be fresh now (mtime is in the past relative to mark time)
        assert!(cache.is_fresh("my-skill", &skill_dir));

        // Modify the file — should no longer be fresh
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(skill_dir.join("SKILL.md"), "# Modified\n").expect("should write");
        assert!(!cache.is_fresh("my-skill", &skill_dir));

        fs::remove_dir_all(&dir).ok();
    }
}
