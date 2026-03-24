//! Persistent store for workflow templates.
//!
//! Separate from [`WorkflowStore`] — templates are immutable snapshots
//! meant to be shared and reused, while workflows are live/mutable.

use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const TEMPLATES_FILE: &str = "templates.json";

/// Persistent record for a saved workflow template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateRecord {
    pub template_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub version: u32,
    /// ReactFlow nodes (serialized JSON array).
    pub nodes: Vec<serde_json::Value>,
    /// ReactFlow edges (serialized JSON array).
    pub edges: Vec<serde_json::Value>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Fields that can be updated on an existing template.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<serde_json::Value>>,
}

/// Persistent store for workflow templates.
pub struct TemplateStore {
    templates: std::sync::RwLock<Vec<TemplateRecord>>,
    backing: Option<EncryptedJsonStore>,
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self {
            templates: std::sync::RwLock::new(Vec::new()),
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

impl TemplateStore {
    /// Create an in-memory-only store (no persistence). Useful for tests.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a persistent store backed by an encrypted JSON file.
    pub fn persistent(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, TEMPLATES_FILE)?;
        let templates: Vec<TemplateRecord> = store.load_or_default()?;
        Ok(Self {
            templates: std::sync::RwLock::new(templates),
            backing: Some(store),
        })
    }

    fn flush(&self) -> anyhow::Result<()> {
        if let Some(ref store) = self.backing {
            let guard = self.templates.read().expect("template store lock poisoned");
            store.save(&*guard)?;
        }
        Ok(())
    }

    /// Create a new template. Generates a unique ID and timestamps.
    pub fn create(&self, mut record: TemplateRecord) -> anyhow::Result<TemplateRecord> {
        let now = now_epoch();
        if record.template_id.is_empty() {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            record.template_id = format!("tmpl-{now}-{nanos}");
        }
        if record.version == 0 {
            record.version = 1;
        }
        record.created_at = now;
        record.updated_at = now;

        let mut guard = self
            .templates
            .write()
            .expect("template store lock poisoned");
        if guard.iter().any(|t| t.template_id == record.template_id) {
            anyhow::bail!("template '{}' already exists", record.template_id);
        }
        guard.push(record.clone());
        drop(guard);
        self.flush()?;
        Ok(record)
    }

    /// Get a template by ID.
    pub fn get(&self, template_id: &str) -> Option<TemplateRecord> {
        let guard = self.templates.read().expect("template store lock poisoned");
        guard.iter().find(|t| t.template_id == template_id).cloned()
    }

    /// List all templates.
    pub fn list(&self) -> Vec<TemplateRecord> {
        let guard = self.templates.read().expect("template store lock poisoned");
        guard.clone()
    }

    /// Update an existing template. Bumps version automatically.
    pub fn update(
        &self,
        template_id: &str,
        update: TemplateUpdate,
    ) -> anyhow::Result<Option<TemplateRecord>> {
        let mut guard = self
            .templates
            .write()
            .expect("template store lock poisoned");
        let Some(record) = guard.iter_mut().find(|t| t.template_id == template_id) else {
            return Ok(None);
        };

        if let Some(name) = update.name {
            record.name = name;
        }
        if let Some(description) = update.description {
            record.description = description;
        }
        if let Some(category) = update.category {
            record.category = category;
        }
        if let Some(tags) = update.tags {
            record.tags = tags;
        }
        if let Some(nodes) = update.nodes {
            record.nodes = nodes;
            record.version += 1;
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

    /// Delete a template by ID. Returns true if it existed.
    pub fn delete(&self, template_id: &str) -> anyhow::Result<bool> {
        let mut guard = self
            .templates
            .write()
            .expect("template store lock poisoned");
        let before = guard.len();
        guard.retain(|t| t.template_id != template_id);
        let removed = guard.len() < before;
        drop(guard);
        if removed {
            self.flush()?;
        }
        Ok(removed)
    }

    /// Return the number of stored templates.
    pub fn count(&self) -> usize {
        let guard = self.templates.read().expect("template store lock poisoned");
        guard.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_template(name: &str) -> TemplateRecord {
        TemplateRecord {
            template_id: String::new(),
            name: name.to_string(),
            description: "test template".to_string(),
            category: "custom".to_string(),
            tags: vec!["test".to_string()],
            version: 0,
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
        let store = TemplateStore::new();
        let created = store.create(sample_template("t1")).expect("create");
        assert!(created.template_id.starts_with("tmpl-"));
        assert_eq!(created.version, 1);
        assert!(created.created_at > 0);

        let fetched = store.get(&created.template_id).expect("should exist");
        assert_eq!(fetched.name, "t1");
        assert_eq!(fetched.category, "custom");
    }

    #[test]
    fn list_templates() {
        let store = TemplateStore::new();
        store.create(sample_template("a")).expect("create a");
        store.create(sample_template("b")).expect("create b");
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn update_bumps_version() {
        let store = TemplateStore::new();
        let created = store.create(sample_template("orig")).expect("create");
        assert_eq!(created.version, 1);

        let updated = store
            .update(
                &created.template_id,
                TemplateUpdate {
                    nodes: Some(vec![json!({"id": "n2"})]),
                    ..Default::default()
                },
            )
            .expect("update")
            .expect("should exist");
        assert_eq!(updated.version, 2);
    }

    #[test]
    fn delete_template() {
        let store = TemplateStore::new();
        let created = store.create(sample_template("del")).expect("create");
        assert!(store.delete(&created.template_id).expect("delete"));
        assert!(store.get(&created.template_id).is_none());
    }

    #[test]
    fn duplicate_id_rejected() {
        let store = TemplateStore::new();
        let mut rec = sample_template("dup");
        rec.template_id = "fixed-id".to_string();
        store.create(rec.clone()).expect("first create");
        assert!(store.create(rec).is_err());
    }
}
