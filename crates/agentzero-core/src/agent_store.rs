//! Trait and types for persistent agent management.
//!
//! The trait lives in `agentzero-core` so that `agentzero-infra` (the tool
//! host) can depend on it without creating a circular dependency with
//! `agentzero-orchestrator` (where the concrete `AgentStore` lives).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Status of a dynamically-created agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Active,
    Stopped,
}

/// Channel configuration for a dynamic agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChannelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
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

/// Trait for persistent agent CRUD operations.
///
/// Implemented by `AgentStore` in `agentzero-orchestrator`. Used by
/// `AgentManageTool` in `agentzero-infra` to avoid a circular crate
/// dependency.
pub trait AgentStoreApi: Send + Sync {
    fn create(&self, record: AgentRecord) -> anyhow::Result<AgentRecord>;
    fn get(&self, agent_id: &str) -> Option<AgentRecord>;
    fn list(&self) -> Vec<AgentRecord>;
    fn update(&self, agent_id: &str, update: AgentUpdate) -> anyhow::Result<Option<AgentRecord>>;
    fn delete(&self, agent_id: &str) -> anyhow::Result<bool>;
    fn set_status(&self, agent_id: &str, status: AgentStatus) -> anyhow::Result<bool>;
    fn count(&self) -> usize;
}
