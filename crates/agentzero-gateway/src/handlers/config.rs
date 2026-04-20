use super::*;
use crate::models::{ConfigResponse, ConfigSection, ConfigUpdateRequest, ConfigUpdateResponse};

// ---------------------------------------------------------------------------
// Config endpoint: GET /v1/config
// ---------------------------------------------------------------------------

/// GET /v1/config — return current runtime configuration as structured sections.
pub(crate) async fn get_config(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ConfigResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let cfg = match state.live_config {
        Some(ref rx) => rx.borrow().clone(),
        None => {
            return Err(GatewayError::NotFound {
                resource: "config".to_string(),
            })
        }
    };

    // Serialize the config to a JSON Value, then split into sections by top-level key.
    let json_val =
        serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Object(Default::default()));
    let sections = if let serde_json::Value::Object(map) = json_val {
        map.into_iter()
            .map(|(key, value)| ConfigSection { key, value })
            .collect()
    } else {
        vec![]
    };

    Ok(Json(ConfigResponse { sections }))
}

/// PUT /v1/config — update configuration sections and write to agentzero.toml.
/// The config watcher will detect the file change and hot-reload automatically.
pub(crate) async fn update_config(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<ConfigUpdateRequest>,
) -> Result<Json<ConfigUpdateResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let config_path = state.config_path.as_ref().ok_or(GatewayError::NotFound {
        resource: "config file".to_string(),
    })?;

    // Read existing TOML content.
    let content = std::fs::read_to_string(config_path.as_ref()).unwrap_or_default();
    let mut doc: toml::Table = toml::from_str(&content).map_err(|e| GatewayError::BadRequest {
        message: format!("failed to parse existing config: {e}"),
    })?;

    // Merge each section from the request into the TOML table.
    let mut count = 0;
    for section in &req.sections {
        let toml_val =
            json_value_to_toml(&section.value).map_err(|e| GatewayError::BadRequest {
                message: format!("invalid value for section '{}': {e}", section.key),
            })?;
        doc.insert(section.key.clone(), toml_val);
        count += 1;
    }

    // Validate the merged config by deserializing it.
    let merged_str = toml::to_string_pretty(&doc).map_err(|e| GatewayError::BadRequest {
        message: format!("failed to serialize config: {e}"),
    })?;
    let merged_cfg: agentzero_config::AgentZeroConfig =
        toml::from_str(&merged_str).map_err(|e| GatewayError::BadRequest {
            message: format!("invalid config after merge: {e}"),
        })?;
    if let Err(e) = merged_cfg.validate() {
        return Err(GatewayError::BadRequest {
            message: format!("config validation failed: {e}"),
        });
    }

    // Write back to the config file.  The ConfigWatcher will detect the
    // mtime change and hot-reload automatically.
    std::fs::write(config_path.as_ref(), &merged_str).map_err(|e| {
        GatewayError::AgentExecutionFailed {
            message: format!("failed to write config file: {e}"),
        }
    })?;

    tracing::info!(sections = count, "config updated via API");

    Ok(Json(ConfigUpdateResponse {
        updated: true,
        sections_written: count,
    }))
}

/// Convert a serde_json::Value to a toml::Value, skipping null entries.
fn json_value_to_toml(v: &serde_json::Value) -> Result<toml::Value, String> {
    match v {
        serde_json::Value::Null => Err("null".to_string()),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(format!("unsupported number: {n}"))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<toml::Value>, String> = arr
                .iter()
                .filter(|v| !v.is_null())
                .map(json_value_to_toml)
                .collect();
            Ok(toml::Value::Array(items?))
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::Table::new();
            for (k, val) in map {
                if val.is_null() {
                    continue; // TOML has no null — omit the key entirely
                }
                table.insert(k.clone(), json_value_to_toml(val)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

// ---------------------------------------------------------------------------
// Memory endpoints: GET /v1/memory, POST /v1/memory/recall, POST /v1/memory/forget
// ---------------------------------------------------------------------------
// Cron endpoints: GET/POST /v1/cron, PATCH/DELETE /v1/cron/:id
