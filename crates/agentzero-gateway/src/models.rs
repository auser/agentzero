use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) service: String,
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
    pub(crate) id: &'static str,
    pub(crate) object: &'static str,
    pub(crate) owned_by: &'static str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionsRequest {
    pub(crate) model: Option<String>,
    pub(crate) messages: Vec<CompletionMessage>,
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
