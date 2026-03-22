//! HTTP/WebSocket gateway server for AgentZero.
//!
//! Exposes the agent loop over HTTP and WebSocket endpoints with
//! pairing-based authentication, streaming responses, and node control.

#![recursion_limit = "512"]

pub(crate) mod a2a;
pub mod api_keys;
mod audit;
mod auth;
mod banner;
pub(crate) mod canvas;
pub mod gateway_channel;
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
mod openapi;
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

use agentzero_config::watcher::ConfigWatcher;
use anyhow::Context;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

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
    /// Default privacy mode when no config file sets one.
    /// Used by agentzero-lite to default to `"private"`.
    pub default_privacy_mode: Option<String>,
}

pub async fn run(host: &str, port: u16, options: GatewayRunOptions) -> anyhow::Result<()> {
    let otp_secret = generate_base32_secret(32);
    tracing::info!("Initialized OTP secret for AgentZero.");
    tracing::debug!(
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
        .and_then(|p| match agentzero_config::load(p) {
            Ok(cfg) => {
                tracing::info!(path = %p.display(), "loaded config");
                Some(cfg)
            }
            Err(e) => {
                tracing::warn!(
                    path = %p.display(),
                    error = %e,
                    "failed to load config — swarm/pipeline features will be unavailable"
                );
                None
            }
        });

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

    // Wire persistent API key store when a data directory is available.
    if let Some(ref data_dir) = options.data_dir {
        match api_keys::ApiKeyStore::persistent(data_dir) {
            Ok(store) => {
                let count = store.list_all_count();
                tracing::info!(keys = count, "loaded persistent API key store");
                state = state.with_api_key_store(Arc::new(store));
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to open persistent API key store");
            }
        }

        match agentzero_orchestrator::AgentStore::persistent(data_dir) {
            Ok(store) => {
                let count = store.count();
                tracing::info!(agents = count, "loaded persistent agent store");
                state = state.with_agent_store(Arc::new(store));
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to open persistent agent store");
            }
        }
    }

    if let (Some(config_path), Some(workspace_root)) = (options.config_path, options.workspace_root)
    {
        state = state.with_agent_paths(config_path, workspace_root);
    }

    // Build memory store for transcript retrieval if config is available.
    if let (Some(config_path), Some(cfg)) = (state.config_path.as_ref(), &full_config) {
        match build_memory_store_from_config(config_path, cfg) {
            Ok(store) => {
                state = state.with_memory_store(std::sync::Arc::from(store));
            }
            Err(e) => {
                tracing::warn!("failed to build memory store for transcripts: {e}");
            }
        }
    }

    // Build MCP server for tool execution (/v1/tool-execute and /mcp/message).
    if let Some(cfg_path) = state.config_path.as_ref() {
        let ws_root = state
            .workspace_root
            .as_ref()
            .map(|p| p.as_ref().clone())
            .unwrap_or_default();
        match agentzero_config::load_tool_security_policy(&ws_root, cfg_path) {
            Ok(policy) => match agentzero_infra::tools::default_tools(&policy, None, None) {
                Ok(tools) => {
                    let tool_count = tools.len();
                    let server = agentzero_infra::mcp_server::McpServer::new(
                        tools,
                        ws_root.to_string_lossy().to_string(),
                    );
                    state.mcp_server = Some(Arc::new(server));
                    tracing::info!(tools = tool_count, "MCP server initialized");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to build tools for MCP server");
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "failed to load tool security policy for MCP server");
            }
        }
    }

    // Build the event bus so the same instance is shared by the job store,
    // presence store, swarm coordinator, and gateway SSE/WebSocket endpoints.
    let event_bus: Arc<dyn agentzero_core::EventBus> = match full_config.as_ref() {
        Some(cfg) if cfg.swarm.enabled => {
            let workspace_root = state
                .workspace_root
                .as_ref()
                .map(|p| p.as_ref().clone())
                .unwrap_or_default();
            match agentzero_orchestrator::build_event_bus(cfg, &workspace_root).await {
                Ok(bus) => bus,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "failed to build configured event bus, falling back to in-memory"
                    );
                    Arc::new(agentzero_core::event_bus::InMemoryBus::new(256))
                }
            }
        }
        _ => Arc::new(agentzero_core::event_bus::InMemoryBus::new(256)),
    };

    // Wire up the job store and presence store for /v1/runs endpoints.
    let job_store =
        Arc::new(agentzero_orchestrator::JobStore::new().with_event_bus(event_bus.clone()));
    state = state
        .with_job_store(job_store.clone())
        .with_event_bus(event_bus.clone());

    let presence_store =
        Arc::new(agentzero_orchestrator::PresenceStore::new().with_event_bus(event_bus.clone()));
    state.presence_store = Some(presence_store);

    // Start config file watcher for live hot-reload.
    let _config_watcher_cancel = if let (Some(ref config_path), Some(ref cfg)) =
        (state.config_path.as_ref(), &full_config)
    {
        let watcher = ConfigWatcher::from_config(
            config_path.as_ref().clone(),
            Duration::from_secs(2),
            cfg.clone(),
        );
        let rx = watcher.subscribe();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        tokio::spawn(watcher.run(cancel_rx));

        // Spawn a task that logs every config reload.
        let mut log_rx = rx.clone();
        tokio::spawn(async move {
            while log_rx.changed().await.is_ok() {
                tracing::info!("gateway config updated (live reload)");
            }
        });

        state = state.with_live_config(rx);
        Some(cancel_tx)
    } else {
        None
    };

    // --- Privacy initialization ---
    // Mode acts as a preset: "private", "encrypted" or "full" auto-enables noise,
    // relay, and key rotation. Users can also individually configure components.
    // Simple mode: just set `mode = "private"` and everything works.
    // Complex mode: fine-tune noise, sealed_envelopes, key_rotation sections.
    #[cfg(feature = "privacy")]
    let _rotation_handle = {
        let config_privacy_mode = full_config.as_ref().map(|c| c.privacy.mode.clone());
        // Use the config's privacy mode if set (and not default "off"),
        // otherwise fall back to the options override (e.g. agentzero-lite
        // sets "private" here), otherwise "off".
        let effective_privacy_mode = match config_privacy_mode.as_deref() {
            Some(m) if m != "off" => m.to_string(),
            _ => options
                .default_privacy_mode
                .clone()
                .unwrap_or_else(|| "off".to_string()),
        };
        let privacy_mode = effective_privacy_mode.as_str();

        if privacy_mode != "off" {
            tracing::info!(mode = privacy_mode, "privacy mode active");
        }

        let noise_explicitly_enabled = full_config
            .as_ref()
            .is_some_and(|c| c.privacy.noise.enabled);
        let relay_explicitly_enabled = full_config
            .as_ref()
            .is_some_and(|c| c.privacy.sealed_envelopes.enabled);

        let noise_on =
            matches!(privacy_mode, "private" | "encrypted" | "full") || noise_explicitly_enabled;
        let relay_on = matches!(privacy_mode, "full") || relay_explicitly_enabled;
        let rotation_on = matches!(privacy_mode, "private" | "encrypted" | "full")
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

            let jitter = relay::JitterConfig {
                enabled: sealed_cfg.timing_jitter_enabled,
                submit_min_ms: sealed_cfg.submit_jitter_min_ms,
                submit_max_ms: sealed_cfg.submit_jitter_max_ms,
                poll_min_ms: sealed_cfg.poll_jitter_min_ms,
                poll_max_ms: sealed_cfg.poll_jitter_max_ms,
            };

            let mailbox = relay::RelayMailbox::with_jitter(
                sealed_cfg.max_envelope_bytes / 1024, // mailbox slots
                sealed_cfg.default_ttl_secs,
                jitter,
            );

            tracing::info!(
                timing_jitter = sealed_cfg.timing_jitter_enabled,
                "sealed envelope relay enabled"
            );
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

    // --- Gateway channel + Swarm coordinator ---
    let swarm_handle = if let Some(ref cfg) = full_config {
        if cfg.swarm.enabled {
            let config_path = state
                .config_path
                .as_ref()
                .map(|p| p.as_ref().clone())
                .unwrap_or_default();
            let workspace_root = state
                .workspace_root
                .as_ref()
                .map(|p| p.as_ref().clone())
                .unwrap_or_default();

            // Create the gateway channel and register it with the channel registry
            // so the swarm coordinator can receive API messages.
            let gw_channel = gateway_channel::GatewayChannel::new(256);
            {
                let channels = Arc::get_mut(&mut state.channels)
                    .expect("channels registry should be uniquely owned at this point");
                channels.register(gw_channel.clone());
            }
            state = state.with_gateway_channel(gw_channel);

            match agentzero_orchestrator::build_swarm_with_presence(
                cfg,
                state.channels.clone(),
                &config_path,
                &workspace_root,
                state.presence_store.clone(),
                event_bus.clone(),
            )
            .await
            {
                Ok(Some((coord, shutdown_tx))) => {
                    tracing::info!("swarm coordinator built, spawning");
                    let shutdown_rx = shutdown_tx.subscribe();
                    Some(tokio::spawn(async move {
                        // Hold shutdown_tx for the coordinator's lifetime so
                        // run_channel_ingestion doesn't see an immediate
                        // sender-dropped signal and abort all listeners.
                        let _shutdown_tx = shutdown_tx;
                        if let Err(e) = coord.run(shutdown_rx).await {
                            tracing::error!(error = %e, "swarm coordinator exited with error");
                        }
                    }))
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::error!(error = %e, "failed to build swarm coordinator");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Initialize OpenTelemetry tracing when the telemetry feature is enabled
    // and config has backend = "otlp". The guards must be held for the server
    // lifetime so spans are flushed on shutdown.
    #[cfg(feature = "telemetry")]
    let _telemetry_guards = {
        let obs_config = full_config
            .as_ref()
            .map(|c| c.observability.clone())
            .unwrap_or_default();
        match agentzero_infra::telemetry::init_telemetry(&obs_config) {
            Ok(guards) => guards,
            Err(e) => {
                tracing::warn!(error = %e, "failed to initialize telemetry, continuing without it");
                None
            }
        }
    };

    // ── AGENTZERO_ENV production validation ────────────────────────────
    validate_production_env(full_config.as_ref());

    // Resolve TLS config from the loaded TOML configuration.
    let tls_config = full_config.as_ref().and_then(|c| c.gateway.tls.clone());

    // Propagate TLS state into middleware config so HSTS headers are applied.
    let mut middleware_config = options.middleware.clone();
    middleware_config.tls_enabled = tls_config.is_some();

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("invalid gateway host/port")?;

    // Auto-enable CORS for localhost origins when bound to loopback and no
    // explicit CORS origins are configured.  This lets the Vite dev server
    // (typically on port 5173) talk to the gateway without extra setup.
    if middleware_config.cors_allowed_origins.is_empty() && addr.ip().is_loopback() {
        middleware_config
            .cors_allowed_origins
            .push(format!("http://localhost:{port}"));
        middleware_config
            .cors_allowed_origins
            .push("http://localhost:5173".to_string());
    }

    let app = build_router(state, &middleware_config);

    // Graceful shutdown: when the signal fires, the server stops accepting new
    // connections and waits for in-flight requests.  If connections don't close
    // within the grace period (e.g. keep-alive, WebSocket), force-exit.
    let grace_ms = full_config
        .as_ref()
        .map(|c| c.swarm.shutdown_grace_ms)
        .unwrap_or(5_000);

    // Start either a TLS or plain-TCP listener based on configuration.
    serve_gateway(addr, app, tls_config, pairing_code.as_deref(), grace_ms).await?;

    // Abort swarm coordinator on gateway shutdown.
    if let Some(handle) = swarm_handle {
        handle.abort();
        let _ = handle.await;
    }

    tracing::info!("gateway shut down");
    Ok(())
}

/// Start the gateway server, choosing TLS or plain TCP based on configuration.
async fn serve_gateway(
    addr: SocketAddr,
    app: axum::Router,
    tls_config: Option<agentzero_config::TlsConfig>,
    pairing_code: Option<&str>,
    grace_ms: u64,
) -> anyhow::Result<()> {
    match tls_config {
        #[cfg(feature = "tls")]
        Some(tls) => serve_tls(addr, app, tls, pairing_code, grace_ms).await,
        #[cfg(not(feature = "tls"))]
        Some(_) => anyhow::bail!(
            "TLS is configured in gateway.tls but the `tls` feature is not enabled. \
             Rebuild with `--features tls` to enable TLS support."
        ),
        None => serve_plain(addr, app, pairing_code, grace_ms).await,
    }
}

/// Serve over plain TCP (no TLS).
async fn serve_plain(
    addr: SocketAddr,
    app: axum::Router,
    pairing_code: Option<&str>,
    grace_ms: u64,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind gateway listener")?;

    let base = format!("http://{}", listener.local_addr()?);
    print_gateway_banner(&base, pairing_code);

    tracing::info!(address = %addr, tls = false, "gateway listening");

    let server = axum::serve(listener, app).with_graceful_shutdown(middleware::shutdown_signal());

    tokio::select! {
        result = server.into_future() => {
            result.context("gateway server failed")?;
        }
        () = async {
            middleware::shutdown_signal().await;
            tracing::info!("forcing exit in {grace_ms}ms (press Ctrl+C again to exit immediately)");
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(grace_ms)) => {
                    tracing::warn!("grace period expired, forcing exit");
                }
                () = force_shutdown_signal() => {
                    tracing::warn!("second interrupt received, forcing exit");
                }
            }
        } => {}
    }
    Ok(())
}

/// Serve over TLS using rustls.
#[cfg(feature = "tls")]
async fn serve_tls(
    addr: SocketAddr,
    app: axum::Router,
    tls: agentzero_config::TlsConfig,
    pairing_code: Option<&str>,
    grace_ms: u64,
) -> anyhow::Result<()> {
    use axum_server::tls_rustls::RustlsConfig;

    let rustls_config = RustlsConfig::from_pem_file(&tls.cert_path, &tls.key_path)
        .await
        .with_context(|| {
            format!(
                "failed to load TLS certificate from cert={} key={}",
                tls.cert_path, tls.key_path
            )
        })?;

    let base = format!("https://{addr}");
    print_gateway_banner(&base, pairing_code);

    tracing::info!(address = %addr, tls = true, "gateway listening (TLS)");

    let server = axum_server::bind_rustls(addr, rustls_config).serve(app.into_make_service());

    // axum-server doesn't have the same `with_graceful_shutdown` API as axum::serve.
    // Use a `tokio::select!` with the shutdown signal.
    tokio::select! {
        result = server => {
            result.context("TLS gateway server failed")?;
        }
        () = async {
            middleware::shutdown_signal().await;
            tracing::info!("forcing exit in {grace_ms}ms (press Ctrl+C again to exit immediately)");
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(grace_ms)) => {
                    tracing::warn!("grace period expired, forcing exit");
                }
                () = force_shutdown_signal() => {
                    tracing::warn!("second interrupt received, forcing exit");
                }
            }
        } => {}
    }
    Ok(())
}

/// Wait for a second Ctrl+C / SIGTERM after the first one has already been handled.
async fn force_shutdown_signal() {
    // The first signal was already consumed by `middleware::shutdown_signal()`.
    // Register fresh listeners for the next one.
    let ctrl_c = async {
        // ctrl_c() returns a new future each time — it will resolve on the *next* signal.
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

/// Build a memory store from gateway config for transcript retrieval.
fn build_memory_store_from_config(
    config_path: &std::path::Path,
    config: &agentzero_config::AgentZeroConfig,
) -> anyhow::Result<Box<dyn agentzero_core::MemoryStore>> {
    let backend = &config.memory.backend;
    match backend.as_str() {
        "sqlite" => {
            let config_dir = config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            let sqlite_path = resolve_sqlite_path(config_path, &config.memory.sqlite_path);
            let key = agentzero_storage::StorageKey::from_config_dir(config_dir)?;
            let pool_size = config.memory.pool_size;
            if pool_size > 1 {
                tracing::info!(pool_size, "using pooled SQLite memory store");
                Ok(Box::new(
                    agentzero_storage::memory::PooledMemoryStore::open(
                        sqlite_path,
                        Some(&key),
                        pool_size,
                    )?,
                ))
            } else {
                Ok(Box::new(
                    agentzero_storage::memory::SqliteMemoryStore::open(sqlite_path, Some(&key))?,
                ))
            }
        }
        _ => anyhow::bail!("memory backend '{backend}' not supported for gateway transcripts"),
    }
}

/// Resolve the SQLite path relative to the config file directory.
fn resolve_sqlite_path(config_path: &std::path::Path, sqlite_path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(sqlite_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(path)
    }
}

// ─── AGENTZERO_ENV production validation ────────────────────────────────────

/// Validate production requirements when `AGENTZERO_ENV=production`.
///
/// Checks:
/// - TLS is configured (cert_path + key_path present in gateway config)
/// - API key auth or pairing is enabled (not running wide-open)
///
/// Emits warnings for any missing requirements but never fails, so existing
/// deployments are not broken by the addition of these checks.
fn validate_production_env(config: Option<&agentzero_config::AgentZeroConfig>) {
    let env_val = std::env::var("AGENTZERO_ENV").unwrap_or_default();
    if env_val != "production" {
        return;
    }

    tracing::info!("AGENTZERO_ENV=production — running production validation checks");

    let mut warnings: Vec<String> = Vec::new();

    match config {
        Some(cfg) => {
            // Check TLS configuration
            match &cfg.gateway.tls {
                Some(tls) => {
                    if tls.cert_path.is_empty() {
                        warnings
                            .push("gateway.tls.cert_path is empty — TLS will not work".to_string());
                    }
                    if tls.key_path.is_empty() {
                        warnings
                            .push("gateway.tls.key_path is empty — TLS will not work".to_string());
                    }
                }
                None => {
                    if !cfg.gateway.allow_insecure {
                        warnings.push(
                            "TLS is not configured (gateway.tls section missing). \
                             Set gateway.allow_insecure = true to suppress this warning."
                                .to_string(),
                        );
                    }
                }
            }

            // Check that authentication is enabled
            if !cfg.gateway.require_pairing {
                warnings.push(
                    "gateway.require_pairing is false — the gateway accepts \
                     unauthenticated requests in production"
                        .to_string(),
                );
            }
        }
        None => {
            warnings.push(
                "no config file loaded — TLS and auth settings cannot be validated".to_string(),
            );
        }
    }

    for warning in &warnings {
        tracing::warn!(env = "production", "{}", warning);
    }

    if warnings.is_empty() {
        tracing::info!("production validation passed — all checks OK");
    }
}

/// Production validation results for testing.
#[cfg(test)]
fn collect_production_warnings(config: Option<&agentzero_config::AgentZeroConfig>) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();

    match config {
        Some(cfg) => {
            match &cfg.gateway.tls {
                Some(tls) => {
                    if tls.cert_path.is_empty() {
                        warnings.push("gateway.tls.cert_path is empty".to_string());
                    }
                    if tls.key_path.is_empty() {
                        warnings.push("gateway.tls.key_path is empty".to_string());
                    }
                }
                None => {
                    if !cfg.gateway.allow_insecure {
                        warnings.push("TLS is not configured".to_string());
                    }
                }
            }
            if !cfg.gateway.require_pairing {
                warnings.push("require_pairing is false".to_string());
            }
        }
        None => {
            warnings.push("no config file loaded".to_string());
        }
    }

    warnings
}

#[cfg(test)]
mod production_env_tests {
    use super::*;

    #[test]
    fn production_warns_when_no_tls_configured() {
        let mut config = agentzero_config::AgentZeroConfig::default();
        // Default config has no TLS and require_pairing = true, allow_insecure = false
        config.gateway.require_pairing = true;
        config.gateway.allow_insecure = false;

        let warnings = collect_production_warnings(Some(&config));

        assert!(
            warnings.iter().any(|w| w.contains("TLS")),
            "should warn about missing TLS: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.contains("require_pairing")),
            "should not warn about pairing when it is enabled: {warnings:?}"
        );
    }

    #[test]
    fn production_warns_when_pairing_disabled() {
        let mut config = agentzero_config::AgentZeroConfig::default();
        config.gateway.require_pairing = false;
        config.gateway.tls = Some(agentzero_config::TlsConfig {
            cert_path: "/etc/ssl/cert.pem".to_string(),
            key_path: "/etc/ssl/key.pem".to_string(),
        });

        let warnings = collect_production_warnings(Some(&config));

        assert!(
            warnings.iter().any(|w| w.contains("require_pairing")),
            "should warn about disabled pairing: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.contains("TLS")),
            "should not warn about TLS when it is configured: {warnings:?}"
        );
    }

    #[test]
    fn production_no_warnings_when_fully_configured() {
        let mut config = agentzero_config::AgentZeroConfig::default();
        config.gateway.require_pairing = true;
        config.gateway.tls = Some(agentzero_config::TlsConfig {
            cert_path: "/etc/ssl/cert.pem".to_string(),
            key_path: "/etc/ssl/key.pem".to_string(),
        });

        let warnings = collect_production_warnings(Some(&config));
        assert!(
            warnings.is_empty(),
            "should have no warnings when fully configured: {warnings:?}"
        );
    }

    #[test]
    fn production_warns_when_no_config() {
        let warnings = collect_production_warnings(None);
        assert!(
            warnings.iter().any(|w| w.contains("no config")),
            "should warn about missing config: {warnings:?}"
        );
    }

    #[test]
    fn production_allows_insecure_suppresses_tls_warning() {
        let mut config = agentzero_config::AgentZeroConfig::default();
        config.gateway.require_pairing = true;
        config.gateway.allow_insecure = true;
        // No TLS configured, but allow_insecure = true

        let warnings = collect_production_warnings(Some(&config));
        assert!(
            !warnings.iter().any(|w| w.contains("TLS")),
            "allow_insecure should suppress TLS warning: {warnings:?}"
        );
    }
}
