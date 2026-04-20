//! REST API connector template.
//!
//! Generates a connector manifest for any REST API given a base URL and
//! optional OpenAPI spec URL for schema auto-discovery.

use crate::templates::{ConnectorTemplate, ReadRequest, ReadResult, WriteRequest, WriteResult};
use crate::{
    AuthConfig, ConnectorCaps, ConnectorConfig, ConnectorManifest, ConnectorType, EntitySchema,
    FieldDef, FieldType, SyncError,
};
use async_trait::async_trait;

/// Template for generic REST API connectors.
pub struct RestApiTemplate;

#[async_trait]
impl ConnectorTemplate for RestApiTemplate {
    fn manifest(&self, config: &ConnectorConfig) -> anyhow::Result<ConnectorManifest> {
        let _base_url = config
            .settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("REST API connector requires `base_url`"))?;

        let has_openapi = config
            .settings
            .get("openapi_url")
            .and_then(|v| v.as_str())
            .is_some();

        Ok(ConnectorManifest {
            name: config.name.clone(),
            connector_type: ConnectorType::RestApi,
            auth: config.auth.clone(),
            entities: vec![], // populated by discover_schema
            capabilities: ConnectorCaps {
                read: true,
                write: true,
                list: true,
                search: true,
                subscribe: false,
                discover_schema: has_openapi,
            },
        })
    }

    async fn discover_schema(&self, config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>> {
        let openapi_url = config.settings.get("openapi_url").and_then(|v| v.as_str());

        match openapi_url {
            Some(url) => discover_from_openapi(url, config).await,
            None => {
                // Without an OpenAPI spec, return a generic record entity
                // that accepts any JSON. The AI agent can refine this by
                // inspecting actual API responses.
                Ok(vec![generic_record_entity(&config.name)])
            }
        }
    }

    async fn read_records(
        &self,
        config: &ConnectorConfig,
        request: &ReadRequest,
    ) -> anyhow::Result<ReadResult> {
        let base_url = config
            .settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("REST API connector requires `base_url`"))?;

        let client = build_client(config)?;

        // Build the list endpoint URL: base_url/entity
        let mut url = format!("{}/{}", base_url.trim_end_matches('/'), request.entity);

        // Add pagination query parameters.
        let mut query_params = Vec::new();
        query_params.push(format!("limit={}", request.batch_size));
        if let Some(ref cursor) = request.cursor {
            // Support both offset-based and cursor-based APIs.
            query_params.push(format!("cursor={}", cursor));
            query_params.push(format!("offset={}", cursor));
        }
        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        let url_clone = url.clone();
        let resp = send_with_retry(&client, |c| c.get(&url_clone), config).await?;

        if !resp.status().is_success() {
            anyhow::bail!("GET {} returned HTTP {}", url, resp.status());
        }

        let body: serde_json::Value = resp.json().await?;

        // Extract next cursor from response before consuming body.
        let next_cursor_from_body = body
            .as_object()
            .and_then(|obj| {
                obj.get("next_cursor")
                    .or_else(|| obj.get("cursor"))
                    .or_else(|| obj.get("next_page"))
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Try to extract records from common response shapes.
        let records = match body {
            serde_json::Value::Array(arr) => arr,
            serde_json::Value::Object(ref obj) => {
                // Look for common data wrapper keys.
                obj.get("data")
                    .or_else(|| obj.get("results"))
                    .or_else(|| obj.get("items"))
                    .or_else(|| obj.get(&request.entity))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_else(|| vec![body.clone()])
            }
            other => vec![other],
        };

        // Use explicit cursor from response, or fall back to offset-based.
        let next_cursor = next_cursor_from_body.or_else(|| {
            // If no explicit cursor, use offset-based pagination.
            if records.len() == request.batch_size as usize {
                let current_offset: u64 = request
                    .cursor
                    .as_deref()
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
                Some((current_offset + records.len() as u64).to_string())
            } else {
                None
            }
        });

        Ok(ReadResult {
            records,
            next_cursor,
        })
    }

    async fn write_records(
        &self,
        config: &ConnectorConfig,
        request: &WriteRequest,
    ) -> anyhow::Result<WriteResult> {
        let base_url = config
            .settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("REST API connector requires `base_url`"))?;

        let client = build_client(config)?;
        let endpoint = format!("{}/{}", base_url.trim_end_matches('/'), request.entity);

        let mut written = 0u64;
        let mut errors = Vec::new();

        for record in &request.records {
            let record_key = record
                .get(&request.primary_key)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            // Try PUT for upsert (update-or-create).
            let pk_val = record.get(&request.primary_key);
            let (method, url) = if let Some(pk) = pk_val {
                let pk_str = match pk {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                // PUT /entity/id for upsert.
                (reqwest::Method::PUT, format!("{}/{}", endpoint, pk_str))
            } else {
                // POST /entity for create.
                (reqwest::Method::POST, endpoint.clone())
            };

            let method_clone = method.clone();
            let url_clone = url.clone();
            let record_clone = record.clone();
            let resp = send_with_retry(
                &client,
                |c| {
                    c.request(method_clone.clone(), &url_clone)
                        .json(&record_clone)
                },
                config,
            )
            .await;

            match resp {
                Ok(r) if r.status().is_success() || r.status().as_u16() == 201 => {
                    written += 1;
                }
                Ok(r) => {
                    let status = r.status();
                    let body = r.text().await.unwrap_or_default();
                    errors.push(SyncError {
                        record_key,
                        message: format!("HTTP {} — {}", status, body),
                    });
                }
                Err(e) => {
                    errors.push(SyncError {
                        record_key,
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(WriteResult {
            written,
            skipped: 0,
            errors,
        })
    }
}

/// Discover entity schemas from an OpenAPI spec.
async fn discover_from_openapi(
    openapi_url: &str,
    config: &ConnectorConfig,
) -> anyhow::Result<Vec<EntitySchema>> {
    let client = build_client(config)?;
    let resp = client.get(openapi_url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "failed to fetch OpenAPI spec from `{}`: HTTP {}",
            openapi_url,
            resp.status()
        );
    }

    let spec: serde_json::Value = resp.json().await?;
    parse_openapi_schemas(&spec)
}

/// Parse entity schemas from an OpenAPI JSON spec.
///
/// Extracts schemas from `components.schemas` (OpenAPI 3.x) or
/// `definitions` (Swagger 2.x).
fn parse_openapi_schemas(spec: &serde_json::Value) -> anyhow::Result<Vec<EntitySchema>> {
    let schemas = spec
        .pointer("/components/schemas")
        .or_else(|| spec.get("definitions"))
        .and_then(|v| v.as_object());

    let Some(schemas) = schemas else {
        return Ok(vec![]);
    };

    let mut entities = Vec::new();

    for (name, schema) in schemas {
        // Skip non-object schemas.
        let schema_type = schema.get("type").and_then(|v| v.as_str());
        if schema_type != Some("object") && schema.get("properties").is_none() {
            continue;
        }

        let properties = schema.get("properties").and_then(|v| v.as_object());

        let required_fields: Vec<&str> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let fields: Vec<FieldDef> = match properties {
            Some(props) => props
                .iter()
                .map(|(field_name, field_schema)| {
                    let field_type = openapi_type_to_field_type(field_schema);
                    let description = field_schema
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    FieldDef {
                        name: field_name.clone(),
                        field_type,
                        required: required_fields.contains(&field_name.as_str()),
                        description,
                    }
                })
                .collect(),
            None => vec![],
        };

        // Infer primary key: look for "id" field, or first required integer field.
        let primary_key = fields
            .iter()
            .find(|f| f.name == "id")
            .or_else(|| {
                fields.iter().find(|f| {
                    f.required && matches!(f.field_type, FieldType::Integer | FieldType::String)
                })
            })
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "id".to_string());

        entities.push(EntitySchema {
            name: name.clone(),
            fields,
            primary_key,
            json_schema: schema.clone(),
        });
    }

    Ok(entities)
}

/// Map OpenAPI type/format to our FieldType.
fn openapi_type_to_field_type(schema: &serde_json::Value) -> FieldType {
    let type_str = schema.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let format_str = schema.get("format").and_then(|v| v.as_str()).unwrap_or("");

    match (type_str, format_str) {
        ("string", "date-time") | ("string", "date") => FieldType::DateTime,
        ("string", "binary") | ("string", "byte") => FieldType::Binary,
        ("string", _) => FieldType::String,
        ("integer", _) => FieldType::Integer,
        ("number", _) => FieldType::Number,
        ("boolean", _) => FieldType::Boolean,
        ("object", _) => FieldType::Json,
        ("array", _) => FieldType::Json,
        _ => {
            // Check for $ref (reference to another schema).
            if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
                let ref_name = ref_str.rsplit('/').next().unwrap_or(ref_str);
                FieldType::Reference(ref_name.to_string())
            } else {
                FieldType::Json
            }
        }
    }
}

/// Build an HTTP client with auth headers from connector config.
fn build_client(config: &ConnectorConfig) -> anyhow::Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();

    match &config.auth {
        AuthConfig::Header { key, value_env } => {
            let value = std::env::var(value_env).map_err(|_| {
                anyhow::anyhow!(
                    "environment variable `{value_env}` not set for connector `{}`",
                    config.name
                )
            })?;
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
                reqwest::header::HeaderValue::from_str(&value)?,
            );
        }
        AuthConfig::Basic {
            username_env,
            password_env,
        } => {
            let username = std::env::var(username_env)
                .map_err(|_| anyhow::anyhow!("environment variable `{username_env}` not set"))?;
            let password = std::env::var(password_env)
                .map_err(|_| anyhow::anyhow!("environment variable `{password_env}` not set"))?;
            let encoded = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{username}:{password}"),
            );
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Basic {encoded}"))?,
            );
        }
        AuthConfig::OAuth2 { profile } => {
            // Resolve OAuth2 token from agentzero-auth profile.
            let data_dir = agentzero_core::common::paths::default_data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from(".agentzero"));
            match agentzero_auth::AuthManager::in_config_dir(&data_dir) {
                Ok(auth_mgr) => match auth_mgr.resolve_credential(Some(profile), "oauth2") {
                    Ok(Some(cred)) => {
                        headers.insert(
                            reqwest::header::AUTHORIZATION,
                            reqwest::header::HeaderValue::from_str(&format!(
                                "Bearer {}",
                                cred.token
                            ))?,
                        );
                    }
                    Ok(None) => {
                        tracing::warn!(
                            profile = %profile,
                            "OAuth2 profile not found — requests will be unauthenticated"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            profile = %profile,
                            "failed to resolve OAuth2 credential: {e}"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!("failed to open auth manager: {e}");
                }
            }
        }
        _ => {}
    }

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

/// Send an HTTP request with retry on 429 (rate limit) and 401 (token refresh).
///
/// Retries up to `max_retries` times on 429 responses, using the `Retry-After`
/// header (or exponential backoff with jitter) for delay. On 401 with OAuth2
/// auth, rebuilds the HTTP client with a freshly-resolved token and retries once.
///
/// Takes a `RequestBuilder` factory (closure) so the body can be re-sent on retry.
async fn send_with_retry<F>(
    client: &reqwest::Client,
    build_request: F,
    config: &ConnectorConfig,
) -> anyhow::Result<reqwest::Response>
where
    F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
{
    let max_retries = config.rate_limit.max_retries;
    let mut attempt = 0u32;
    let mut tried_token_refresh = false;
    // Start with the provided client; may be replaced on 401 retry.
    let mut active_client = client.clone();

    loop {
        let resp = build_request(&active_client).send().await?;
        let status = resp.status().as_u16();

        if status == 429 && attempt < max_retries {
            attempt += 1;

            let delay_secs = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    let base = 1u64 << attempt.min(5);
                    let jitter = base / 4;
                    base + (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64 % jitter.max(1))
                        .unwrap_or(0))
                });

            tracing::debug!(attempt, delay_secs, "rate limited (429), retrying");
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            continue;
        }

        // On 401 with OAuth2, rebuild the client with a fresh token and retry once.
        if status == 401 && !tried_token_refresh {
            if let AuthConfig::OAuth2 { ref profile } = config.auth {
                tried_token_refresh = true;
                tracing::debug!(profile = %profile, "401 received, refreshing token and rebuilding client");

                let data_dir = agentzero_core::common::paths::default_data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from(".agentzero"));
                if let Ok(auth_mgr) = agentzero_auth::AuthManager::in_config_dir(&data_dir) {
                    if auth_mgr.resolve_credential(Some(profile), "oauth2").is_ok() {
                        // Rebuild the client — build_client re-resolves the token from the
                        // auth manager, which now has the refreshed credential.
                        match build_client(config) {
                            Ok(new_client) => {
                                active_client = new_client;
                                tracing::info!(profile = %profile, "client rebuilt with fresh token, retrying");
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!(profile = %profile, error = %e, "failed to rebuild client");
                            }
                        }
                    }
                }
                tracing::warn!(profile = %profile, "token refresh failed, returning 401");
            }
        }

        if status == 429 {
            anyhow::bail!("rate limited after {max_retries} retries");
        }

        return Ok(resp);
    }
}

/// Generic record entity for connectors without schema discovery.
fn generic_record_entity(connector_name: &str) -> EntitySchema {
    EntitySchema {
        name: "records".to_string(),
        fields: vec![
            FieldDef {
                name: "id".to_string(),
                field_type: FieldType::String,
                required: true,
                description: "Record identifier".to_string(),
            },
            FieldDef {
                name: "data".to_string(),
                field_type: FieldType::Json,
                required: false,
                description: format!("Raw JSON data from {connector_name}"),
            },
        ],
        primary_key: "id".to_string(),
        json_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "data": {"type": "object"}
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn rest_config(name: &str, base_url: &str) -> ConnectorConfig {
        let mut settings = HashMap::new();
        settings.insert(
            "base_url".to_string(),
            serde_json::Value::String(base_url.to_string()),
        );
        ConnectorConfig {
            name: name.to_string(),
            connector_type: ConnectorType::RestApi,
            settings,
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: crate::RateLimitConfig::default(),
            pagination: crate::PaginationStrategy::None,
            batch_size: 100,
        }
    }

    #[test]
    fn manifest_requires_base_url() {
        let config = ConnectorConfig {
            name: "test".to_string(),
            connector_type: ConnectorType::RestApi,
            settings: HashMap::new(),
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: crate::RateLimitConfig::default(),
            pagination: crate::PaginationStrategy::None,
            batch_size: 100,
        };
        let template = RestApiTemplate;
        let result = template.manifest(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("base_url"));
    }

    #[test]
    fn manifest_basic() {
        let config = rest_config("shopify", "https://api.example.com");
        let template = RestApiTemplate;
        let manifest = template.manifest(&config).expect("manifest");
        assert_eq!(manifest.name, "shopify");
        assert_eq!(manifest.connector_type, ConnectorType::RestApi);
        assert!(manifest.capabilities.read);
        assert!(manifest.capabilities.write);
    }

    #[test]
    fn parse_openapi_schemas_basic() {
        let spec = serde_json::json!({
            "components": {
                "schemas": {
                    "Order": {
                        "type": "object",
                        "required": ["id", "total"],
                        "properties": {
                            "id": {"type": "integer"},
                            "total": {"type": "number"},
                            "status": {"type": "string"},
                            "created_at": {"type": "string", "format": "date-time"}
                        }
                    },
                    "ErrorResponse": {
                        "type": "string"
                    }
                }
            }
        });

        let entities = parse_openapi_schemas(&spec).expect("parse");
        assert_eq!(entities.len(), 1); // ErrorResponse skipped (not object)
        let order = &entities[0];
        assert_eq!(order.name, "Order");
        assert_eq!(order.primary_key, "id");
        assert_eq!(order.fields.len(), 4);

        let id_field = order.fields.iter().find(|f| f.name == "id").expect("id");
        assert_eq!(id_field.field_type, FieldType::Integer);
        assert!(id_field.required);

        let created = order
            .fields
            .iter()
            .find(|f| f.name == "created_at")
            .expect("created_at");
        assert_eq!(created.field_type, FieldType::DateTime);
    }

    #[test]
    fn openapi_type_mapping() {
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"type": "string"})),
            FieldType::String
        );
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"type": "integer"})),
            FieldType::Integer
        );
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"type": "number"})),
            FieldType::Number
        );
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"type": "boolean"})),
            FieldType::Boolean
        );
        assert_eq!(
            openapi_type_to_field_type(
                &serde_json::json!({"type": "string", "format": "date-time"})
            ),
            FieldType::DateTime
        );
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"type": "string", "format": "binary"})),
            FieldType::Binary
        );
        assert_eq!(
            openapi_type_to_field_type(&serde_json::json!({"$ref": "#/components/schemas/User"})),
            FieldType::Reference("User".to_string())
        );
    }

    #[test]
    fn generic_record_entity_structure() {
        let entity = generic_record_entity("test_api");
        assert_eq!(entity.name, "records");
        assert_eq!(entity.primary_key, "id");
        assert_eq!(entity.fields.len(), 2);
    }

    #[test]
    fn parse_swagger_2_definitions() {
        let spec = serde_json::json!({
            "definitions": {
                "Product": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"},
                        "price": {"type": "number"}
                    }
                }
            }
        });

        let entities = parse_openapi_schemas(&spec).expect("parse");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Product");
    }
}
