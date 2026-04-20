//! Data source connector framework for AgentZero.
//!
//! Provides a schema-driven connector system that lets the AI agent discover,
//! link, and sync data between arbitrary sources (REST APIs, databases, files).
//! Connectors generate `DynamicToolDef` entries using the existing tool
//! infrastructure — no new core traits required.

pub mod event_sync;
pub mod registry;
pub mod sync_engine;
pub mod templates;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Connector manifest ───────────────────────────────────────────────

/// Describes a configured data source: its type, auth, entities, and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorManifest {
    pub name: String,
    pub connector_type: ConnectorType,
    pub auth: AuthConfig,
    pub entities: Vec<EntitySchema>,
    pub capabilities: ConnectorCaps,
}

/// The kind of data source a connector represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorType {
    RestApi,
    Database,
    File,
}

impl std::fmt::Display for ConnectorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RestApi => write!(f, "rest_api"),
            Self::Database => write!(f, "database"),
            Self::File => write!(f, "file"),
        }
    }
}

/// Authentication configuration for a connector.
///
/// Credentials are never stored in plaintext — values reference environment
/// variables via `*_env` fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    /// No authentication required.
    None,
    /// Static header (e.g. `X-API-Key: <value>`).
    Header {
        key: String,
        /// Name of the environment variable holding the secret value.
        value_env: String,
    },
    /// HTTP Basic authentication.
    Basic {
        username_env: String,
        password_env: String,
    },
    /// OAuth2 — delegates to an `agentzero-auth` profile.
    OAuth2 { profile: String },
    /// Connection string (databases).
    ConnectionString {
        /// Name of the environment variable holding the connection string.
        connection_string_env: String,
    },
}

/// What operations a connector supports.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConnectorCaps {
    pub read: bool,
    pub write: bool,
    pub list: bool,
    pub search: bool,
    /// Supports webhook / streaming subscription.
    pub subscribe: bool,
    /// Supports runtime schema introspection.
    pub discover_schema: bool,
}

impl Default for ConnectorCaps {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            list: true,
            search: false,
            subscribe: false,
            discover_schema: false,
        }
    }
}

// ── Entity schema ────────────────────────────────────────────────────

/// Schema for one entity type exposed by a connector (e.g. "orders", "contacts").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySchema {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub primary_key: String,
    /// Full JSON Schema for validation of records.
    #[serde(default)]
    pub json_schema: serde_json::Value,
}

/// Definition of a single field within an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

/// Primitive field types used for schema comparison and mapping suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    DateTime,
    /// Reference to another entity (foreign key).
    Reference(std::string::String),
    /// Arbitrary JSON object.
    Json,
    /// Binary / blob data.
    Binary,
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Number => write!(f, "number"),
            Self::Integer => write!(f, "integer"),
            Self::Boolean => write!(f, "boolean"),
            Self::DateTime => write!(f, "datetime"),
            Self::Reference(entity) => write!(f, "reference({entity})"),
            Self::Json => write!(f, "json"),
            Self::Binary => write!(f, "binary"),
        }
    }
}

// ── Data link ────────────────────────────────────────────────────────

/// A link between two data sources: defines how data flows from source to target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLink {
    pub id: String,
    pub name: String,
    pub source: DataEndpoint,
    pub target: DataEndpoint,
    pub field_mappings: Vec<FieldMapping>,
    pub sync_mode: SyncMode,
    /// Optional JSONata/jq-style transform expression applied to each record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
    /// Cursor for resumable sync — last processed primary key or timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_cursor: Option<String>,
    /// Unix timestamp of the last successful sync.
    #[serde(default)]
    pub last_sync_at: u64,
}

/// One side of a data link: a connector + entity pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataEndpoint {
    pub connector: String,
    pub entity: String,
}

/// Mapping of a single field from source to target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMapping {
    pub source_field: String,
    pub target_field: String,
    /// Optional per-field transform expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<String>,
}

/// How a data link is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMode {
    /// Triggered manually via the `data_sync` tool.
    OnDemand,
    /// Runs on a cron schedule (reuses existing `cron_executor`).
    Scheduled { cron: String },
    /// Triggered by events on the `EventBus`.
    EventDriven { event_topic: String },
}

// ── Sync result ──────────────────────────────────────────────────────

/// Summary of a sync execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub link_id: String,
    pub records_read: u64,
    pub records_written: u64,
    pub records_skipped: u64,
    pub records_failed: u64,
    pub errors: Vec<SyncError>,
    /// Updated cursor position after sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// A single record-level sync error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncError {
    pub record_key: String,
    pub message: String,
}

// ── Schema drift ─────────────────────────────────────────────────────

/// Warning about a schema change that may break existing data links.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftWarning {
    pub connector: String,
    pub entity: String,
    pub field: String,
    pub kind: DriftKind,
    pub affected_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    FieldRemoved,
    TypeChanged { was: FieldType, now: FieldType },
    RequiredAdded,
}

// ── Pagination ───────────────────────────────────────────────────────

/// Pagination strategy for list operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaginationStrategy {
    /// Follow a `next_cursor` field in the response body.
    Cursor {
        cursor_field: String,
        #[serde(default = "default_page_size")]
        page_size: u32,
    },
    /// Increment an `offset` parameter.
    Offset {
        #[serde(default = "default_page_size")]
        page_size: u32,
    },
    /// Follow RFC 5988 `Link: <url>; rel="next"` headers.
    LinkHeader,
    /// No pagination — single request returns all records.
    None,
}

fn default_page_size() -> u32 {
    100
}

// ── Rate limiting ────────────────────────────────────────────────────

/// Rate limit configuration for a connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per second (0 = unlimited).
    #[serde(default)]
    pub max_requests_per_second: f64,
    /// Maximum retries on 429 responses.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_max_retries() -> u32 {
    3
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_second: 0.0,
            max_retries: default_max_retries(),
        }
    }
}

// ── Connector config (parsed from TOML) ──────────────────────────────

/// User-facing configuration for a connector instance.
///
/// Parsed from `[[connectors]]` in `agentzero.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    /// Extra connector-specific settings (base_url, path, openapi_url, etc.).
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
    /// Authentication configuration.
    #[serde(default = "default_auth")]
    pub auth: AuthConfig,
    /// Privacy boundary for data flowing through this connector.
    #[serde(default)]
    pub privacy_boundary: String,
    /// Rate limiting configuration.
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    /// Pagination strategy for list operations.
    #[serde(default = "default_pagination")]
    pub pagination: PaginationStrategy,
    /// Batch size for write operations.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
}

fn default_auth() -> AuthConfig {
    AuthConfig::None
}

fn default_pagination() -> PaginationStrategy {
    PaginationStrategy::None
}

fn default_batch_size() -> u32 {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_type_display() {
        assert_eq!(ConnectorType::RestApi.to_string(), "rest_api");
        assert_eq!(ConnectorType::Database.to_string(), "database");
        assert_eq!(ConnectorType::File.to_string(), "file");
    }

    #[test]
    fn field_type_display() {
        assert_eq!(FieldType::String.to_string(), "string");
        assert_eq!(FieldType::Number.to_string(), "number");
        assert_eq!(FieldType::DateTime.to_string(), "datetime");
        assert_eq!(
            FieldType::Reference("orders".to_string()).to_string(),
            "reference(orders)"
        );
    }

    #[test]
    fn auth_config_roundtrips() {
        let auth = AuthConfig::Header {
            key: "X-API-Key".to_string(),
            value_env: "MY_API_KEY".to_string(),
        };
        let json = serde_json::to_string(&auth).expect("serialize");
        let back: AuthConfig = serde_json::from_str(&json).expect("deserialize");
        match back {
            AuthConfig::Header { key, value_env } => {
                assert_eq!(key, "X-API-Key");
                assert_eq!(value_env, "MY_API_KEY");
            }
            _ => panic!("expected Header variant"),
        }
    }

    #[test]
    fn sync_mode_roundtrips() {
        let modes = vec![
            SyncMode::OnDemand,
            SyncMode::Scheduled {
                cron: "0 */15 * * *".to_string(),
            },
            SyncMode::EventDriven {
                event_topic: "connector:shopify:orders:changed".to_string(),
            },
        ];
        for mode in &modes {
            let json = serde_json::to_string(mode).expect("serialize");
            let _back: SyncMode = serde_json::from_str(&json).expect("deserialize");
        }
    }

    #[test]
    fn connector_config_deserializes_from_toml() {
        let toml_str = r#"
            name = "test_api"
            type = "rest_api"
            base_url = "https://api.example.com/v1"
            [auth]
            type = "header"
            key = "Authorization"
            value_env = "API_TOKEN"
        "#;
        let config: ConnectorConfig = toml::from_str(toml_str).expect("parse TOML");
        assert_eq!(config.name, "test_api");
        assert_eq!(config.connector_type, ConnectorType::RestApi);
        assert_eq!(
            config.settings.get("base_url").and_then(|v| v.as_str()),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn data_link_roundtrips() {
        let link = DataLink {
            id: "link-1".to_string(),
            name: "shopify_to_db".to_string(),
            source: DataEndpoint {
                connector: "shopify".to_string(),
                entity: "orders".to_string(),
            },
            target: DataEndpoint {
                connector: "orders_db".to_string(),
                entity: "orders".to_string(),
            },
            field_mappings: vec![
                FieldMapping {
                    source_field: "id".to_string(),
                    target_field: "shopify_id".to_string(),
                    transform: None,
                },
                FieldMapping {
                    source_field: "total_price".to_string(),
                    target_field: "amount".to_string(),
                    transform: Some("parseFloat".to_string()),
                },
            ],
            sync_mode: SyncMode::Scheduled {
                cron: "0 */15 * * *".to_string(),
            },
            transform: None,
            last_sync_cursor: None,
            last_sync_at: 0,
        };
        let json = serde_json::to_string_pretty(&link).expect("serialize");
        let back: DataLink = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.field_mappings.len(), 2);
        assert_eq!(back.source.connector, "shopify");
    }

    #[test]
    fn entity_schema_with_fields() {
        let schema = EntitySchema {
            name: "contacts".to_string(),
            fields: vec![
                FieldDef {
                    name: "id".to_string(),
                    field_type: FieldType::Integer,
                    required: true,
                    description: "Primary key".to_string(),
                },
                FieldDef {
                    name: "email".to_string(),
                    field_type: FieldType::String,
                    required: true,
                    description: "Contact email".to_string(),
                },
                FieldDef {
                    name: "created_at".to_string(),
                    field_type: FieldType::DateTime,
                    required: false,
                    description: String::new(),
                },
            ],
            primary_key: "id".to_string(),
            json_schema: serde_json::json!({}),
        };
        let json = serde_json::to_string(&schema).expect("serialize");
        let back: EntitySchema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.fields.len(), 3);
        assert_eq!(back.primary_key, "id");
    }

    #[test]
    fn drift_warning_serializes() {
        let warning = DriftWarning {
            connector: "shopify".to_string(),
            entity: "orders".to_string(),
            field: "discount_code".to_string(),
            kind: DriftKind::FieldRemoved,
            affected_links: vec!["link-1".to_string()],
        };
        let json = serde_json::to_string(&warning).expect("serialize");
        assert!(json.contains("field_removed"));
    }

    #[test]
    fn sync_result_with_errors() {
        let result = SyncResult {
            link_id: "link-1".to_string(),
            records_read: 100,
            records_written: 97,
            records_skipped: 0,
            records_failed: 3,
            errors: vec![SyncError {
                record_key: "42".to_string(),
                message: "duplicate key".to_string(),
            }],
            cursor: Some("100".to_string()),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: SyncResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.records_failed, 3);
        assert_eq!(back.errors.len(), 1);
    }
}
