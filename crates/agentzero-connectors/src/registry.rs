//! Connector registry: loads connector configs, stores manifests and data links.

use crate::templates::{template_for_type, ReadRequest, ReadResult, WriteRequest, WriteResult};
use crate::{ConnectorConfig, ConnectorManifest, DataLink, DriftKind, DriftWarning, EntitySchema};
use agentzero_storage::EncryptedJsonStore;
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

const LINKS_FILE: &str = "data_links.json";

/// Central registry for configured connectors and their data links.
///
/// Loads connector manifests from config, stores discovered schemas,
/// and manages `DataLink` definitions for syncing data between sources.
/// Links are persisted to an encrypted JSON store for durability across restarts.
#[derive(Debug)]
pub struct ConnectorRegistry {
    /// Loaded connector configs keyed by name.
    configs: HashMap<String, ConnectorConfig>,
    /// Discovered/cached manifests keyed by connector name.
    manifests: HashMap<String, ConnectorManifest>,
    /// Active data links keyed by link ID.
    links: HashMap<String, DataLink>,
    /// Encrypted store for persisting data links. `None` for in-memory-only mode (tests).
    link_store: Option<EncryptedJsonStore>,
}

impl ConnectorRegistry {
    /// Create an empty registry (in-memory only — links are not persisted).
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
            manifests: HashMap::new(),
            links: HashMap::new(),
            link_store: None,
        }
    }

    /// Create a registry with encrypted persistence for data links.
    ///
    /// Links are stored in `{data_dir}/data_links.json` using AES-256-GCM encryption.
    /// Existing links are loaded on creation.
    pub fn with_persistence(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, LINKS_FILE)?;
        let persisted_links: Vec<DataLink> = store.load_or_default()?;

        let mut links = HashMap::new();
        for link in persisted_links {
            links.insert(link.id.clone(), link);
        }

        if !links.is_empty() {
            info!(count = links.len(), "loaded persisted data links");
        }

        Ok(Self {
            configs: HashMap::new(),
            manifests: HashMap::new(),
            links,
            link_store: Some(store),
        })
    }

    /// Load connectors from a list of configs (parsed from TOML).
    pub fn load_configs(&mut self, configs: Vec<ConnectorConfig>) {
        for config in configs {
            info!(connector = %config.name, r#type = %config.connector_type, "registered connector");
            self.configs.insert(config.name.clone(), config);
        }
    }

    /// Return a reference to all loaded connector configs.
    pub fn configs(&self) -> &HashMap<String, ConnectorConfig> {
        &self.configs
    }

    /// Return a reference to a specific connector config by name.
    pub fn config(&self, name: &str) -> Option<&ConnectorConfig> {
        self.configs.get(name)
    }

    /// Return a reference to a cached manifest by connector name.
    pub fn manifest(&self, name: &str) -> Option<&ConnectorManifest> {
        self.manifests.get(name)
    }

    /// Return all cached manifests.
    pub fn manifests(&self) -> &HashMap<String, ConnectorManifest> {
        &self.manifests
    }

    /// Discover the schema for a connector by calling its template.
    ///
    /// Updates the cached manifest with the discovered entities. Returns
    /// drift warnings if the schema has changed since the last discovery
    /// and existing data links reference removed/changed fields.
    pub async fn discover(
        &mut self,
        connector_name: &str,
    ) -> anyhow::Result<(Vec<EntitySchema>, Vec<DriftWarning>)> {
        let config = self
            .configs
            .get(connector_name)
            .ok_or_else(|| anyhow::anyhow!("connector `{connector_name}` not found"))?
            .clone();

        let template = template_for_type(config.connector_type);
        let new_entities = template.discover_schema(&config).await?;
        let manifest = template.manifest(&config)?;

        // Check for drift against previous manifest.
        let drift_warnings = if let Some(old_manifest) = self.manifests.get(connector_name) {
            detect_drift(connector_name, old_manifest, &new_entities, &self.links)
        } else {
            vec![]
        };

        for warning in &drift_warnings {
            warn!(
                connector = %warning.connector,
                entity = %warning.entity,
                field = %warning.field,
                "schema drift detected: {:?}",
                warning.kind
            );
        }

        // Update cached manifest.
        let mut updated_manifest = manifest;
        updated_manifest.entities = new_entities.clone();
        self.manifests
            .insert(connector_name.to_string(), updated_manifest);

        Ok((new_entities, drift_warnings))
    }

    // ── Data link management ─────────────────────────────────────────

    /// Add or update a data link. Persists to encrypted store if available.
    pub fn upsert_link(&mut self, link: DataLink) {
        info!(link_id = %link.id, name = %link.name, "upserted data link");
        self.links.insert(link.id.clone(), link);
        self.persist_links();
    }

    /// Remove a data link by ID. Persists to encrypted store if available.
    pub fn remove_link(&mut self, link_id: &str) -> Option<DataLink> {
        let removed = self.links.remove(link_id);
        if removed.is_some() {
            self.persist_links();
        }
        removed
    }

    /// Get a data link by ID.
    pub fn link(&self, link_id: &str) -> Option<&DataLink> {
        self.links.get(link_id)
    }

    /// Get a mutable reference to a data link by ID.
    pub fn link_mut(&mut self, link_id: &str) -> Option<&mut DataLink> {
        self.links.get_mut(link_id)
    }

    /// List all data links.
    pub fn links(&self) -> &HashMap<String, DataLink> {
        &self.links
    }

    /// Load persisted links (e.g. from encrypted JSON store on startup).
    pub fn load_links(&mut self, links: Vec<DataLink>) {
        for link in links {
            self.links.insert(link.id.clone(), link);
        }
    }

    /// Validate that all field mappings in a link still reference valid
    /// fields in the source and target schemas.
    pub fn validate_link_mappings(&self, link: &DataLink) -> Vec<String> {
        let mut errors = Vec::new();

        if let Some(source_manifest) = self.manifests.get(&link.source.connector) {
            if let Some(entity) = source_manifest
                .entities
                .iter()
                .find(|e| e.name == link.source.entity)
            {
                for mapping in &link.field_mappings {
                    if !entity.fields.iter().any(|f| f.name == mapping.source_field) {
                        errors.push(format!(
                            "source field `{}` not found in `{}.{}`",
                            mapping.source_field, link.source.connector, link.source.entity
                        ));
                    }
                }
            } else {
                errors.push(format!(
                    "source entity `{}` not found in connector `{}`",
                    link.source.entity, link.source.connector
                ));
            }
        }

        if let Some(target_manifest) = self.manifests.get(&link.target.connector) {
            if let Some(entity) = target_manifest
                .entities
                .iter()
                .find(|e| e.name == link.target.entity)
            {
                for mapping in &link.field_mappings {
                    if !entity.fields.iter().any(|f| f.name == mapping.target_field) {
                        errors.push(format!(
                            "target field `{}` not found in `{}.{}`",
                            mapping.target_field, link.target.connector, link.target.entity
                        ));
                    }
                }
            } else {
                errors.push(format!(
                    "target entity `{}` not found in connector `{}`",
                    link.target.entity, link.target.connector
                ));
            }
        }

        errors
    }

    /// Return the names of all registered connectors.
    pub fn connector_names(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }

    /// Read records from a connector entity.
    pub async fn read_records(
        &self,
        connector_name: &str,
        request: &ReadRequest,
    ) -> anyhow::Result<ReadResult> {
        let config = self
            .configs
            .get(connector_name)
            .ok_or_else(|| anyhow::anyhow!("connector `{connector_name}` not found"))?;

        let template = template_for_type(config.connector_type);
        template.read_records(config, request).await
    }

    /// Write records to a connector entity.
    pub async fn write_records(
        &self,
        connector_name: &str,
        request: &WriteRequest,
    ) -> anyhow::Result<WriteResult> {
        let config = self
            .configs
            .get(connector_name)
            .ok_or_else(|| anyhow::anyhow!("connector `{connector_name}` not found"))?;

        let template = template_for_type(config.connector_type);
        template.write_records(config, request).await
    }

    /// Serialize all data links to JSON for persistence.
    pub fn export_links(&self) -> serde_json::Value {
        let links: Vec<&DataLink> = self.links.values().collect();
        serde_json::to_value(links).unwrap_or_default()
    }

    /// Persist links to the encrypted store (if configured).
    fn persist_links(&self) {
        if let Some(ref store) = self.link_store {
            let links: Vec<&DataLink> = self.links.values().collect();
            if let Err(e) = store.save(&links) {
                warn!("failed to persist data links: {e}");
            }
        }
    }

    /// Persist cursor/timestamp updates after a sync completes.
    pub fn persist_sync_state(&self) {
        self.persist_links();
    }
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect schema drift between old and new entity schemas.
fn detect_drift(
    connector_name: &str,
    old_manifest: &ConnectorManifest,
    new_entities: &[EntitySchema],
    links: &HashMap<String, DataLink>,
) -> Vec<DriftWarning> {
    let mut warnings = Vec::new();

    for old_entity in &old_manifest.entities {
        let new_entity = new_entities.iter().find(|e| e.name == old_entity.name);

        let Some(new_entity) = new_entity else {
            // Entire entity disappeared — check if any links reference it.
            let affected: Vec<String> = links
                .values()
                .filter(|l| {
                    (l.source.connector == connector_name && l.source.entity == old_entity.name)
                        || (l.target.connector == connector_name
                            && l.target.entity == old_entity.name)
                })
                .map(|l| l.id.clone())
                .collect();

            if !affected.is_empty() {
                for field in &old_entity.fields {
                    warnings.push(DriftWarning {
                        connector: connector_name.to_string(),
                        entity: old_entity.name.clone(),
                        field: field.name.clone(),
                        kind: DriftKind::FieldRemoved,
                        affected_links: affected.clone(),
                    });
                }
            }
            continue;
        };

        // Check each field in the old entity.
        for old_field in &old_entity.fields {
            let new_field = new_entity.fields.iter().find(|f| f.name == old_field.name);

            // Find links that reference this field.
            let affected: Vec<String> = links
                .values()
                .filter(|l| {
                    let source_match = l.source.connector == connector_name
                        && l.source.entity == old_entity.name
                        && l.field_mappings
                            .iter()
                            .any(|m| m.source_field == old_field.name);
                    let target_match = l.target.connector == connector_name
                        && l.target.entity == old_entity.name
                        && l.field_mappings
                            .iter()
                            .any(|m| m.target_field == old_field.name);
                    source_match || target_match
                })
                .map(|l| l.id.clone())
                .collect();

            if affected.is_empty() {
                continue;
            }

            match new_field {
                None => {
                    warnings.push(DriftWarning {
                        connector: connector_name.to_string(),
                        entity: old_entity.name.clone(),
                        field: old_field.name.clone(),
                        kind: DriftKind::FieldRemoved,
                        affected_links: affected,
                    });
                }
                Some(new_f) if new_f.field_type != old_field.field_type => {
                    warnings.push(DriftWarning {
                        connector: connector_name.to_string(),
                        entity: old_entity.name.clone(),
                        field: old_field.name.clone(),
                        kind: DriftKind::TypeChanged {
                            was: old_field.field_type.clone(),
                            now: new_f.field_type.clone(),
                        },
                        affected_links: affected,
                    });
                }
                _ => {}
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn test_config(name: &str) -> ConnectorConfig {
        ConnectorConfig {
            name: name.to_string(),
            connector_type: ConnectorType::File,
            settings: HashMap::new(),
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: RateLimitConfig::default(),
            pagination: PaginationStrategy::None,
            batch_size: 100,
        }
    }

    fn test_entity(name: &str, fields: &[(&str, FieldType)]) -> EntitySchema {
        EntitySchema {
            name: name.to_string(),
            fields: fields
                .iter()
                .map(|(n, t)| FieldDef {
                    name: n.to_string(),
                    field_type: t.clone(),
                    required: false,
                    description: String::new(),
                })
                .collect(),
            primary_key: fields
                .first()
                .map(|(n, _)| n.to_string())
                .unwrap_or_default(),
            json_schema: serde_json::json!({}),
        }
    }

    #[test]
    fn registry_loads_configs() {
        let mut reg = ConnectorRegistry::new();
        reg.load_configs(vec![test_config("src"), test_config("dst")]);
        assert_eq!(reg.connector_names().len(), 2);
        assert!(reg.config("src").is_some());
        assert!(reg.config("missing").is_none());
    }

    #[test]
    fn link_crud() {
        let mut reg = ConnectorRegistry::new();
        let link = DataLink {
            id: "l1".to_string(),
            name: "test link".to_string(),
            source: DataEndpoint {
                connector: "src".to_string(),
                entity: "orders".to_string(),
            },
            target: DataEndpoint {
                connector: "dst".to_string(),
                entity: "orders".to_string(),
            },
            field_mappings: vec![],
            sync_mode: SyncMode::OnDemand,
            transform: None,
            last_sync_cursor: None,
            last_sync_at: 0,
        };
        reg.upsert_link(link.clone());
        assert!(reg.link("l1").is_some());
        assert_eq!(reg.links().len(), 1);

        let removed = reg.remove_link("l1");
        assert!(removed.is_some());
        assert!(reg.link("l1").is_none());
    }

    #[test]
    fn validate_link_mappings_catches_missing_fields() {
        let mut reg = ConnectorRegistry::new();
        reg.load_configs(vec![test_config("src"), test_config("dst")]);

        // Insert manifests with known schemas.
        let src_manifest = ConnectorManifest {
            name: "src".to_string(),
            connector_type: ConnectorType::File,
            auth: AuthConfig::None,
            entities: vec![test_entity(
                "orders",
                &[("id", FieldType::Integer), ("total", FieldType::Number)],
            )],
            capabilities: ConnectorCaps::default(),
        };
        let dst_manifest = ConnectorManifest {
            name: "dst".to_string(),
            connector_type: ConnectorType::Database,
            auth: AuthConfig::None,
            entities: vec![test_entity(
                "orders",
                &[
                    ("order_id", FieldType::Integer),
                    ("amount", FieldType::Number),
                ],
            )],
            capabilities: ConnectorCaps::default(),
        };
        reg.manifests.insert("src".to_string(), src_manifest);
        reg.manifests.insert("dst".to_string(), dst_manifest);

        let link = DataLink {
            id: "l1".to_string(),
            name: "test".to_string(),
            source: DataEndpoint {
                connector: "src".to_string(),
                entity: "orders".to_string(),
            },
            target: DataEndpoint {
                connector: "dst".to_string(),
                entity: "orders".to_string(),
            },
            field_mappings: vec![
                FieldMapping {
                    source_field: "id".to_string(),
                    target_field: "order_id".to_string(),
                    transform: None,
                },
                FieldMapping {
                    source_field: "missing_field".to_string(),
                    target_field: "amount".to_string(),
                    transform: None,
                },
            ],
            sync_mode: SyncMode::OnDemand,
            transform: None,
            last_sync_cursor: None,
            last_sync_at: 0,
        };

        let errors = reg.validate_link_mappings(&link);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing_field"));
    }

    #[test]
    fn drift_detection_field_removed() {
        let old_manifest = ConnectorManifest {
            name: "src".to_string(),
            connector_type: ConnectorType::RestApi,
            auth: AuthConfig::None,
            entities: vec![test_entity(
                "orders",
                &[
                    ("id", FieldType::Integer),
                    ("discount_code", FieldType::String),
                ],
            )],
            capabilities: ConnectorCaps::default(),
        };

        let new_entities = vec![test_entity("orders", &[("id", FieldType::Integer)])];

        let mut links = HashMap::new();
        links.insert(
            "l1".to_string(),
            DataLink {
                id: "l1".to_string(),
                name: "test".to_string(),
                source: DataEndpoint {
                    connector: "src".to_string(),
                    entity: "orders".to_string(),
                },
                target: DataEndpoint {
                    connector: "dst".to_string(),
                    entity: "orders".to_string(),
                },
                field_mappings: vec![FieldMapping {
                    source_field: "discount_code".to_string(),
                    target_field: "discount".to_string(),
                    transform: None,
                }],
                sync_mode: SyncMode::OnDemand,
                transform: None,
                last_sync_cursor: None,
                last_sync_at: 0,
            },
        );

        let warnings = detect_drift("src", &old_manifest, &new_entities, &links);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "discount_code");
        assert!(matches!(warnings[0].kind, DriftKind::FieldRemoved));
        assert_eq!(warnings[0].affected_links, vec!["l1"]);
    }

    #[test]
    fn drift_detection_type_changed() {
        let old_manifest = ConnectorManifest {
            name: "src".to_string(),
            connector_type: ConnectorType::RestApi,
            auth: AuthConfig::None,
            entities: vec![test_entity("orders", &[("total", FieldType::Number)])],
            capabilities: ConnectorCaps::default(),
        };

        let new_entities = vec![test_entity("orders", &[("total", FieldType::String)])];

        let mut links = HashMap::new();
        links.insert(
            "l1".to_string(),
            DataLink {
                id: "l1".to_string(),
                name: "test".to_string(),
                source: DataEndpoint {
                    connector: "src".to_string(),
                    entity: "orders".to_string(),
                },
                target: DataEndpoint {
                    connector: "dst".to_string(),
                    entity: "orders".to_string(),
                },
                field_mappings: vec![FieldMapping {
                    source_field: "total".to_string(),
                    target_field: "amount".to_string(),
                    transform: None,
                }],
                sync_mode: SyncMode::OnDemand,
                transform: None,
                last_sync_cursor: None,
                last_sync_at: 0,
            },
        );

        let warnings = detect_drift("src", &old_manifest, &new_entities, &links);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].kind, DriftKind::TypeChanged { .. }));
    }

    #[test]
    fn no_drift_when_no_links_affected() {
        let old_manifest = ConnectorManifest {
            name: "src".to_string(),
            connector_type: ConnectorType::RestApi,
            auth: AuthConfig::None,
            entities: vec![test_entity(
                "orders",
                &[("id", FieldType::Integer), ("removed", FieldType::String)],
            )],
            capabilities: ConnectorCaps::default(),
        };

        let new_entities = vec![test_entity("orders", &[("id", FieldType::Integer)])];

        // No links reference the removed field.
        let links = HashMap::new();

        let warnings = detect_drift("src", &old_manifest, &new_entities, &links);
        assert!(warnings.is_empty());
    }
}
