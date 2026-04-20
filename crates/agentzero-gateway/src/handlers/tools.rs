use super::*;
use crate::models::{ToolSummary, ToolsResponse};

/// `GET /v1/openapi.json` — serves the auto-generated OpenAPI 3.1 specification.
pub(crate) async fn openapi_spec() -> Json<serde_json::Value> {
    Json(crate::openapi::build_openapi_spec())
}

// ---------------------------------------------------------------------------
// Webhook auto-registration helpers
// ---------------------------------------------------------------------------

/// Resolve the gateway's public URL from live config or environment variable.
pub(crate) fn resolve_public_url(state: &GatewayState) -> Option<String> {
    // Try live config first.
    if let Some(ref rx) = state.live_config {
        let url = rx.borrow().gateway.public_url.clone();
        if url.is_some() {
            return url;
        }
    }
    // Fall back to environment variable.
    std::env::var("AGENTZERO_PUBLIC_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Tools endpoint: GET /v1/tools
// ---------------------------------------------------------------------------

/// GET /v1/tools — list all available tools with metadata and JSON schema.
pub(crate) async fn get_tools(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<ToolsResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    // Build tool policy from user config — warn on failure instead of silent degradation
    let policy = if let Some(ref config_path) = state.config_path {
        let ws_root = config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        agentzero_config::load_tool_security_policy(&ws_root, config_path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load tool security policy from config, using defaults");
            agentzero_infra::tools::ToolSecurityPolicy::default_for_workspace(ws_root)
        })
    } else {
        tracing::debug!("No config path set, using default tool security policy");
        agentzero_infra::tools::ToolSecurityPolicy::default_for_workspace(std::env::temp_dir())
    };
    let tools = agentzero_infra::tools::default_tools(&policy, None, None).unwrap_or_default();

    let summaries: Vec<ToolSummary> = tools
        .iter()
        .map(|t| ToolSummary {
            name: t.name().to_string(),
            description: t.description().to_string(),
            category: infer_tool_category(t.name()),
            input_schema: t.input_schema(),
        })
        .collect();

    let total = summaries.len();
    Ok(Json(ToolsResponse {
        object: "list",
        tools: summaries,
        total,
    }))
}

/// Infer the tool category from the tool name for UI grouping.
fn infer_tool_category(name: &str) -> String {
    let cat = if name.starts_with("read_file")
        || name.starts_with("write_file")
        || name.starts_with("glob_search")
        || name.starts_with("content_search")
        || name.starts_with("apply_patch")
        || name.starts_with("pdf_read")
        || name.starts_with("docx_read")
        || name == "file_edit"
    {
        "file"
    } else if name.starts_with("web_fetch")
        || name.starts_with("web_search")
        || name.starts_with("http_request")
        || name.starts_with("url_validation")
    {
        "web"
    } else if name.starts_with("shell")
        || name.starts_with("process")
        || name.starts_with("git_")
        || name == "code_interpreter"
    {
        "execution"
    } else if name.starts_with("memory_") {
        "memory"
    } else if name.starts_with("schedule") || name.starts_with("cron_") {
        "scheduling"
    } else if name.starts_with("delegate")
        || name.starts_with("sub_agent")
        || name.starts_with("task_plan")
        || name.starts_with("agent_")
    {
        "delegation"
    } else if name.starts_with("image_")
        || name.starts_with("screenshot")
        || name.starts_with("tts")
        || name.starts_with("video_")
    {
        "media"
    } else if name.starts_with("hardware_") {
        "hardware"
    } else {
        "other"
    };
    cat.to_string()
}

// ---------------------------------------------------------------------------
// Dynamic tool sharing endpoints
// ---------------------------------------------------------------------------

/// GET /v1/dynamic-tools — list dynamic tools with quality metadata.
pub(crate) async fn list_dynamic_tools(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let registry = state
        .dynamic_tool_registry
        .as_ref()
        .ok_or(GatewayError::BadRequest {
            message: "dynamic tools not enabled".to_string(),
        })?;

    let defs = registry.list().await;
    let tools: Vec<serde_json::Value> = defs
        .iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "description": d.description,
                "strategy_type": match &d.strategy {
                    agentzero_infra::tools::dynamic_tool::DynamicToolStrategy::Shell { .. } => "shell",
                    agentzero_infra::tools::dynamic_tool::DynamicToolStrategy::Http { .. } => "http",
                    agentzero_infra::tools::dynamic_tool::DynamicToolStrategy::Llm { .. } => "llm",
                    agentzero_infra::tools::dynamic_tool::DynamicToolStrategy::Composite { .. } => "composite",
                    agentzero_infra::tools::dynamic_tool::DynamicToolStrategy::Codegen { .. } => "codegen",
                },
                "created_at": d.created_at,
                "total_invocations": d.total_invocations,
                "total_successes": d.total_successes,
                "total_failures": d.total_failures,
                "success_rate": d.success_rate(),
                "generation": d.generation,
                "parent_name": d.parent_name,
                "user_rated": d.user_rated,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "object": "list",
        "tools": tools,
        "total": tools.len(),
    })))
}

/// GET /v1/dynamic-tools/:name/bundle — export a tool as a shareable bundle.
pub(crate) async fn export_dynamic_tool_bundle(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsRead)?;

    let registry = state
        .dynamic_tool_registry
        .as_ref()
        .ok_or(GatewayError::BadRequest {
            message: "dynamic tools not enabled".to_string(),
        })?;

    // Export without recipe store reference to avoid MutexGuard across await.
    let bundle = registry
        .export_bundle(&name, None)
        .await
        .map_err(|e| GatewayError::AgentExecutionFailed {
            message: format!("failed to export bundle: {e}"),
        })?
        .ok_or(GatewayError::NotFound {
            resource: format!("dynamic tool '{name}'"),
        })?;

    let json = serde_json::to_value(&bundle).map_err(|e| GatewayError::AgentExecutionFailed {
        message: format!("failed to serialize bundle: {e}"),
    })?;

    Ok(Json(json))
}

/// POST /v1/dynamic-tools/import — import a tool bundle.
pub(crate) async fn import_dynamic_tool_bundle(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(bundle): AppJson<agentzero_infra::tools::dynamic_tool::ToolBundle>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    let registry = state
        .dynamic_tool_registry
        .as_ref()
        .ok_or(GatewayError::BadRequest {
            message: "dynamic tools not enabled".to_string(),
        })?;

    let name = registry.import_bundle(bundle, None).await.map_err(|e| {
        GatewayError::AgentExecutionFailed {
            message: format!("failed to import bundle: {e}"),
        }
    })?;

    Ok(Json(serde_json::json!({
        "imported": name,
        "message": format!("Tool '{}' imported successfully (quality counters reset).", name),
    })))
}
