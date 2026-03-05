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

    // Noise Protocol handshake and transport routes (privacy feature).
    #[cfg(feature = "privacy")]
    {
        use crate::noise_handshake::{noise_handshake_step1, noise_handshake_step2};

        router = router
            .route("/v1/noise/handshake/step1", post(noise_handshake_step1))
            .route("/v1/noise/handshake/step2", post(noise_handshake_step2))
            .route("/v1/privacy/info", get(privacy_info));

        // Noise transport middleware: decrypt request / encrypt response for
        // requests that carry the X-Noise-Session header.
        if let Some(ref sessions) = state.noise_sessions {
            let sessions = sessions.clone();
            router = router.layer(from_fn(
                move |req: Request<Body>, next: axum::middleware::Next| {
                    let s = sessions.clone();
                    async move {
                        crate::noise_middleware::noise_transport_middleware(req, next, s).await
                    }
                },
            ));
        }

        // Relay routes: submit and poll sealed envelopes.
        if state.relay_mode {
            use crate::relay::{relay_poll, relay_submit, strip_metadata_headers};

            router = router
                .route("/v1/relay/submit", post(relay_submit))
                .route("/v1/relay/poll/:routing_id", get(relay_poll))
                .layer(from_fn(strip_metadata_headers));
        }
    }

    // Request metrics middleware (outermost — records all requests).
    router = router.layer(from_fn(middleware::request_metrics));

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

/// GET /v1/privacy/info — returns gateway privacy capabilities.
/// Clients call this to discover what's available before initiating handshake.
#[cfg(feature = "privacy")]
async fn privacy_info(
    axum::extract::State(state): axum::extract::State<GatewayState>,
) -> axum::Json<serde_json::Value> {
    use base64::Engine as _;

    let noise_enabled = state.noise_sessions.is_some();
    let relay_mode = state.relay_mode;

    let (public_key, key_fingerprint) = if let Some(ref kp) = state.noise_keypair {
        let pk = base64::engine::general_purpose::STANDARD.encode(kp.public);
        // Fingerprint: first 8 bytes of routing_id (SHA-256 of public key), hex-encoded.
        let routing_id = kp.routing_id();
        let fingerprint = routing_id[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        (Some(pk), Some(fingerprint))
    } else {
        (None, None)
    };

    axum::Json(serde_json::json!({
        "noise_enabled": noise_enabled,
        "handshake_pattern": "XX",
        "public_key": public_key,
        "key_fingerprint": key_fingerprint,
        "sealed_envelopes_enabled": relay_mode,
        "relay_mode": relay_mode,
    }))
}
