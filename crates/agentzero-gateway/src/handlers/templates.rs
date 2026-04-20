use super::*;

/// POST /v1/templates — create a new reusable workflow template.
pub(crate) async fn create_template(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_template_store()?;

    let name = req["name"].as_str().unwrap_or("Untitled").to_string();
    let description = req["description"].as_str().unwrap_or("").to_string();
    let category = req["category"].as_str().unwrap_or("custom").to_string();
    let tags: Vec<String> = req["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let layout = &req["layout"];
    let nodes = layout["nodes"]
        .as_array()
        .or_else(|| req["nodes"].as_array())
        .cloned()
        .unwrap_or_default();
    let edges = layout["edges"]
        .as_array()
        .or_else(|| req["edges"].as_array())
        .cloned()
        .unwrap_or_default();

    let record = agentzero_orchestrator::TemplateRecord {
        template_id: String::new(),
        name,
        description,
        category,
        tags,
        version: 0,
        nodes,
        edges,
        created_at: 0,
        updated_at: 0,
    };

    let created = store.create(record).map_err(|e| GatewayError::BadRequest {
        message: e.to_string(),
    })?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({
            "template_id": created.template_id,
            "name": created.name,
            "description": created.description,
            "category": created.category,
            "tags": created.tags,
            "version": created.version,
            "node_count": created.nodes.len(),
            "edge_count": created.edges.len(),
            "created_at": created.created_at,
            "updated_at": created.updated_at,
        })),
    ))
}

/// GET /v1/templates — list all templates.
pub(crate) async fn list_templates(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state.require_template_store()?;

    let include_layout = params.get("include").is_some_and(|v| v == "layout");

    let templates: Vec<Value> = store
        .list()
        .into_iter()
        .map(|t| {
            let mut entry = json!({
                "template_id": t.template_id,
                "name": t.name,
                "description": t.description,
                "category": t.category,
                "tags": t.tags,
                "version": t.version,
                "node_count": t.nodes.len(),
                "edge_count": t.edges.len(),
                "created_at": t.created_at,
                "updated_at": t.updated_at,
            });
            if include_layout {
                entry["layout"] = json!({
                    "nodes": t.nodes,
                    "edges": t.edges,
                });
            }
            entry
        })
        .collect();

    let total = templates.len();
    Ok(Json(json!({
        "object": "list",
        "data": templates,
        "total": total,
    })))
}

/// GET /v1/templates/:id — get a single template.
pub(crate) async fn get_template(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let store = state.require_template_store()?;

    let template = store.get(&id).ok_or(GatewayError::NotFound {
        resource: format!("template/{id}"),
    })?;

    let include_layout = params.get("include").is_some_and(|v| v == "layout");

    let mut entry = json!({
        "template_id": template.template_id,
        "name": template.name,
        "description": template.description,
        "category": template.category,
        "tags": template.tags,
        "version": template.version,
        "node_count": template.nodes.len(),
        "edge_count": template.edges.len(),
        "created_at": template.created_at,
        "updated_at": template.updated_at,
    });
    if include_layout {
        entry["layout"] = json!({
            "nodes": template.nodes,
            "edges": template.edges,
        });
    }
    Ok(Json(entry))
}

/// PATCH /v1/templates/:id — update an existing template.
pub(crate) async fn update_template(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    AppJson(req): AppJson<Value>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_template_store()?;

    let layout = &req["layout"];
    let update = agentzero_orchestrator::TemplateUpdate {
        name: req["name"].as_str().map(String::from),
        description: req["description"].as_str().map(String::from),
        category: req["category"].as_str().map(String::from),
        tags: req["tags"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        }),
        nodes: layout["nodes"]
            .as_array()
            .or_else(|| req["nodes"].as_array())
            .cloned(),
        edges: layout["edges"]
            .as_array()
            .or_else(|| req["edges"].as_array())
            .cloned(),
    };

    let updated = store
        .update(&id, update)
        .map_err(|e| GatewayError::BadRequest {
            message: e.to_string(),
        })?
        .ok_or(GatewayError::NotFound {
            resource: format!("template/{id}"),
        })?;

    Ok(Json(json!({
        "template_id": updated.template_id,
        "name": updated.name,
        "description": updated.description,
        "category": updated.category,
        "tags": updated.tags,
        "version": updated.version,
        "node_count": updated.nodes.len(),
        "edge_count": updated.edges.len(),
        "created_at": updated.created_at,
        "updated_at": updated.updated_at,
    })))
}

/// DELETE /v1/templates/:id — delete a template.
pub(crate) async fn delete_template(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::Admin)?;

    let store = state.require_template_store()?;

    let removed = store.delete(&id).map_err(|e| GatewayError::BadRequest {
        message: e.to_string(),
    })?;

    if !removed {
        return Err(GatewayError::NotFound {
            resource: format!("template/{id}"),
        });
    }

    Ok(Json(json!({ "deleted": true, "template_id": id })))
}

/// Extract fallback headers from the task-local, if a provider fallback occurred.
///
/// Returns a list of `(header-name, value)` pairs that should be added to the
/// HTTP response so API consumers can detect when a fallback provider served
/// the request.
pub(crate) fn fallback_response_headers() -> Vec<(String, String)> {
    agentzero_providers::FALLBACK_INFO
        .try_with(|cell| {
            cell.borrow().as_ref().map(|fi| {
                vec![
                    ("X-Provider-Fallback".to_string(), "true".to_string()),
                    ("X-Provider-Used".to_string(), fi.actual_provider.clone()),
                ]
            })
        })
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GatewayState;

    #[test]
    fn build_agent_request_uses_capability_override() {
        use agentzero_core::security::capability::{Capability, CapabilitySet};
        use std::path::PathBuf;

        let mut state = GatewayState::test_with_bearer(None);
        state.config_path = Some(std::sync::Arc::new(PathBuf::from("/tmp/test.toml")));
        state.workspace_root = Some(std::sync::Arc::new(PathBuf::from("/tmp")));

        let ceiling = CapabilitySet::new(
            vec![Capability::Tool {
                name: "web_search".to_string(),
            }],
            vec![],
        );
        let req = build_agent_request(&state, "hello".to_string(), None, ceiling.clone())
            .expect("should build request");
        assert!(
            !req.capability_set_override.is_empty(),
            "capability_set_override should be set from the override parameter"
        );
        assert!(req.capability_set_override.allows_tool("web_search"));
        assert!(!req.capability_set_override.allows_tool("shell"));
    }
}
