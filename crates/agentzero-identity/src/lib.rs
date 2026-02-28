use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent,
    Service,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActorIdentity {
    pub id: String,
    pub display_name: String,
    pub kind: ActorKind,
    pub roles: BTreeSet<String>,
    pub metadata: BTreeMap<String, String>,
    pub created_at_epoch_secs: u64,
    pub updated_at_epoch_secs: u64,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum IdentityError {
    #[error("id must not be empty")]
    EmptyId,
    #[error("display_name must not be empty")]
    EmptyDisplayName,
    #[error("role must be lowercase snake_case (a-z0-9_)")]
    InvalidRole,
}

impl ActorIdentity {
    pub fn new(id: &str, display_name: &str, kind: ActorKind) -> Result<Self, IdentityError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(IdentityError::EmptyId);
        }

        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(IdentityError::EmptyDisplayName);
        }

        let now = now_epoch_secs();
        Ok(Self {
            id: id.to_string(),
            display_name: display_name.to_string(),
            kind,
            roles: BTreeSet::new(),
            metadata: BTreeMap::new(),
            created_at_epoch_secs: now,
            updated_at_epoch_secs: now,
        })
    }

    pub fn add_role(&mut self, role: &str) -> Result<(), IdentityError> {
        let normalized = normalize_role(role).ok_or(IdentityError::InvalidRole)?;
        self.roles.insert(normalized);
        self.touch();
        Ok(())
    }

    pub fn remove_role(&mut self, role: &str) {
        let normalized = role.trim().to_ascii_lowercase();
        self.roles.remove(&normalized);
        self.touch();
    }

    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata
            .insert(key.trim().to_string(), value.to_string());
        self.touch();
    }

    pub fn has_role(&self, role: &str) -> bool {
        let normalized = role.trim().to_ascii_lowercase();
        self.roles.contains(&normalized)
    }

    fn touch(&mut self) {
        self.updated_at_epoch_secs = now_epoch_secs();
    }
}

fn normalize_role(raw: &str) -> Option<String> {
    let role = raw.trim().to_ascii_lowercase();
    if role.is_empty() {
        return None;
    }
    if role
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        Some(role)
    } else {
        None
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_identity_new_and_role_management_success_path() {
        let mut identity =
            ActorIdentity::new("operator-1", "Operator", ActorKind::Human).expect("valid");
        identity.add_role("admin").expect("valid role");
        identity.add_role("on_call").expect("valid role");
        identity.set_metadata("team", "runtime");

        assert!(identity.has_role("admin"));
        assert!(identity.has_role("on_call"));
        assert_eq!(identity.metadata.get("team"), Some(&"runtime".to_string()));
    }

    #[test]
    fn actor_identity_rejects_empty_id_negative_path() {
        let err = ActorIdentity::new("   ", "Operator", ActorKind::Human)
            .expect_err("empty id should fail");
        assert_eq!(err, IdentityError::EmptyId);
    }

    #[test]
    fn actor_identity_rejects_invalid_role_negative_path() {
        let mut identity =
            ActorIdentity::new("operator-1", "Operator", ActorKind::Human).expect("valid");
        let err = identity
            .add_role("Admin Role")
            .expect_err("invalid role should fail");
        assert_eq!(err, IdentityError::InvalidRole);
    }

    #[test]
    fn actor_identity_round_trips_json_success_path() {
        let mut identity =
            ActorIdentity::new("agent-1", "Planner", ActorKind::Agent).expect("valid");
        identity.add_role("planner").expect("role should be valid");

        let json = serde_json::to_string(&identity).expect("serialize should work");
        let decoded: ActorIdentity = serde_json::from_str(&json).expect("deserialize should work");
        assert_eq!(decoded.id, "agent-1");
        assert!(decoded.roles.contains("planner"));
    }
}
