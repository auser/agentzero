use anyhow::Context;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};

#[derive(Clone)]
struct GatewayState {
    service_name: Arc<String>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: String,
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

pub async fn run(host: &str, port: u16) -> anyhow::Result<()> {
    let state = GatewayState {
        service_name: Arc::new("agentzero-gateway".to_string()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/ping", post(ping))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("invalid gateway host/port")?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind gateway listener")?;

    println!("gateway listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .await
        .context("gateway server failed")?;
    Ok(())
}

async fn health(State(state): State<GatewayState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: (*state.service_name).clone(),
    })
}

async fn ping(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse {
        ok: true,
        echo: req.message,
    })
}
