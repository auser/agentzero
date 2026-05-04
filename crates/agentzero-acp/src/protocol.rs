//! ACP wire protocol types.

use serde::{Deserialize, Serialize};

/// An ACP request from an editor/client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRequest {
    pub id: String,
    pub method: AcpMethod,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// ACP methods supported by AgentZero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpMethod {
    /// Initialize the connection.
    Initialize,
    /// Send a chat message.
    Chat,
    /// Request tool execution.
    ToolCall,
    /// Get session info.
    SessionInfo,
    /// List available tools.
    ListTools,
    /// Shutdown the connection.
    Shutdown,
}

/// An ACP response to the editor/client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpResponse {
    pub id: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AcpResponse {
    pub fn ok(id: &str, result: serde_json::Value) -> Self {
        Self {
            id: id.to_string(),
            success: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: &str, error: &str) -> Self {
        Self {
            id: id.to_string(),
            success: false,
            result: None,
            error: Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes() {
        let req = AcpRequest {
            id: "1".into(),
            method: AcpMethod::Initialize,
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).expect("should serialize");
        assert!(json.contains("initialize"));
    }

    #[test]
    fn response_ok_serializes() {
        let resp = AcpResponse::ok("1", serde_json::json!({"status": "ready"}));
        let json = serde_json::to_string(&resp).expect("should serialize");
        assert!(json.contains("ready"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn response_err_serializes() {
        let resp = AcpResponse::err("1", "something failed");
        let json = serde_json::to_string(&resp).expect("should serialize");
        assert!(json.contains("something failed"));
        assert!(!json.contains("result"));
    }

    #[test]
    fn request_deserializes() {
        let json = r#"{"id":"2","method":"chat","params":{"message":"hello"}}"#;
        let req: AcpRequest = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(req.id, "2");
        assert!(matches!(req.method, AcpMethod::Chat));
    }
}
