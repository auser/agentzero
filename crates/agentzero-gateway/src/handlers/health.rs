use super::*;
use crate::models::{HealthResponse, LivenessResponse, ReadyResponse};
use axum::response::Html;
use secrecy::ExposeSecret;
use std::time::Duration;

pub(crate) async fn dashboard(State(state): State<GatewayState>) -> Html<String> {
    Html(format!(
        "<html><body><h1>{}</h1><p>OTP configured: {}</p></body></html>",
        state.service_name,
        !state.otp_secret.expose_secret().is_empty()
    ))
}

pub(crate) async fn health(State(state): State<GatewayState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: (*state.service_name).clone(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub(crate) async fn health_ready(State(state): State<GatewayState>) -> Json<ReadyResponse> {
    let mut checks_failed = Vec::new();

    // Check memory store availability when config is loaded.
    if state.memory_store.is_none() && state.config_path.is_some() {
        checks_failed.push("memory_store".to_string());
    }

    let ready = checks_failed.is_empty();
    Json(ReadyResponse {
        ready,
        service: (*state.service_name).clone(),
        version: env!("CARGO_PKG_VERSION"),
        checks_failed,
    })
}

/// GET /health/live — liveness probe that verifies the tokio runtime is responsive.
pub(crate) async fn health_live() -> Json<LivenessResponse> {
    // Spawn a trivial task and confirm it completes within 1 second.
    // If the runtime is deadlocked or overloaded, this will time out.
    let alive = tokio::time::timeout(Duration::from_secs(1), tokio::spawn(async { 42 }))
        .await
        .is_ok();
    Json(LivenessResponse { alive })
}

pub(crate) async fn metrics(State(state): State<GatewayState>) -> impl IntoResponse {
    let payload = state.prometheus_handle.render();
    ([("content-type", "text/plain; version=0.0.4")], payload)
}
