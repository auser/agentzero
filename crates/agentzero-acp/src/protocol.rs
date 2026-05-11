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
    /// Send a chat message (runs full agentic loop).
    Chat,
    /// Request tool execution.
    ToolCall,
    /// Get session info.
    SessionInfo,
    /// List available tools.
    ListTools,
    /// List available models from all configured providers.
    ListModels,
    /// Switch to a different model mid-session.
    SwitchModel,
    /// Respond to an approval request from the server.
    ApproveAction,
    /// Cancel a running chat request.
    Cancel,
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

/// A server-initiated notification (no `id` field — not a response to a request).
///
/// Used for streaming tokens, tool progress, and approval requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpNotification {
    /// Notification type: "token", "tool_start", "tool_result", "requires_approval", "context_compacted"
    pub notification: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl AcpNotification {
    pub fn token(text: &str) -> Self {
        Self {
            notification: "token".into(),
            params: serde_json::json!({ "text": text }),
        }
    }

    pub fn tool_start(tool_name: &str, args: &serde_json::Value) -> Self {
        Self {
            notification: "tool_start".into(),
            params: serde_json::json!({ "tool": tool_name, "arguments": args }),
        }
    }

    pub fn tool_result(tool_name: &str, success: bool, output_len: usize) -> Self {
        Self {
            notification: "tool_result".into(),
            params: serde_json::json!({
                "tool": tool_name,
                "success": success,
                "output_bytes": output_len
            }),
        }
    }

    pub fn requires_approval(request_id: &str, tool_name: &str, args: &serde_json::Value) -> Self {
        Self {
            notification: "requires_approval".into(),
            params: serde_json::json!({
                "request_id": request_id,
                "tool": tool_name,
                "arguments": args
            }),
        }
    }

    pub fn context_compacted(before: usize, after: usize) -> Self {
        Self {
            notification: "context_compacted".into(),
            params: serde_json::json!({ "before": before, "after": after }),
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

    #[test]
    fn new_methods_deserialize() {
        let json = r#"{"id":"3","method":"list_models","params":{}}"#;
        let req: AcpRequest = serde_json::from_str(json).expect("should deserialize");
        assert!(matches!(req.method, AcpMethod::ListModels));

        let json = r#"{"id":"4","method":"switch_model","params":{"model":"codellama"}}"#;
        let req: AcpRequest = serde_json::from_str(json).expect("should deserialize");
        assert!(matches!(req.method, AcpMethod::SwitchModel));

        let json = r#"{"id":"5","method":"approve_action","params":{"approved":true}}"#;
        let req: AcpRequest = serde_json::from_str(json).expect("should deserialize");
        assert!(matches!(req.method, AcpMethod::ApproveAction));

        let json = r#"{"id":"6","method":"cancel","params":{}}"#;
        let req: AcpRequest = serde_json::from_str(json).expect("should deserialize");
        assert!(matches!(req.method, AcpMethod::Cancel));
    }

    #[test]
    fn notification_serializes() {
        let n = AcpNotification::token("hello");
        let json = serde_json::to_string(&n).expect("should serialize");
        assert!(json.contains("\"notification\":\"token\""));
        assert!(json.contains("hello"));
        // Must NOT contain "id" field
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn notification_tool_start() {
        let n = AcpNotification::tool_start("read", &serde_json::json!({"path": "Cargo.toml"}));
        let json = serde_json::to_string(&n).expect("should serialize");
        assert!(json.contains("tool_start"));
        assert!(json.contains("Cargo.toml"));
    }

    #[test]
    fn notification_requires_approval() {
        let n = AcpNotification::requires_approval(
            "req-1",
            "shell",
            &serde_json::json!({"command": "rm -rf /"}),
        );
        let json = serde_json::to_string(&n).expect("should serialize");
        assert!(json.contains("requires_approval"));
        assert!(json.contains("req-1"));
    }
}
