use super::types::{validate_domain_name, Domain};
use anyhow::Context;
use std::path::{Path, PathBuf};
use tokio::fs;

const DOMAINS_DIR: &str = ".agentzero/domains";

/// Persistent store for domain definitions.
///
/// Domains are stored as individual JSON files:
/// - Project-level: `{workspace}/.agentzero/domains/{name}.json`
/// - Global: `~/.config/agentzero/domains/{name}.json`
///
/// Project domains take precedence over global domains with the same name.
pub struct DomainStore;

impl DomainStore {
    /// List all available domains (project + global, project overrides global).
    pub async fn list(workspace_root: &str) -> anyhow::Result<Vec<Domain>> {
        let mut domains = std::collections::HashMap::new();

        // Load global domains first (lower priority).
        if let Some(global_dir) = Self::global_dir() {
            if let Ok(entries) = Self::load_dir(&global_dir).await {
                for domain in entries {
                    domains.insert(domain.name.clone(), domain);
                }
            }
        }

        // Load project domains (higher priority, overrides global).
        let project_dir = Self::project_dir(workspace_root);
        if let Ok(entries) = Self::load_dir(&project_dir).await {
            for domain in entries {
                domains.insert(domain.name.clone(), domain);
            }
        }

        let mut result: Vec<Domain> = domains.into_values().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    /// Load a specific domain by name (project takes precedence over global).
    pub async fn load(workspace_root: &str, name: &str) -> anyhow::Result<Domain> {
        validate_domain_name(name)?;

        // Try project first.
        let project_path = Self::project_dir(workspace_root).join(format!("{name}.json"));
        if project_path.exists() {
            return Self::load_file(&project_path).await;
        }

        // Fall back to global.
        if let Some(global_dir) = Self::global_dir() {
            let global_path = global_dir.join(format!("{name}.json"));
            if global_path.exists() {
                return Self::load_file(&global_path).await;
            }
        }

        anyhow::bail!("domain not found: {name}")
    }

    /// Save a domain to the project-level store.
    pub async fn save(workspace_root: &str, domain: &Domain) -> anyhow::Result<()> {
        validate_domain_name(&domain.name)?;

        let dir = Self::project_dir(workspace_root);
        fs::create_dir_all(&dir)
            .await
            .context("failed to create domains directory")?;

        let path = dir.join(format!("{}.json", domain.name));
        let data =
            serde_json::to_string_pretty(domain).context("failed to serialize domain config")?;
        fs::write(&path, data)
            .await
            .context("failed to write domain config")?;
        Ok(())
    }

    /// Delete a domain from the project-level store.
    pub async fn delete(workspace_root: &str, name: &str) -> anyhow::Result<()> {
        validate_domain_name(name)?;

        let path = Self::project_dir(workspace_root).join(format!("{name}.json"));
        if !path.exists() {
            anyhow::bail!("domain not found: {name}");
        }
        fs::remove_file(&path)
            .await
            .context("failed to delete domain config")?;
        Ok(())
    }

    /// Check whether a domain exists (project or global).
    pub async fn exists(workspace_root: &str, name: &str) -> bool {
        let project_path = Self::project_dir(workspace_root).join(format!("{name}.json"));
        if project_path.exists() {
            return true;
        }
        if let Some(global_dir) = Self::global_dir() {
            let global_path = global_dir.join(format!("{name}.json"));
            if global_path.exists() {
                return true;
            }
        }
        false
    }

    fn project_dir(workspace_root: &str) -> PathBuf {
        Path::new(workspace_root).join(DOMAINS_DIR)
    }

    fn global_dir() -> Option<PathBuf> {
        // Use $HOME/.config/agentzero/domains (no dirs crate dependency).
        std::env::var("HOME").ok().map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("agentzero")
                .join("domains")
        })
    }

    async fn load_dir(dir: &Path) -> anyhow::Result<Vec<Domain>> {
        let mut domains = Vec::new();
        if !dir.exists() {
            return Ok(domains);
        }

        let mut entries = fs::read_dir(dir)
            .await
            .context("failed to read domains directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                // Skip the lessons file.
                if path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with('_'))
                    .unwrap_or(false)
                {
                    continue;
                }
                match Self::load_file(&path).await {
                    Ok(domain) => domains.push(domain),
                    Err(e) => {
                        tracing::warn!("skipping invalid domain file {:?}: {e}", path);
                    }
                }
            }
        }
        Ok(domains)
    }

    async fn load_file(path: &Path) -> anyhow::Result<Domain> {
        let data = fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read domain file: {}", path.display()))?;
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse domain file: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        DomainConstraints, SourceConfig, VerificationConfig, WorkflowTemplate,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-domain-store-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn test_domain(name: &str) -> Domain {
        Domain {
            name: name.to_string(),
            description: format!("Test domain: {name}"),
            sources: vec![SourceConfig {
                kind: "web_search".to_string(),
                label: "Web Search".to_string(),
                config: serde_json::json!({}),
                priority: 0,
                enabled: true,
            }],
            verification: VerificationConfig::default(),
            workflows: vec![WorkflowTemplate {
                name: "basic".to_string(),
                description: "Basic workflow".to_string(),
                steps: vec!["Search".to_string(), "Report".to_string()],
                approval_required: vec![],
            }],
            system_prompt: "Be helpful.".to_string(),
            constraints: DomainConstraints::default(),
            created_at: "2026-03-16T00:00:00Z".to_string(),
            updated_at: String::new(),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let domain = test_domain("test-roundtrip");
        DomainStore::save(&ws, &domain)
            .await
            .expect("save should succeed");

        let loaded = DomainStore::load(&ws, "test-roundtrip")
            .await
            .expect("load should succeed");
        assert_eq!(loaded.name, "test-roundtrip");
        assert_eq!(loaded.sources.len(), 1);
        assert_eq!(loaded.workflows.len(), 1);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_returns_sorted() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        DomainStore::save(&ws, &test_domain("bravo"))
            .await
            .expect("save should succeed");
        DomainStore::save(&ws, &test_domain("alpha"))
            .await
            .expect("save should succeed");
        DomainStore::save(&ws, &test_domain("charlie"))
            .await
            .expect("save should succeed");

        let domains = DomainStore::list(&ws).await.expect("list should succeed");
        let names: Vec<&str> = domains.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bravo", "charlie"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn delete_removes_domain() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        DomainStore::save(&ws, &test_domain("to-delete"))
            .await
            .expect("save should succeed");
        assert!(DomainStore::exists(&ws, "to-delete").await);

        DomainStore::delete(&ws, "to-delete")
            .await
            .expect("delete should succeed");
        assert!(!DomainStore::exists(&ws, "to-delete").await);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn load_nonexistent_fails() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let err = DomainStore::load(&ws, "nonexistent")
            .await
            .expect_err("should fail for nonexistent domain");
        assert!(err.to_string().contains("not found"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_empty_returns_empty() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let domains = DomainStore::list(&ws).await.expect("list should succeed");
        assert!(domains.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn save_overwrites_existing() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let mut domain = test_domain("overwrite-me");
        DomainStore::save(&ws, &domain)
            .await
            .expect("first save should succeed");

        domain.description = "Updated description".to_string();
        DomainStore::save(&ws, &domain)
            .await
            .expect("overwrite save should succeed");

        let loaded = DomainStore::load(&ws, "overwrite-me")
            .await
            .expect("load should succeed");
        assert_eq!(loaded.description, "Updated description");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn save_rejects_invalid_name() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let mut domain = test_domain("valid");
        domain.name = "bad/name".to_string();
        let err = DomainStore::save(&ws, &domain)
            .await
            .expect_err("invalid name should fail");
        assert!(err.to_string().contains("alphanumeric"));

        std::fs::remove_dir_all(dir).ok();
    }
}
