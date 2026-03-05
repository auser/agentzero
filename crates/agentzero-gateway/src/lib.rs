mod auth;
mod banner;
mod handlers;
pub mod middleware;
mod models;
mod router;
mod state;
#[cfg(test)]
mod tests;
mod token_store;
mod util;

use anyhow::Context;
use std::net::SocketAddr;
use std::path::PathBuf;

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
