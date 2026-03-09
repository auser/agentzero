use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) service: String,
    pub(crate) version: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReadyResponse {
    pub(crate) ready: bool,
    pub(crate) service: String,
    pub(crate) version: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) checks_failed: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PingRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PingResponse {
    pub ok: bool,
    pub echo: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebhookResponse {
    pub(crate) accepted: bool,
    pub(crate) channel: String,
    pub(crate) detail: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PairRequest;

#[derive(Debug, Serialize)]
pub(crate) struct PairResponse {
    pub(crate) paired: bool,
    pub(crate) token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatRequest {
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) context: Vec<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatResponse {
    pub(crate) message: String,
    pub(crate) tokens_used_estimate: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelsResponse {
    pub(crate) object: &'static str,
    pub(crate) data: Vec<ModelItem>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelItem {
    pub(crate) id: String,
    pub(crate) object: &'static str,
    pub(crate) owned_by: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionsRequest {
    pub(crate) model: Option<String>,
    pub(crate) messages: Vec<CompletionMessage>,
    #[serde(default)]
    pub(crate) stream: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CompletionMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChatCompletionsResponse {
    pub(crate) id: String,
    pub(crate) object: &'static str,
    pub(crate) choices: Vec<CompletionChoice>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompletionChoice {
    pub(crate) index: usize,
    pub(crate) message: CompletionChoiceMessage,
    pub(crate) finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompletionChoiceMessage {
    pub(crate) role: &'static str,
    pub(crate) content: String,
}

/// A single transcript entry for sub-agent conversation retrieval.
#[derive(Debug, Serialize)]
pub(crate) struct TranscriptEntry {
    pub(crate) role: String,
    pub(crate) content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) created_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Structured error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum GatewayError {
    AuthRequired,
    AuthFailed,
    NotFound { resource: String },
    AgentUnavailable,
    AgentExecutionFailed { message: String },
    RateLimited,
    PayloadTooLarge,
    BadRequest { message: String },
}

impl GatewayError {
    fn error_type(&self) -> &'static str {
        match self {
            Self::AuthRequired => "auth_required",
            Self::AuthFailed => "auth_failed",
            Self::NotFound { .. } => "not_found",
            Self::AgentUnavailable => "agent_unavailable",
            Self::AgentExecutionFailed { .. } => "agent_execution_failed",
            Self::RateLimited => "rate_limited",
            Self::PayloadTooLarge => "payload_too_large",
            Self::BadRequest { .. } => "bad_request",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::AuthRequired => StatusCode::UNAUTHORIZED,
            Self::AuthFailed => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::AgentUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::AgentExecutionFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::AuthRequired => "authentication required".to_string(),
            Self::AuthFailed => "authentication failed".to_string(),
            Self::NotFound { resource } => format!("not found: {resource}"),
            Self::AgentUnavailable => "agent runtime not configured".to_string(),
            Self::AgentExecutionFailed { message } => format!("agent execution failed: {message}"),
            Self::RateLimited => "rate limit exceeded".to_string(),
            Self::PayloadTooLarge => "request body too large".to_string(),
            Self::BadRequest { message } => message.clone(),
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        crate::gateway_metrics::record_error(self.error_type());
        let status = self.status_code();
        let body = json!({
            "error": {
                "type": self.error_type(),
                "message": self.message(),
            }
        });
        (status, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Async job submission (OpenClaw-style /v1/runs)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct AsyncSubmitRequest {
    pub(crate) message: String,
    /// Lane override: "main" (default), "cron", or "subagent".
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) lane: Option<String>,
    /// Queue mode: "steer" (default), "followup", "collect", "interrupt".
    #[serde(default)]
    pub(crate) mode: Option<String>,
    /// Run ID for followup mode — appends to an existing run's conversation.
    #[serde(default)]
    pub(crate) run_id: Option<String>,
    /// Model override.
    #[serde(default)]
    pub(crate) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CancelQuery {
    #[serde(default)]
    pub(crate) cascade: Option<bool>,
}

/// Query params for WebSocket run subscription.
#[derive(Debug, Deserialize)]
pub(crate) struct WsRunQuery {
    /// Output format: "raw" (default) or "blocks" (markdown-aware chunking).
    #[serde(default)]
    pub(crate) format: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AsyncSubmitResponse {
    pub(crate) run_id: String,
    pub(crate) accepted_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct JobStatusResponse {
    pub(crate) run_id: String,
    pub(crate) status: String,
    pub(crate) agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JobListQuery {
    pub(crate) status: Option<String>,
}

impl From<StatusCode> for GatewayError {
    fn from(status: StatusCode) -> Self {
        match status {
            StatusCode::UNAUTHORIZED => Self::AuthRequired,
            StatusCode::FORBIDDEN => Self::AuthFailed,
            StatusCode::SERVICE_UNAVAILABLE => Self::AgentUnavailable,
            StatusCode::TOO_MANY_REQUESTS => Self::RateLimited,
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            _ => Self::BadRequest {
                message: status.to_string(),
            },
        }
    }
}
