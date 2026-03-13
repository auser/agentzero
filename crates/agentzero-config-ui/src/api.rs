use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::schema::{
    build_node_type_descriptors, build_tool_summaries, NodeTypeDescriptor, ToolSummary,
};
use crate::toml_bridge::{
    config_to_graph, graph_to_toml, toml_to_graph, validate_graph, GraphModel, ValidationError,
};

/// GET /api/schema — returns all node type descriptors.
pub async fn get_schema() -> Json<Vec<NodeTypeDescriptor>> {
    Json(build_node_type_descriptors())
}

/// GET /api/tools — returns all registered tool summaries.
pub async fn get_tools() -> Json<Vec<ToolSummary>> {
    Json(build_tool_summaries())
}

/// GET /api/defaults — returns a default graph model.
pub async fn get_defaults() -> Json<GraphModel> {
    let config = agentzero_config::AgentZeroConfig::default();
    Json(config_to_graph(&config))
}

#[derive(Deserialize)]
pub struct ImportRequest {
    pub toml: String,
}

#[derive(Serialize)]
pub struct ImportResponse {
    pub graph: GraphModel,
}

/// POST /api/import — parse TOML string into a graph model.
pub async fn import_toml(
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportResponse>, (StatusCode, String)> {
    match toml_to_graph(&req.toml) {
        Ok(graph) => Ok(Json(ImportResponse { graph })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

#[derive(Serialize)]
pub struct ExportResponse {
    pub toml: String,
}

/// POST /api/export — convert a graph model to TOML.
pub async fn export_toml(
    Json(graph): Json<GraphModel>,
) -> Result<Json<ExportResponse>, (StatusCode, String)> {
    match graph_to_toml(&graph) {
        Ok(toml) => Ok(Json(ExportResponse { toml })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

#[derive(Serialize)]
pub struct ValidateResponse {
    pub errors: Vec<ValidationError>,
    pub valid: bool,
}

/// POST /api/validate — validate a graph model.
pub async fn validate(Json(graph): Json<GraphModel>) -> impl IntoResponse {
    let errors = validate_graph(&graph);
    let valid = errors
        .iter()
        .all(|e| matches!(e.severity, crate::toml_bridge::Severity::Warning));
    Json(ValidateResponse { errors, valid })
}
