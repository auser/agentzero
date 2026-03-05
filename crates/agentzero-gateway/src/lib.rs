//! HTTP/WebSocket gateway server for AgentZero.
//!
//! Exposes the agent loop over HTTP and WebSocket endpoints with
//! pairing-based authentication, streaming responses, and node control.

mod auth;
mod banner;
mod gateway_metrics;
mod handlers;
#[cfg(feature = "privacy")]
mod key_rotation;
pub mod middleware;
mod models;
#[cfg(feature = "privacy")]
mod noise_handshake;
#[cfg(feature = "privacy")]
mod noise_middleware;
#[cfg(feature = "privacy")]
pub mod privacy_state;
#[cfg(feature = "privacy")]
pub(crate) mod relay;
mod router;
mod state;
#[cfg(test)]
mod tests;
mod token_store;
mod util;

use anyhow::Context;
use std::net::SocketAddr;
use std::path::PathBuf;
#[cfg(feature = "privacy")]
use std::sync::Arc;

use banner::print_gateway_banner;
use middleware::MiddlewareConfig;
use router::build_router;
use state::GatewayState;
use token_store::{clear_paired_tokens, load_paired_tokens};
use util::{generate_base32_secret, generate_pairing_code};

#[derive(Debug, Clone, Default)]
pub struct GatewayRunOptions {
    pub token_store_path: Option<PathBuf>,
    pub new_pairing: bool,
    /// Middleware configuration (rate limiting, CORS, request size limits).
    pub middleware: MiddlewareConfig,
    /// Path to agentzero.toml config file (enables real agent execution).
    pub config_path: Option<PathBuf>,
    /// Workspace root directory.
    pub workspace_root: Option<PathBuf>,
    /// Data directory for persistent state (keyrings, tokens, etc.).
    pub data_dir: Option<PathBuf>,
}

pub async fn run(host: &str, port: u16, options: GatewayRunOptions) -> anyhow::Result<()> {
    let otp_secret = generate_base32_secret(32);
    println!("Initialized OTP secret for AgentZero.");
    println!(
        "Enrollment URI: otpauth://totp/AgentZero:agentzero?secret={otp_secret}&issuer=AgentZero&period=30"
    );

    if options.new_pairing {
        clear_paired_tokens(options.token_store_path.as_deref())?;
        println!("Cleared paired gateway tokens; generating fresh pairing code.");
    }

    // Load config from TOML if available.
    let full_config = options
        .config_path
        .as_ref()
        .and_then(|p| agentzero_config::load(p).ok());

    let (require_pairing, allow_public_bind) = match full_config.as_ref().map(|c| &c.gateway) {
        Some(gw) => (gw.require_pairing, gw.allow_public_bind),
        None => (true, false),
    };

    // Enforce allow_public_bind: refuse non-loopback addresses unless config allows it.
    if !allow_public_bind {
        let check_addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .context("invalid gateway host/port")?;
        if !check_addr.ip().is_loopback() {
            anyhow::bail!(
                "gateway: binding to non-loopback address {host} is not allowed \
                 (set gateway.allow_public_bind = true in config to override)"
            );
        }
    }

    let prometheus_handle = gateway_metrics::init_prometheus();

    let paired_tokens = load_paired_tokens(options.token_store_path.as_deref())?;
    let pairing_code = if paired_tokens.is_empty() {
        Some(generate_pairing_code())
    } else {
        None
    };
    let mut state = GatewayState::new(
        pairing_code.clone(),
        otp_secret,
        paired_tokens,
        options.token_store_path,
        prometheus_handle,
    )
    .with_gateway_config(require_pairing, allow_public_bind);

    // Wire perplexity filter from loaded security config.
    if let Some(ref cfg) = full_config {
        let pf = &cfg.security.perplexity_filter;
        state =
            state.with_perplexity_filter(agentzero_channels::pipeline::PerplexityFilterSettings {
                enabled: pf.enable_perplexity_filter,
                perplexity_threshold: pf.perplexity_threshold,
                suffix_window_chars: pf.suffix_window_chars,
                min_prompt_chars: pf.min_prompt_chars,
                symbol_ratio_threshold: pf.symbol_ratio_threshold,
            });
    }

    if let (Some(config_path), Some(workspace_root)) = (options.config_path, options.workspace_root)
    {
        state = state.with_agent_paths(config_path, workspace_root);
    }

    // --- Privacy initialization ---
    // Mode acts as a preset: "encrypted" or "full" auto-enables noise, relay, and
    // key rotation. Users can also individually configure components for advanced use.
    // Simple mode: just set `mode = "encrypted"` and everything works.
    // Complex mode: fine-tune noise, sealed_envelopes, key_rotation sections.
    #[cfg(feature = "privacy")]
    let _rotation_handle = {
        let privacy_mode = full_config
            .as_ref()
            .map(|c| c.privacy.mode.as_str())
            .unwrap_or("off");

        let noise_explicitly_enabled = full_config
            .as_ref()
            .is_some_and(|c| c.privacy.noise.enabled);
        let relay_explicitly_enabled = full_config
            .as_ref()
            .is_some_and(|c| c.privacy.sealed_envelopes.enabled);

        let noise_on = matches!(privacy_mode, "encrypted" | "full") || noise_explicitly_enabled;
        let relay_on = matches!(privacy_mode, "full") || relay_explicitly_enabled;
        let rotation_on = matches!(privacy_mode, "encrypted" | "full")
            || full_config
                .as_ref()
                .is_some_and(|c| c.privacy.key_rotation.enabled);

        let mut rotation_handle: Option<tokio::task::JoinHandle<()>> = None;

        if noise_on {
            let noise_cfg = full_config
                .as_ref()
                .map(|c| &c.privacy.noise)
                .cloned()
                .unwrap_or_default();

            let sessions = privacy_state::NoiseSessionStore::new(
                noise_cfg.max_sessions,
                noise_cfg.session_timeout_secs,
            );

            let keypair = agentzero_core::privacy::noise::NoiseKeypair::generate()
                .context("failed to generate noise keypair")?;

            tracing::info!(
                pattern = %noise_cfg.handshake_pattern,
                max_sessions = noise_cfg.max_sessions,
                "noise protocol enabled"
            );

            state = state.with_noise_privacy(sessions, keypair);
        }

        if relay_on {
            let sealed_cfg = full_config
                .as_ref()
                .map(|c| &c.privacy.sealed_envelopes)
                .cloned()
                .unwrap_or_default();

            let mailbox = relay::RelayMailbox::new(
                sealed_cfg.max_envelope_bytes / 1024, // mailbox slots
                sealed_cfg.default_ttl_secs,
            );

            tracing::info!("sealed envelope relay enabled");
            state = state.with_relay_mode(mailbox);
        }

        if rotation_on {
            let kr_cfg = full_config
                .as_ref()
                .map(|c| &c.privacy.key_rotation)
                .cloned()
                .unwrap_or_default();

            let keyring = agentzero_core::privacy::keyring::PrivacyKeyRing::new(
                kr_cfg.rotation_interval_secs,
                kr_cfg.overlap_secs,
            );

            tracing::info!(
                interval_secs = kr_cfg.rotation_interval_secs,
                overlap_secs = kr_cfg.overlap_secs,
                epoch = keyring.epoch(),
                "key rotation enabled"
            );

            let keyring = Arc::new(tokio::sync::Mutex::new(keyring));

            // Persist keyring after rotation if data_dir is available.
            let data_dir = options.data_dir.clone();
            let keyring_for_task = keyring.clone();
            let check_interval = std::cmp::max(kr_cfg.rotation_interval_secs / 10, 60);
            rotation_handle = Some(key_rotation::spawn_rotation_task_with_persistence(
                keyring_for_task,
                check_interval,
                data_dir,
            ));
        }

        rotation_handle
    };

    let app = build_router(state, &options.middleware);

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("invalid gateway host/port")?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind gateway listener")?;

    let base = format!("http://{}", listener.local_addr()?);
    print_gateway_banner(&base, pairing_code.as_deref());

    tracing::info!(address = %addr, "gateway listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(middleware::shutdown_signal())
        .await
        .context("gateway server failed")?;

    tracing::info!("gateway shut down gracefully");
    Ok(())
}
