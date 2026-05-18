//! Hibernation checkpoint for AgentZero sessions.
//!
//! Captures the full agent state — messages, approvals, config, dynamic
//! tools — so a session can survive process exit and resume later.
//! Used by both the CLI idle-timeout path and gateway wake triggers.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ollama::ChatMessage;

/// Current checkpoint format version. Increment when fields change.
const CHECKPOINT_VERSION: u32 = 1;

/// Serializable snapshot of a running `AgentLoop`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCheckpoint {
    /// Format version for forward compatibility.
    pub version: u32,
    /// Session identifier.
    pub session_id: String,
    /// Model name the session was using.
    pub model: String,
    /// Full conversation history.
    pub messages: Vec<ChatMessage>,
    /// Tool names approved for the session scope.
    pub session_approvals: HashSet<String>,
    /// Serializable subset of loop configuration.
    pub config: CheckpointConfig,
    /// Names of dynamic tools registered for this project.
    /// WASM bytes stay on disk in `.agentzero/skills/<name>/`.
    pub dynamic_tools: Vec<String>,
    /// ISO 8601 timestamp when the checkpoint was created.
    pub created_at: String,
    /// Why the agent hibernated (e.g. "idle timeout", "explicit request").
    pub idle_reason: Option<String>,
    /// Conditions that should wake this session.
    pub wake_triggers: Vec<WakeTrigger>,
}

/// Serializable loop configuration (mirrors `AgentLoopConfig` fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    pub max_tool_rounds: usize,
    pub max_output_bytes: usize,
    pub max_tools_in_context: usize,
}

/// Conditions that can resume a hibernated session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WakeTrigger {
    /// Resume via `az chat --resume <id>`.
    CliResume,
    /// Resume when a message arrives on a messaging gateway.
    GatewayMessage { gateway: String },
    /// Resume at a specific time (ISO 8601).
    Timer { at: String },
}

/// Parameters for creating a checkpoint. Avoids a long argument list.
pub struct CaptureParams<'a> {
    pub session_id: &'a str,
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub session_approvals: &'a HashSet<String>,
    pub config: CheckpointConfig,
    pub dynamic_tools: Vec<String>,
    pub idle_reason: Option<String>,
    pub wake_triggers: Vec<WakeTrigger>,
}

impl AgentCheckpoint {
    /// Create a checkpoint from the current agent state.
    pub fn capture(params: CaptureParams<'_>) -> Self {
        Self {
            version: CHECKPOINT_VERSION,
            session_id: params.session_id.to_string(),
            model: params.model.to_string(),
            messages: params.messages.to_vec(),
            session_approvals: params.session_approvals.clone(),
            config: params.config,
            dynamic_tools: params.dynamic_tools,
            created_at: chrono::Utc::now().to_rfc3339(),
            idle_reason: params.idle_reason,
            wake_triggers: params.wake_triggers,
        }
    }

    /// Save the checkpoint to a JSON file.
    pub fn save(&self, path: &Path) -> Result<(), CheckpointError> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| CheckpointError::Serialize(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| CheckpointError::Io(e.to_string()))
    }

    /// Load a checkpoint from a JSON file.
    pub fn load(path: &Path) -> Result<Self, CheckpointError> {
        let content =
            std::fs::read_to_string(path).map_err(|e| CheckpointError::Io(e.to_string()))?;
        let checkpoint: Self = serde_json::from_str(&content)
            .map_err(|e| CheckpointError::Deserialize(e.to_string()))?;
        Ok(checkpoint)
    }

    /// Path for a checkpoint file given a session ID and base directory.
    pub fn path_for(sessions_dir: &Path, session_id: &str) -> std::path::PathBuf {
        sessions_dir.join(format!("{session_id}.checkpoint.json"))
    }
}

/// Errors that can occur during checkpoint operations.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint IO error: {0}")]
    Io(String),
    #[error("checkpoint serialization error: {0}")]
    Serialize(String),
    #[error("checkpoint deserialization error: {0}")]
    Deserialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_checkpoint() {
        let checkpoint = AgentCheckpoint::capture(CaptureParams {
            session_id: "test-session-123",
            model: "llama3.2",
            messages: &[
                ChatMessage::system("You are a helpful assistant."),
                ChatMessage::user("Hello!"),
            ],
            session_approvals: &HashSet::from(["run_command".to_string()]),
            config: CheckpointConfig {
                max_tool_rounds: 5,
                max_output_bytes: 8000,
                max_tools_in_context: 20,
            },
            dynamic_tools: vec!["my_tool".to_string()],
            idle_reason: Some("idle timeout".to_string()),
            wake_triggers: vec![WakeTrigger::CliResume],
        });

        let json = serde_json::to_string_pretty(&checkpoint).expect("should serialize");
        let loaded: AgentCheckpoint = serde_json::from_str(&json).expect("should deserialize");

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.session_id, "test-session-123");
        assert_eq!(loaded.model, "llama3.2");
        assert_eq!(loaded.messages.len(), 2);
        assert!(loaded.session_approvals.contains("run_command"));
        assert_eq!(loaded.dynamic_tools, vec!["my_tool"]);
        assert_eq!(loaded.idle_reason, Some("idle timeout".to_string()));
        assert_eq!(loaded.wake_triggers.len(), 1);
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join("agentzero_checkpoint_test");
        std::fs::create_dir_all(&dir).expect("create dir");

        let checkpoint = AgentCheckpoint::capture(CaptureParams {
            session_id: "file-test",
            model: "llama3.2",
            messages: &[ChatMessage::user("test")],
            session_approvals: &HashSet::new(),
            config: CheckpointConfig {
                max_tool_rounds: 5,
                max_output_bytes: 8000,
                max_tools_in_context: 20,
            },
            dynamic_tools: vec![],
            idle_reason: None,
            wake_triggers: vec![],
        });

        let path = AgentCheckpoint::path_for(&dir, "file-test");
        checkpoint.save(&path).expect("should save");
        let loaded = AgentCheckpoint::load(&path).expect("should load");

        assert_eq!(loaded.session_id, "file-test");
        assert_eq!(loaded.messages.len(), 1);

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn checkpoint_path_format() {
        let path = AgentCheckpoint::path_for(Path::new("/tmp/sessions"), "abc-123");
        assert_eq!(
            path.to_str().expect("valid path"),
            "/tmp/sessions/abc-123.checkpoint.json"
        );
    }

    #[test]
    fn wake_trigger_serialization() {
        let triggers = vec![
            WakeTrigger::CliResume,
            WakeTrigger::GatewayMessage {
                gateway: "slack".to_string(),
            },
            WakeTrigger::Timer {
                at: "2026-05-20T09:00:00Z".to_string(),
            },
        ];
        let json = serde_json::to_string(&triggers).expect("should serialize");
        let loaded: Vec<WakeTrigger> = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(loaded.len(), 3);
    }
}
