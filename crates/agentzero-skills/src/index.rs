//! Central skill index: short-name resolution for `agentzero install <name>`.
//!
//! The index is a JSON file hosted at a well-known GitHub repository.
//! It maps short skill names to their GitHub owner/repo coordinates.
//! A local cache avoids fetching the index on every install.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::RegistryError;

/// Default GitHub repo hosting the central skill index.
pub const DEFAULT_INDEX_REPO: &str = "agentzero-skills/index";

/// File within the index repo containing the skill registry.
pub const INDEX_FILE: &str = "index.json";

/// How long the local cache is considered fresh (24 hours in seconds).
const CACHE_TTL_SECS: u64 = 86400;

/// The central skill index mapping short names to GitHub repos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillIndex {
    pub version: u32,
    pub skills: BTreeMap<String, SkillIndexEntry>,
}

/// A single entry in the skill index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillIndexEntry {
    /// GitHub owner/repo (e.g., "agentzero-skills/repo-security-audit").
    pub repo: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Minimum compatible version (informational).
    #[serde(default)]
    pub min_version: Option<String>,
}

impl SkillIndex {
    /// Resolve a short name to a GitHub owner and repo.
    pub fn resolve(&self, name: &str) -> Option<(String, String)> {
        let entry = self.skills.get(name)?;
        let parts: Vec<&str> = entry.repo.splitn(2, '/').collect();
        if parts.len() == 2 {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }

    /// List all available skill names.
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.skills
            .iter()
            .map(|(name, entry)| (name.as_str(), entry.description.as_str()))
            .collect()
    }
}

/// Local cache for the skill index, stored at `.agentzero/index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexCache {
    /// Epoch seconds when the index was last fetched.
    pub fetched_at: u64,
    /// The cached index data.
    pub index: SkillIndex,
}

impl IndexCache {
    /// Path to the cached index file.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(".agentzero/index.json")
    }

    /// Load from disk.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
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

    /// Check if the cache is stale (older than CACHE_TTL_SECS).
    pub fn is_stale(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.fetched_at) > CACHE_TTL_SECS
    }
}

/// Fetch the skill index from GitHub raw content API.
pub async fn fetch_index(
    client: &reqwest::Client,
    index_repo: &str,
) -> Result<SkillIndex, RegistryError> {
    let parts: Vec<&str> = index_repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(RegistryError::ParseError(format!(
            "invalid index repo: {index_repo} (expected owner/repo)"
        )));
    }

    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/main/{INDEX_FILE}",
        parts[0], parts[1]
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "agentzero/0.1")
        .send()
        .await
        .map_err(|e| RegistryError::IoError(format!("failed to fetch index: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(RegistryError::IoError(format!(
            "failed to fetch index (HTTP {status}): {url}"
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| RegistryError::IoError(format!("failed to read index response: {e}")))?;

    let index: SkillIndex =
        serde_json::from_str(&body).map_err(|e| RegistryError::ParseError(e.to_string()))?;

    Ok(index)
}

/// Load the index from cache, fetching fresh data if stale or missing.
///
/// If `force_refresh` is true, always fetches regardless of cache age.
pub async fn load_or_fetch_index(
    project_root: &Path,
    index_repo: &str,
    force_refresh: bool,
) -> Result<SkillIndex, RegistryError> {
    let cache_path = IndexCache::path(project_root);

    // Try loading from cache first
    if !force_refresh {
        if let Ok(cache) = IndexCache::load(&cache_path) {
            if !cache.is_stale() {
                return Ok(cache.index);
            }
        }
    }

    // Fetch fresh index
    let client = reqwest::Client::new();
    let index = fetch_index(&client, index_repo).await?;

    // Cache it
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cache = IndexCache {
        fetched_at: now,
        index: index.clone(),
    };

    if let Err(e) = cache.save(&cache_path) {
        eprintln!("warning: failed to cache skill index: {e}");
    }

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index_json() -> &'static str {
        r#"{
            "version": 1,
            "skills": {
                "security-audit": {
                    "repo": "agentzero-skills/repo-security-audit",
                    "description": "Audit a repository for leaked secrets and PII"
                },
                "dependency-audit": {
                    "repo": "agentzero-skills/dependency-audit",
                    "description": "Check dependencies for known vulnerabilities",
                    "min_version": "0.1.0"
                }
            }
        }"#
    }

    #[test]
    fn deserialize_index() {
        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");
        assert_eq!(index.version, 1);
        assert_eq!(index.skills.len(), 2);
        assert!(index.skills.contains_key("security-audit"));
        assert!(index.skills.contains_key("dependency-audit"));
    }

    #[test]
    fn resolve_returns_owner_repo() {
        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");
        let result = index.resolve("security-audit");
        assert_eq!(
            result,
            Some(("agentzero-skills".into(), "repo-security-audit".into()))
        );
    }

    #[test]
    fn resolve_returns_none_for_unknown() {
        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");
        assert!(index.resolve("nonexistent").is_none());
    }

    #[test]
    fn list_returns_all_skills() {
        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");
        let list = index.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agentzero-index-cache-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("should create dir");
        let path = dir.join("index.json");

        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");
        let cache = IndexCache {
            fetched_at: 1000000,
            index,
        };

        cache.save(&path).expect("should save");
        let loaded = IndexCache::load(&path).expect("should load");
        assert_eq!(loaded.fetched_at, 1000000);
        assert_eq!(loaded.index.skills.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cache_staleness() {
        let index: SkillIndex = serde_json::from_str(sample_index_json()).expect("should parse");

        // Fresh cache (just now)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_secs();

        let fresh = IndexCache {
            fetched_at: now,
            index: index.clone(),
        };
        assert!(!fresh.is_stale());

        // Stale cache (2 days ago)
        let stale = IndexCache {
            fetched_at: now.saturating_sub(172800),
            index,
        };
        assert!(stale.is_stale());
    }
}
