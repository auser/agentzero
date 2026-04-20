//! Connector templates: generate `ConnectorManifest` + tool definitions from config.

#[cfg(feature = "connector-database")]
pub mod database;
#[cfg(feature = "connector-file")]
pub mod file;
#[cfg(feature = "connector-rest")]
pub mod rest_api;

use crate::{ConnectorConfig, ConnectorManifest, ConnectorType, EntitySchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Parameters for reading records from a connector entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {
    /// Entity to read from.
    pub entity: String,
    /// Resume from this cursor position (primary key or timestamp).
    #[serde(default)]
    pub cursor: Option<String>,
    /// Maximum number of records to return per batch.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
}

fn default_batch_size() -> u32 {
    100
}

/// Result of reading a batch of records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResult {
    /// The records retrieved (as JSON objects).
    pub records: Vec<serde_json::Value>,
    /// Cursor for the next batch. `None` means no more records.
    pub next_cursor: Option<String>,
}

/// Parameters for writing records to a connector entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    /// Entity to write to.
    pub entity: String,
    /// Records to upsert (as JSON objects).
    pub records: Vec<serde_json::Value>,
    /// Field used as the primary key for upsert semantics.
    pub primary_key: String,
}

/// Result of writing a batch of records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub written: u64,
    pub skipped: u64,
    pub errors: Vec<crate::SyncError>,
}

/// A connector template generates a manifest and tool definitions from
/// a user-provided `ConnectorConfig`.
///
/// Implementations exist for each `ConnectorType` (REST API, Database, File).
/// The registry dispatches to the correct template based on
/// `ConnectorConfig::connector_type`.
#[async_trait]
pub trait ConnectorTemplate: Send + Sync {
    /// Build a manifest describing the connector's entities and capabilities.
    fn manifest(&self, config: &ConnectorConfig) -> anyhow::Result<ConnectorManifest>;

    /// Discover the schema of the remote data source at runtime.
    ///
    /// Returns entity schemas introspected from the live source (e.g. by
    /// querying `information_schema` for databases, or parsing an OpenAPI
    /// spec for REST APIs).
    async fn discover_schema(&self, config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>>;

    /// Read a batch of records from an entity.
    ///
    /// Supports cursor-based pagination: pass `cursor: None` for the first
    /// batch, then pass the returned `next_cursor` for subsequent batches.
    async fn read_records(
        &self,
        config: &ConnectorConfig,
        request: &ReadRequest,
    ) -> anyhow::Result<ReadResult>;

    /// Write (upsert) records to an entity.
    ///
    /// Uses upsert semantics: records with an existing primary key are
    /// updated, new records are inserted. This makes syncs idempotent.
    async fn write_records(
        &self,
        config: &ConnectorConfig,
        request: &WriteRequest,
    ) -> anyhow::Result<WriteResult>;
}

/// Look up the template implementation for a connector type.
pub fn template_for_type(connector_type: ConnectorType) -> Box<dyn ConnectorTemplate> {
    match connector_type {
        #[cfg(feature = "connector-rest")]
        ConnectorType::RestApi => Box::new(rest_api::RestApiTemplate),
        #[cfg(feature = "connector-database")]
        ConnectorType::Database => Box::new(database::DatabaseTemplate),
        #[cfg(feature = "connector-file")]
        ConnectorType::File => Box::new(file::FileTemplate),
        #[allow(unreachable_patterns)]
        _ => Box::new(NullTemplate),
    }
}

/// Fallback template for disabled connector types.
struct NullTemplate;

#[async_trait]
impl ConnectorTemplate for NullTemplate {
    fn manifest(&self, config: &ConnectorConfig) -> anyhow::Result<ConnectorManifest> {
        anyhow::bail!(
            "connector type `{}` is not enabled in this build",
            config.connector_type
        )
    }

    async fn discover_schema(&self, config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>> {
        anyhow::bail!(
            "connector type `{}` is not enabled in this build",
            config.connector_type
        )
    }

    async fn read_records(
        &self,
        config: &ConnectorConfig,
        _request: &ReadRequest,
    ) -> anyhow::Result<ReadResult> {
        anyhow::bail!(
            "connector type `{}` is not enabled in this build",
            config.connector_type
        )
    }

    async fn write_records(
        &self,
        config: &ConnectorConfig,
        _request: &WriteRequest,
    ) -> anyhow::Result<WriteResult> {
        anyhow::bail!(
            "connector type `{}` is not enabled in this build",
            config.connector_type
        )
    }
}
