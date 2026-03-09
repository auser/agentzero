//! API key management for multi-tenancy and scope-based authorization.
//!
//! Foundation layer for RBAC: API keys carry organization isolation,
//! user identity, and permission scopes.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// Scopes that can be assigned to an API key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// Read runs and their results.
    #[serde(rename = "runs:read")]
    RunsRead,
    /// Submit new runs.
    #[serde(rename = "runs:write")]
    RunsWrite,
    /// Cancel and manage runs.
    #[serde(rename = "runs:manage")]
    RunsManage,
    /// Access admin endpoints (e-stop, key management).
    #[serde(rename = "admin")]
    Admin,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::RunsRead => "runs:read",
            Scope::RunsWrite => "runs:write",
            Scope::RunsManage => "runs:manage",
            Scope::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "runs:read" => Some(Scope::RunsRead),
            "runs:write" => Some(Scope::RunsWrite),
            "runs:manage" => Some(Scope::RunsManage),
            "admin" => Some(Scope::Admin),
            _ => None,
        }
    }
}

/// Stored API key record (key_hash, not the raw key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub key_id: String,
    pub key_hash: String,
    pub org_id: String,
    pub user_id: String,
    pub scopes: HashSet<Scope>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

/// Info extracted after validating an API key.
#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub key_id: String,
    pub org_id: String,
    pub user_id: String,
    pub scopes: HashSet<Scope>,
}

impl ApiKeyInfo {
    /// Check if this key has the required scope.
    pub fn has_scope(&self, scope: &Scope) -> bool {
        self.scopes.contains(scope)
    }
}

/// In-memory API key store.
#[derive(Default)]
pub struct ApiKeyStore {
    keys: std::sync::RwLock<Vec<ApiKeyRecord>>,
}

impl ApiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new API key. Returns the raw key string (only available at creation time).
    pub fn create(
        &self,
        org_id: &str,
        user_id: &str,
        scopes: HashSet<Scope>,
        expires_at: Option<u64>,
    ) -> (String, ApiKeyRecord) {
        let raw_key = generate_api_key();
        let key_hash = hash_key(&raw_key);
        let key_id = format!("azk_{}", &key_hash[..12]);

        let record = ApiKeyRecord {
            key_id: key_id.clone(),
            key_hash,
            org_id: org_id.to_string(),
            user_id: user_id.to_string(),
            scopes,
            created_at: now_epoch(),
            expires_at,
        };

        self.keys
            .write()
            .expect("api key store lock")
            .push(record.clone());

        (raw_key, record)
    }

    /// Validate a raw API key. Returns `Some(ApiKeyInfo)` if valid, `None` if not found or expired.
    pub fn validate(&self, raw_key: &str) -> Option<ApiKeyInfo> {
        let key_hash = hash_key(raw_key);
        let now = now_epoch();

        let keys = self.keys.read().expect("api key store lock");
        keys.iter().find_map(|record| {
            if record.key_hash != key_hash {
                return None;
            }
            if let Some(expires_at) = record.expires_at {
                if now >= expires_at {
                    return None;
                }
            }
            Some(ApiKeyInfo {
                key_id: record.key_id.clone(),
                org_id: record.org_id.clone(),
                user_id: record.user_id.clone(),
                scopes: record.scopes.clone(),
            })
        })
    }

    /// Revoke a key by key_id.
    pub fn revoke(&self, key_id: &str) -> bool {
        let mut keys = self.keys.write().expect("api key store lock");
        let before = keys.len();
        keys.retain(|r| r.key_id != key_id);
        keys.len() < before
    }

    /// List all keys for an org.
    pub fn list(&self, org_id: &str) -> Vec<ApiKeyRecord> {
        let keys = self.keys.read().expect("api key store lock");
        keys.iter()
            .filter(|r| r.org_id == org_id)
            .cloned()
            .collect()
    }
}

fn hash_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    format!(
        "az_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_validate_roundtrip() {
        let store = ApiKeyStore::new();
        let scopes: HashSet<Scope> = [Scope::RunsRead, Scope::RunsWrite].into();
        let (raw_key, record) = store.create("org-1", "user-1", scopes.clone(), None);

        assert!(raw_key.starts_with("az_"));
        assert!(record.key_id.starts_with("azk_"));

        let info = store.validate(&raw_key).expect("key should validate");
        assert_eq!(info.org_id, "org-1");
        assert_eq!(info.user_id, "user-1");
        assert!(info.has_scope(&Scope::RunsRead));
        assert!(info.has_scope(&Scope::RunsWrite));
        assert!(!info.has_scope(&Scope::Admin));
    }

    #[test]
    fn validate_unknown_key_returns_none() {
        let store = ApiKeyStore::new();
        assert!(store.validate("az_nonexistent").is_none());
    }

    #[test]
    fn expired_key_returns_none() {
        let store = ApiKeyStore::new();
        // Expire in the past.
        let (raw_key, _) = store.create("org-1", "user-1", HashSet::new(), Some(0));
        assert!(store.validate(&raw_key).is_none());
    }

    #[test]
    fn revoke_removes_key() {
        let store = ApiKeyStore::new();
        let (raw_key, record) = store.create("org-1", "user-1", [Scope::Admin].into(), None);

        assert!(store.validate(&raw_key).is_some());
        assert!(store.revoke(&record.key_id));
        assert!(store.validate(&raw_key).is_none());
    }

    #[test]
    fn revoke_unknown_returns_false() {
        let store = ApiKeyStore::new();
        assert!(!store.revoke("azk_nonexistent"));
    }

    #[test]
    fn list_filters_by_org() {
        let store = ApiKeyStore::new();
        store.create("org-1", "u1", [Scope::RunsRead].into(), None);
        store.create("org-2", "u2", [Scope::RunsRead].into(), None);
        store.create("org-1", "u3", [Scope::Admin].into(), None);

        let org1_keys = store.list("org-1");
        assert_eq!(org1_keys.len(), 2);
        assert!(org1_keys.iter().all(|k| k.org_id == "org-1"));

        let org2_keys = store.list("org-2");
        assert_eq!(org2_keys.len(), 1);
    }

    #[test]
    fn scope_from_str_roundtrip() {
        for scope in [
            Scope::RunsRead,
            Scope::RunsWrite,
            Scope::RunsManage,
            Scope::Admin,
        ] {
            let s = scope.as_str();
            let parsed = Scope::parse(s).expect("should parse");
            assert_eq!(parsed, scope);
        }
        assert!(Scope::parse("unknown").is_none());
    }
}
