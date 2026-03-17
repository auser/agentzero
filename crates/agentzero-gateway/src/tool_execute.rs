//! Remote tool execution endpoint for agentzero-lite delegation.
//!
//! `POST /v1/tool-execute` accepts a tool name and input, executes it on the
//! full-featured node, and returns the result. Currently a stub that will be
//! wired to the runtime tool registry once integrated.

use crate::state::GatewayState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Known tools that the stub recognises. When the runtime is wired up this
/// list will be replaced by a dynamic lookup against the tool registry.
const KNOWN_TOOL_PREFIXES: &[&str] = &[
    "read_file",
    "write_file",
    "search",
    "shell",
    "http",
    "memory",
    "delegate",
];

#[derive(Debug, Deserialize)]
pub(crate) struct ToolExecuteRequest {
    pub(crate) tool: String,
    /// Tool input payload. Currently unused in the stub handler but will be
    /// forwarded to the actual tool implementation once the runtime is wired.
    #[allow(dead_code)]
    pub(crate) input: Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolExecuteResponse {
    pub(crate) tool: String,
    pub(crate) output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// `POST /v1/tool-execute` — execute a tool by name.
///
/// Currently returns a stub response. The actual tool dispatch will be
/// connected when the runtime is integrated into the gateway state.
pub(crate) async fn handle_tool_execute(
    State(_state): State<GatewayState>,
    Json(req): Json<ToolExecuteRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    if req.tool.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "type": "bad_request",
                    "message": "tool name must not be empty"
                }
            })),
        ));
    }

    // Check whether the tool name matches any known prefix. This is a
    // placeholder — once the runtime is wired, we will look up the actual
    // tool registry.
    let is_known = KNOWN_TOOL_PREFIXES
        .iter()
        .any(|prefix| req.tool.starts_with(prefix));

    if !is_known {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "type": "not_found",
                    "message": format!("unknown tool: {}", req.tool)
                }
            })),
        ));
    }

    Ok(Json(ToolExecuteResponse {
        tool: req.tool,
        output: "remote tool execution not yet wired".to_string(),
        error: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::MiddlewareConfig;
    use crate::router::build_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    fn test_app() -> axum::Router {
        let state = GatewayState::test_with_bearer(None);
        // Disable pairing requirement so we don't need auth headers.
        let state = state.with_gateway_config(false, false);
        build_router(state, &MiddlewareConfig::default())
    }

    #[tokio::test]
    async fn tool_execute_valid_tool_returns_200() {
        let app = test_app();
        let body = json!({ "tool": "read_file", "input": { "path": "/tmp/test.txt" } });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/tool-execute")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).expect("serialize")))
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body should be json");
        assert_eq!(json["output"], "remote tool execution not yet wired");
        assert!(json["error"].is_null());
    }

    #[tokio::test]
    async fn tool_execute_empty_body_returns_error() {
        let app = test_app();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/tool-execute")
            .header("content-type", "application/json")
            .body(Body::empty())
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        // Empty body cannot be deserialized — axum returns 422 (Unprocessable Entity)
        // for JSON deserialization failures, or 400 for missing content-type.
        assert!(
            response.status() == StatusCode::UNPROCESSABLE_ENTITY
                || response.status() == StatusCode::BAD_REQUEST,
            "expected 400 or 422, got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn tool_execute_response_includes_tool_name() {
        let app = test_app();
        let body = json!({ "tool": "shell", "input": { "command": "echo hello" } });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/tool-execute")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).expect("serialize")))
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body should be json");
        assert_eq!(json["tool"], "shell");
    }

    #[tokio::test]
    async fn tool_execute_unknown_tool_returns_404() {
        let app = test_app();
        let body = json!({ "tool": "nonexistent_tool", "input": {} });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/tool-execute")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).expect("serialize")))
            .expect("request should build");

        let response = app
            .oneshot(request)
            .await
            .expect("response should be returned");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("body should be json");
        assert!(json["error"]["message"]
            .as_str()
            .expect("message should be string")
            .contains("nonexistent_tool"));
    }
}
