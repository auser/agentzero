use crate::handlers::{
    api_chat, api_fallback, dashboard, health, legacy_webhook, metrics, pair, ping,
    v1_chat_completions, v1_models, webhook, ws_chat,
};
use crate::state::GatewayState;
use axum::{
    routing::{get, post},
    Router,
};

pub(crate) fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/pair", post(pair))
        .route("/webhook", post(legacy_webhook))
        .route("/v1/ping", post(ping))
        .route("/v1/webhook/:channel", post(webhook))
        .route("/api/chat", post(api_chat))
        .route("/v1/chat/completions", post(v1_chat_completions))
        .route("/v1/models", get(v1_models))
        .route("/ws/chat", get(ws_chat))
        .route("/api/*path", get(api_fallback))
        .with_state(state)
}
