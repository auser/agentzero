//! Persistent store for dynamically-created agents.
//!
//! Follows the [`ApiKeyStore`](crate::api_keys::ApiKeyStore) pattern:
//! `RwLock<Vec<AgentRecord>>` with optional `EncryptedJsonStore` backing
//! for encrypted-at-rest persistence.

use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Types ───────────────────────────────────────────────────────────────────

/// Status of a dynamically-created agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Active,
    Stopped,
}

/// Channel configuration for a dynamic agent.
///
/// Tokens are stored encrypted at rest via `EncryptedJsonStore`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChannelConfig {
    /// Bot token or access token for the platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    /// Registered webhook URL (set after auto-registration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Additional platform-specific fields.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, String>,
}

/// Persistent record for a dynamically-created agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    pub agent_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub channels: HashMap<String, AgentChannelConfig>,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: AgentStatus,
}

/// Fields that can be updated on an existing agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<HashMap<String, AgentChannelConfig>>,
}

impl AgentRecord {
    /// Convert this record to a `SwarmAgentConfig` for use with the swarm builder.
    pub fn to_swarm_config(&self) -> agentzero_config::SwarmAgentConfig {
        agentzero_config::SwarmAgentConfig::new(&self.name, &self.description)
            .with_provider(&self.provider, &self.model)
            .with_system_prompt(self.system_prompt.as_deref().unwrap_or(""))
            .with_keywords(self.keywords.clone())
            .with_allowed_tools(self.allowed_tools.clone())
    }

    /// Convert this record to an `AgentDescriptor` for routing registration.
    pub fn to_descriptor(&self) -> crate::agent_router::AgentDescriptor {
        crate::agent_router::AgentDescriptor {
            id: self.agent_id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            keywords: self.keywords.clone(),
            subscribes_to: vec!["channel.*.message".to_string()],
            produces: vec![],
            privacy_boundary: String::new(),
        }
    }
}

// ─── AgentStore ──────────────────────────────────────────────────────────────

const AGENTS_FILE: &str = "agents.json";

/// Persistent store for dynamically-created agents.
///
/// When constructed with `persistent()`, every mutation is atomically flushed
/// to an encrypted JSON file. When constructed with `new()`, operates purely
/// in-memory (useful for tests).
pub struct AgentStore {
    agents: std::sync::RwLock<Vec<AgentRecord>>,
    backing: Option<EncryptedJsonStore>,
}

impl Default for AgentStore {
    fn default() -> Self {
        Self {
            agents: std::sync::RwLock::new(Vec::new()),
            backing: None,
        }
    }
}

impl AgentStore {
    /// Create an in-memory-only store (no persistence). Useful for tests.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a persistent store backed by an encrypted JSON file.
    /// Loads any existing agents from disk on construction.
    pub fn persistent(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, AGENTS_FILE)?;
        let agents: Vec<AgentRecord> = store.load_or_default()?;
        Ok(Self {
            agents: std::sync::RwLock::new(agents),
            backing: Some(store),
        })
    }

    /// Create a new agent. Returns the created record.
    pub fn create(&self, mut record: AgentRecord) -> anyhow::Result<AgentRecord> {
        let now = now_epoch();
        if record.agent_id.is_empty() {
            record.agent_id = generate_agent_id();
        }
        record.created_at = now;
        record.updated_at = now;
        record.status = AgentStatus::Active;

        let mut agents = self.agents.write().expect("agent store lock");

        // Reject duplicate IDs.
        if agents.iter().any(|a| a.agent_id == record.agent_id) {
            anyhow::bail!("agent with id '{}' already exists", record.agent_id);
        }

        agents.push(record.clone());
        self.flush(&agents)?;

        tracing::info!(
            agent_id = %record.agent_id,
            name = %record.name,
            "agent created"
        );

        Ok(record)
    }

    /// Get an agent by ID.
    pub fn get(&self, agent_id: &str) -> Option<AgentRecord> {
        let agents = self.agents.read().expect("agent store lock");
        agents.iter().find(|a| a.agent_id == agent_id).cloned()
    }

    /// List all agents.
    pub fn list(&self) -> Vec<AgentRecord> {
        self.agents.read().expect("agent store lock").clone()
    }

    /// Update an agent. Returns the updated record, or `None` if not found.
    pub fn update(
        &self,
        agent_id: &str,
        update: AgentUpdate,
    ) -> anyhow::Result<Option<AgentRecord>> {
        let mut agents = self.agents.write().expect("agent store lock");
        let Some(record) = agents.iter_mut().find(|a| a.agent_id == agent_id) else {
            return Ok(None);
        };

        if let Some(name) = update.name {
            record.name = name;
        }
        if let Some(description) = update.description {
            record.description = description;
        }
        if let Some(system_prompt) = update.system_prompt {
            record.system_prompt = Some(system_prompt);
        }
        if let Some(provider) = update.provider {
            record.provider = provider;
        }
        if let Some(model) = update.model {
            record.model = model;
        }
        if let Some(keywords) = update.keywords {
            record.keywords = keywords;
        }
        if let Some(allowed_tools) = update.allowed_tools {
            record.allowed_tools = allowed_tools;
        }
        if let Some(channels) = update.channels {
            record.channels = channels;
        }
        record.updated_at = now_epoch();

        let updated = record.clone();
        self.flush(&agents)?;

        tracing::info!(agent_id = %agent_id, "agent updated");

        Ok(Some(updated))
    }

    /// Delete an agent by ID. Returns `true` if it existed.
    pub fn delete(&self, agent_id: &str) -> anyhow::Result<bool> {
        let mut agents = self.agents.write().expect("agent store lock");
        let before = agents.len();
        agents.retain(|a| a.agent_id != agent_id);
        let removed = agents.len() < before;
        if removed {
            self.flush(&agents)?;
            tracing::info!(agent_id = %agent_id, "agent deleted");
        }
        Ok(removed)
    }

    /// Set agent status.
    pub fn set_status(&self, agent_id: &str, status: AgentStatus) -> anyhow::Result<bool> {
        let mut agents = self.agents.write().expect("agent store lock");
        let Some(record) = agents.iter_mut().find(|a| a.agent_id == agent_id) else {
            return Ok(false);
        };
        record.status = status;
        record.updated_at = now_epoch();
        self.flush(&agents)?;
        Ok(true)
    }

    /// Return total number of stored agents.
    pub fn count(&self) -> usize {
        self.agents.read().expect("agent store lock").len()
    }

    /// Flush current state to the encrypted backing store (if present).
    fn flush(&self, agents: &[AgentRecord]) -> anyhow::Result<()> {
        if let Some(ref store) = self.backing {
            store.save(&agents.to_vec())?;
        }
        Ok(())
    }
}

fn generate_agent_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_nanos();
    let seq = CTR.fetch_add(1, Ordering::Relaxed);
    let hash = {
        let raw = format!("{}-{}-{now}-{seq}", std::process::id(), now);
        // Simple hash to get a compact hex ID.
        let mut h: u64 = 0xcbf29ce484222325; // FNV-1a offset basis
        for b in raw.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3); // FNV-1a prime
        }
        h
    };
    format!("agent_{hash:016x}")
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(name: &str) -> AgentRecord {
        AgentRecord {
            agent_id: String::new(),
            name: name.to_string(),
            description: format!("{name} agent"),
            system_prompt: Some(format!("You are {name}")),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            keywords: vec!["test".to_string()],
            allowed_tools: vec![],
            channels: HashMap::new(),
            created_at: 0,
            updated_at: 0,
            status: AgentStatus::Active,
        }
    }

    #[test]
    fn create_and_get_roundtrip() {
        let store = AgentStore::new();
        let record = store.create(make_record("Aria")).expect("create");
        assert!(!record.agent_id.is_empty());
        assert!(record.agent_id.starts_with("agent_"));
        assert!(record.created_at > 0);

        let fetched = store.get(&record.agent_id).expect("should exist");
        assert_eq!(fetched.name, "Aria");
        assert_eq!(fetched.agent_id, record.agent_id);
    }

    #[test]
    fn list_returns_all() {
        let store = AgentStore::new();
        store.create(make_record("Alpha")).expect("create");
        store.create(make_record("Beta")).expect("create");
        store.create(make_record("Gamma")).expect("create");

        let all = store.list();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn update_modifies_fields() {
        let store = AgentStore::new();
        let record = store.create(make_record("Old")).expect("create");

        let updated = store
            .update(
                &record.agent_id,
                AgentUpdate {
                    name: Some("New".to_string()),
                    system_prompt: Some("You are New".to_string()),
                    ..Default::default()
                },
            )
            .expect("update")
            .expect("should find agent");

        assert_eq!(updated.name, "New");
        assert_eq!(updated.system_prompt.as_deref(), Some("You are New"));
        assert!(updated.updated_at >= record.updated_at);
    }

    #[test]
    fn update_unknown_returns_none() {
        let store = AgentStore::new();
        let result = store
            .update("nonexistent", AgentUpdate::default())
            .expect("no error");
        assert!(result.is_none());
    }

    #[test]
    fn delete_removes_agent() {
        let store = AgentStore::new();
        let record = store.create(make_record("Temp")).expect("create");
        assert_eq!(store.count(), 1);

        assert!(store.delete(&record.agent_id).expect("delete"));
        assert_eq!(store.count(), 0);
        assert!(store.get(&record.agent_id).is_none());
    }

    #[test]
    fn delete_unknown_returns_false() {
        let store = AgentStore::new();
        assert!(!store.delete("nonexistent").expect("delete"));
    }

    #[test]
    fn duplicate_id_rejected() {
        let store = AgentStore::new();
        let mut r1 = make_record("First");
        r1.agent_id = "fixed_id".to_string();
        store.create(r1).expect("create");

        let mut r2 = make_record("Second");
        r2.agent_id = "fixed_id".to_string();
        assert!(store.create(r2).is_err());
    }

    #[test]
    fn set_status_updates() {
        let store = AgentStore::new();
        let record = store.create(make_record("Runner")).expect("create");

        assert!(store
            .set_status(&record.agent_id, AgentStatus::Stopped)
            .expect("set status"));
        let fetched = store.get(&record.agent_id).expect("get");
        assert_eq!(fetched.status, AgentStatus::Stopped);
    }

    #[test]
    fn persistent_store_survives_reload() {
        let dir = unique_temp_dir();

        let agent_id = {
            let store = AgentStore::persistent(&dir).expect("open");
            let record = store.create(make_record("Durable")).expect("create");
            record.agent_id
        };

        // Reload from disk.
        let store2 = AgentStore::persistent(&dir).expect("reopen");
        let fetched = store2.get(&agent_id).expect("should survive reload");
        assert_eq!(fetched.name, "Durable");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn persistent_delete_survives_reload() {
        let dir = unique_temp_dir();

        let agent_id = {
            let store = AgentStore::persistent(&dir).expect("open");
            let record = store.create(make_record("Ephemeral")).expect("create");
            store.delete(&record.agent_id).expect("delete");
            record.agent_id
        };

        let store2 = AgentStore::persistent(&dir).expect("reopen");
        assert!(store2.get(&agent_id).is_none());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn persistent_file_is_encrypted() {
        let dir = unique_temp_dir();
        let store = AgentStore::persistent(&dir).expect("open");
        store.create(make_record("SecretAgent")).expect("create");

        let raw = std::fs::read_to_string(dir.join(AGENTS_FILE)).expect("read file");
        assert!(
            !raw.contains("SecretAgent"),
            "agent name should not appear in plaintext on disk"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = CTR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-agents-{}-{now}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
