//! Google Agent-to-Agent (A2A) protocol types.
//!
//! Implements the core data structures from the A2A specification:
//! Agent Cards, Tasks, Messages, Parts, and Artifacts.

use serde::{Deserialize, Serialize};

/// Agent Card — describes an agent's identity and capabilities.
/// Served at `GET /.well-known/agent.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub skills: Vec<AgentSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_input_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_output_modes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub push_notifications: bool,
    #[serde(default)]
    pub state_transition_history: bool,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            streaming: false,
            push_notifications: false,
            state_transition_history: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Task — the unit of work in A2A.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub history: Vec<Message>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

/// Task status with state and optional message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

/// Task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Submitted,
    Working,
    #[serde(rename = "input-required")]
    InputRequired,
    Completed,
    Canceled,
    Failed,
    Unknown,
}

/// A2A Message — a conversational turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Agent,
}

/// A content part within a message.
///
/// The primary tag field is `"type"` for backward compatibility, but
/// `"kind"` is also accepted during deserialization for forward compat
/// with newer A2A spec drafts. We achieve this by implementing a custom
/// deserializer that checks both `"kind"` and `"type"`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Part {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "data")]
    Data {
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}

impl<'de> serde::Deserialize<'de> for Part {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;

        // Accept "kind" as an alias for "type" during deserialization.
        if !map.contains_key("type") {
            if let Some(kind_val) = map.remove("kind") {
                map.insert("type".to_string(), kind_val);
            }
        }

        let value = serde_json::Value::Object(map);

        // Re-use a helper enum with the standard serde tag for actual parsing.
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum PartHelper {
            #[serde(rename = "text")]
            Text { text: String },
            #[serde(rename = "data")]
            Data {
                data: String,
                #[serde(default)]
                mime_type: Option<String>,
            },
        }

        let helper: PartHelper = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        match helper {
            PartHelper::Text { text } => Ok(Part::Text { text }),
            PartHelper::Data { data, mime_type } => Ok(Part::Data { data, mime_type }),
        }
    }
}

impl Part {
    pub fn text(s: impl Into<String>) -> Self {
        Part::Text { text: s.into() }
    }
}

/// An artifact produced by a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

// --- JSON-RPC request/response types for A2A ---

/// JSON-RPC 2.0 request for A2A.
#[derive(Debug, Clone, Deserialize)]
pub struct A2aRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// `tasks/send` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSendParams {
    pub id: String,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// `tasks/get` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGetParams {
    pub id: String,
    #[serde(default)]
    pub history_length: Option<usize>,
}

/// `tasks/cancel` parameters.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCancelParams {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_card_serializes_camel_case() {
        let card = AgentCard {
            name: "test-agent".to_string(),
            description: Some("A test agent".to_string()),
            url: "http://localhost:8080".to_string(),
            version: Some("1.0.0".to_string()),
            capabilities: AgentCapabilities::default(),
            skills: vec![AgentSkill {
                id: "chat".to_string(),
                name: "Chat".to_string(),
                description: Some("General conversation".to_string()),
                tags: vec!["general".to_string()],
            }],
            default_input_modes: None,
            default_output_modes: None,
        };
        let json = serde_json::to_string_pretty(&card).expect("serialize");
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"pushNotifications\""));
        assert!(json.contains("\"stateTransitionHistory\""));
    }

    #[test]
    fn task_state_serializes_correctly() {
        assert_eq!(
            serde_json::to_string(&TaskState::InputRequired).expect("serialize"),
            "\"input-required\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::Completed).expect("serialize"),
            "\"completed\""
        );
    }

    #[test]
    fn task_roundtrip() {
        let task = Task {
            id: "task-123".to_string(),
            status: TaskStatus {
                state: TaskState::Completed,
                message: Some(Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::text("Done!")],
                }),
            },
            history: vec![
                Message {
                    role: MessageRole::User,
                    parts: vec![Part::text("Hello")],
                },
                Message {
                    role: MessageRole::Agent,
                    parts: vec![Part::text("Hi there!")],
                },
            ],
            artifacts: vec![],
        };
        let json = serde_json::to_string(&task).expect("serialize");
        let parsed: Task = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, "task-123");
        assert_eq!(parsed.status.state, TaskState::Completed);
        assert_eq!(parsed.history.len(), 2);
    }

    #[test]
    fn part_text_helper() {
        let part = Part::text("hello");
        match part {
            Part::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn message_role_deserializes() {
        let user: MessageRole = serde_json::from_str("\"user\"").expect("deserialize");
        assert_eq!(user, MessageRole::User);
        let agent: MessageRole = serde_json::from_str("\"agent\"").expect("deserialize");
        assert_eq!(agent, MessageRole::Agent);
    }

    #[test]
    fn part_deserializes_with_kind_alias() {
        let json = serde_json::json!({"kind": "text", "text": "hello via kind"});
        let part: Part = serde_json::from_value(json).expect("should deserialize kind alias");
        match part {
            Part::Text { text } => assert_eq!(text, "hello via kind"),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn part_deserializes_with_type_tag() {
        let json = serde_json::json!({"type": "text", "text": "hello via type"});
        let part: Part = serde_json::from_value(json).expect("should deserialize type tag");
        match part {
            Part::Text { text } => assert_eq!(text, "hello via type"),
            _ => panic!("expected Text part"),
        }
    }

    #[test]
    fn task_send_params_deserializes() {
        let json = serde_json::json!({
            "id": "task-1",
            "message": {
                "role": "user",
                "parts": [{"type": "text", "text": "Hello agent"}]
            }
        });
        let params: TaskSendParams = serde_json::from_value(json).expect("deserialize");
        assert_eq!(params.id, "task-1");
        assert_eq!(params.message.role, MessageRole::User);
        assert_eq!(params.message.parts.len(), 1);
    }
}
