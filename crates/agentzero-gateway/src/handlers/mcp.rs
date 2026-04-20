use super::*;

pub(crate) async fn tool_execute(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(body): AppJson<crate::models::ToolExecuteRequest>,
) -> Result<Json<crate::models::ToolExecuteResponse>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let server = state.mcp_server.as_ref().ok_or(GatewayError::BadRequest {
        message: "tool execution not available — no tools loaded (check config)".to_string(),
    })?;

    let input = if body.input.is_object() || body.input.is_array() {
        serde_json::to_string(&body.input).unwrap_or_default()
    } else {
        body.input.as_str().unwrap_or("").to_string()
    };

    match server.execute_tool(&body.tool, &input).await {
        Ok(result) => Ok(Json(crate::models::ToolExecuteResponse {
            tool: body.tool,
            output: result.output,
            error: None,
        })),
        Err(e) => Ok(Json(crate::models::ToolExecuteResponse {
            tool: body.tool,
            output: String::new(),
            error: Some(e.to_string()),
        })),
    }
}

/// `POST /mcp/message` — Handle MCP JSON-RPC messages over HTTP.
///
/// Accepts a JSON-RPC 2.0 request, processes it via the MCP server,
/// and returns the JSON-RPC response. Used by MCP clients that prefer
/// HTTP transport over stdio.
pub(crate) async fn mcp_message(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(body): AppJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    authorize_request(&state, &headers, false)?;

    let server = state.mcp_server.as_ref().ok_or(GatewayError::BadRequest {
        message: "MCP server not available — no tools loaded (check config)".to_string(),
    })?;

    match server.handle_message(&body).await {
        Some(response) => Ok(Json(response)),
        None => {
            // Notification — no response body needed, return empty JSON object.
            Ok(Json(serde_json::json!({})))
        }
    }
}

// ---------------------------------------------------------------------------
// Workflow CRUD + Execution: /v1/workflows
