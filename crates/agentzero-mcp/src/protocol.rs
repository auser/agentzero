//! MCP JSON-RPC 2.0 protocol types.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("method not found: {0}")]
    MethodNotFound(String),
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("internal error: {0}")]
    InternalError(String),
}

impl McpError {
    pub fn code(&self) -> i64 {
        match self {
            Self::ParseError(_) => -32700,
            Self::MethodNotFound(_) => -32601,
            Self::InvalidParams(_) => -32602,
            Self::InternalError(_) => -32603,
        }
    }
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, err: &McpError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: err.code(),
                message: err.to_string(),
            }),
        }
    }
}

/// MCP tool definition for the tools/list response.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP tool call result for tools/call response.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl McpToolResult {
    pub fn text(output: &str) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".into(),
                text: output.to_string(),
            }],
            is_error: None,
        }
    }

    pub fn error(msg: &str) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".into(),
                text: msg.to_string(),
            }],
            is_error: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_response_serializes() {
        let resp = JsonRpcResponse::success(
            Some(serde_json::json!(1)),
            serde_json::json!({"status": "ok"}),
        );
        let json = serde_json::to_string(&resp).expect("should serialize");
        assert!(json.contains("2.0"));
        assert!(json.contains("ok"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn error_response_serializes() {
        let err = McpError::MethodNotFound("unknown".into());
        let resp = JsonRpcResponse::error(Some(serde_json::json!(2)), &err);
        let json = serde_json::to_string(&resp).expect("should serialize");
        assert!(json.contains("-32601"));
        assert!(json.contains("unknown"));
    }

    #[test]
    fn tool_result_text() {
        let result = McpToolResult::text("hello world");
        let json = serde_json::to_string(&result).expect("should serialize");
        assert!(json.contains("hello world"));
        assert!(json.contains("text"));
    }

    #[test]
    fn tool_result_error() {
        let result = McpToolResult::error("something failed");
        let json = serde_json::to_string(&result).expect("should serialize");
        assert!(json.contains("something failed"));
        assert!(json.contains("isError"));
    }

    #[test]
    fn request_deserializes() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).expect("should parse");
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.id, Some(serde_json::json!(1)));
    }
}
