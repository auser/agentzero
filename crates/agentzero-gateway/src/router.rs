use crate::handlers::{
    api_chat, api_fallback, dashboard, health, legacy_webhook, metrics, pair, ping,
    v1_chat_completions, v1_models, webhook, ws_chat,
};
use crate::middleware::{self, MiddlewareConfig, RateLimiter};
use crate::state::GatewayState;
use axum::{
    body::Body,
    extract::Request,
    middleware::from_fn,
    routing::{get, post},
    Router,
};
use std::sync::Arc;

pub(crate) fn build_router(state: GatewayState, config: &MiddlewareConfig) -> Router {
    let max_bytes = config.max_body_bytes;
    let limiter = Arc::new(RateLimiter::new(
        config.rate_limit_max,
        config.rate_limit_window_secs,
    ));
    let cors_origins = config.cors_allowed_origins.clone();

    let mut router = Router::new()
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
        .route("/api/*path", get(api_fallback));

    // Request size limit middleware.
    router = router.layer(from_fn(
        move |req: Request<Body>, next: axum::middleware::Next| async move {
            middleware::request_size_limit(req, next, max_bytes).await
        },
    ));

    // Rate limiting middleware (only if configured).
    if config.rate_limit_max > 0 {
        let rate_limiter = limiter;
        router = router.layer(from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| {
                let lim = rate_limiter.clone();
                async move { middleware::rate_limit(req, next, lim).await }
            },
        ));
    }

    // CORS middleware (only if configured).
    if !cors_origins.is_empty() {
        router = router.layer(from_fn(
            move |req: Request<Body>, next: axum::middleware::Next| {
                let o = cors_origins.clone();
                async move { middleware::cors_middleware(req, next, o).await }
            },
        ));
    }

    router.with_state(state)
}
