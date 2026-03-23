//! Persistent store for workflow definitions.
//!
//! Follows the [`AgentStore`] pattern: `RwLock<Vec<WorkflowRecord>>` with
//! optional `EncryptedJsonStore` backing for encrypted-at-rest persistence.

use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const WORKFLOWS_FILE: &str = "workflows.json";

/// Persistent record for a saved workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub workflow_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// ReactFlow nodes (serialized JSON array).
    pub nodes: Vec<serde_json::Value>,
    /// ReactFlow edges (serialized JSON array).
    pub edges: Vec<serde_json::Value>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Fields that can be updated on an existing workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<serde_json::Value>>,
}

/// Persistent store for workflow definitions.
///
/// When constructed with `persistent()`, every mutation is atomically flushed
/// to an encrypted JSON file. When constructed with `new()`, operates purely
/// in-memory (useful for tests).
pub struct WorkflowStore {
    workflows: std::sync::RwLock<Vec<WorkflowRecord>>,
    backing: Option<EncryptedJsonStore>,
}

impl Default for WorkflowStore {
    fn default() -> Self {
        Self {
            workflows: std::sync::RwLock::new(Vec::new()),
            backing: None,
        }
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl WorkflowStore {
    /// Create an in-memory-only store (no persistence). Useful for tests.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a persistent store backed by an encrypted JSON file.
    /// Loads any existing workflows from disk on construction.
    pub fn persistent(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, WORKFLOWS_FILE)?;
        let workflows: Vec<WorkflowRecord> = store.load_or_default()?;
        Ok(Self {
            workflows: std::sync::RwLock::new(workflows),
            backing: Some(store),
        })
    }

    /// Flush the current state to disk (if persistent).
    fn flush(&self) -> anyhow::Result<()> {
        if let Some(ref store) = self.backing {
            let guard = self.workflows.read().expect("workflow store lock poisoned");
            store.save(&*guard)?;
        }
        Ok(())
    }

    /// Create a new workflow. Generates a unique ID and timestamps.
    pub fn create(&self, mut record: WorkflowRecord) -> anyhow::Result<WorkflowRecord> {
        let now = now_epoch();
        if record.workflow_id.is_empty() {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            record.workflow_id = format!("wf-{now}-{nanos}");
        }
        record.created_at = now;
        record.updated_at = now;

        let mut guard = self
            .workflows
            .write()
            .expect("workflow store lock poisoned");
        if guard.iter().any(|w| w.workflow_id == record.workflow_id) {
            anyhow::bail!("workflow '{}' already exists", record.workflow_id);
        }
        guard.push(record.clone());
        drop(guard);
        self.flush()?;
        Ok(record)
    }

    /// Get a workflow by ID.
    pub fn get(&self, workflow_id: &str) -> Option<WorkflowRecord> {
        let guard = self.workflows.read().expect("workflow store lock poisoned");
        guard.iter().find(|w| w.workflow_id == workflow_id).cloned()
    }

    /// List all workflows.
    pub fn list(&self) -> Vec<WorkflowRecord> {
        let guard = self.workflows.read().expect("workflow store lock poisoned");
        guard.clone()
    }

    /// Update an existing workflow. Returns the updated record, or None if not found.
    pub fn update(
        &self,
        workflow_id: &str,
        update: WorkflowUpdate,
    ) -> anyhow::Result<Option<WorkflowRecord>> {
        let mut guard = self
            .workflows
            .write()
            .expect("workflow store lock poisoned");
        let Some(record) = guard.iter_mut().find(|w| w.workflow_id == workflow_id) else {
            return Ok(None);
        };

        if let Some(name) = update.name {
            record.name = name;
        }
        if let Some(description) = update.description {
            record.description = description;
        }
        if let Some(nodes) = update.nodes {
            record.nodes = nodes;
        }
        if let Some(edges) = update.edges {
            record.edges = edges;
        }
        record.updated_at = now_epoch();

        let updated = record.clone();
        drop(guard);
        self.flush()?;
        Ok(Some(updated))
    }

    /// Delete a workflow by ID. Returns true if it existed.
    pub fn delete(&self, workflow_id: &str) -> anyhow::Result<bool> {
        let mut guard = self
            .workflows
            .write()
            .expect("workflow store lock poisoned");
        let before = guard.len();
        guard.retain(|w| w.workflow_id != workflow_id);
        let removed = guard.len() < before;
        drop(guard);
        if removed {
            self.flush()?;
        }
        Ok(removed)
    }

    /// Return the number of stored workflows.
    pub fn count(&self) -> usize {
        let guard = self.workflows.read().expect("workflow store lock poisoned");
        guard.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_record(name: &str) -> WorkflowRecord {
        WorkflowRecord {
            workflow_id: String::new(),
            name: name.to_string(),
            description: "test workflow".to_string(),
            nodes: vec![json!({
                "id": "n1",
                "data": { "name": "agent1", "nodeType": "agent", "metadata": {} }
            })],
            edges: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn create_and_get() {
        let store = WorkflowStore::new();
        let created = store.create(sample_record("wf1")).expect("create");
        assert!(!created.workflow_id.is_empty());
        assert!(created.created_at > 0);

        let fetched = store.get(&created.workflow_id).expect("should exist");
        assert_eq!(fetched.name, "wf1");
    }

    #[test]
    fn list_workflows() {
        let store = WorkflowStore::new();
        store.create(sample_record("a")).expect("create a");
        store.create(sample_record("b")).expect("create b");
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn update_workflow() {
        let store = WorkflowStore::new();
        let created = store.create(sample_record("orig")).expect("create");
        let updated = store
            .update(
                &created.workflow_id,
                WorkflowUpdate {
                    name: Some("renamed".to_string()),
                    ..Default::default()
                },
            )
            .expect("update")
            .expect("should exist");
        assert_eq!(updated.name, "renamed");
    }

    #[test]
    fn delete_workflow() {
        let store = WorkflowStore::new();
        let created = store.create(sample_record("del")).expect("create");
        assert!(store.delete(&created.workflow_id).expect("delete"));
        assert!(store.get(&created.workflow_id).is_none());
    }

    #[test]
    fn duplicate_id_rejected() {
        let store = WorkflowStore::new();
        let mut rec = sample_record("dup");
        rec.workflow_id = "fixed-id".to_string();
        store.create(rec.clone()).expect("first create");
        assert!(store.create(rec).is_err());
    }
}
