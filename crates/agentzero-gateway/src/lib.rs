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

    let paired_tokens = load_paired_tokens(options.token_store_path.as_deref())?;
    let pairing_code = if paired_tokens.is_empty() {
        Some(generate_pairing_code())
    } else {
        None
    };
    let state = GatewayState::new(
        pairing_code.clone(),
        otp_secret,
        paired_tokens,
        options.token_store_path,
    );

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
