use crate::cli::AuthCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_auth::AuthStatus;
use agentzero_auth::{
    AuthManager, AuthProfileSummary, PendingOAuthLogin, ProfileHealth, RefreshStatus,
};
use agentzero_common::util::build_query_string_ordered;
use anyhow::{bail, Context};
use async_trait::async_trait;
use base64::engine::{general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::Digest;
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AuthCommand;

#[async_trait]
impl AgentZeroCommand for AuthCommand {
    type Options = AuthCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let config_dir = ctx.data_dir.clone();
        let manager = AuthManager::in_config_dir(&config_dir)?;

        match opts {
            AuthCommands::Login {
                provider,
                profile,
                device_code,
            } => {
                let provider = match provider {
                    Some(p) => normalize_oauth_provider(&p)?.to_string(),
                    None => {
                        #[cfg(feature = "interactive")]
                        {
                            let options = vec![
                                "OpenAI Codex  (browser login)",
                                "Anthropic     (paste API key)",
                                "Google Gemini (paste API key)",
                            ];
                            let selection = inquire::Select::new(
                                "Which provider do you want to log in with?",
                                options,
                            )
                            .prompt()?;
                            if selection.starts_with("OpenAI") {
                                "openai-codex".to_string()
                            } else if selection.starts_with("Anthropic") {
                                "anthropic".to_string()
                            } else {
                                "gemini".to_string()
                            }
                        }
                        #[cfg(not(feature = "interactive"))]
                        {
                            bail!("--provider is required (built without interactive feature)")
                        }
                    }
                };

                // Anthropic and Gemini use paste-key flow, not browser OAuth.
                if provider == "anthropic" || provider == "gemini" {
                    let label = if provider == "anthropic" {
                        "Paste your Anthropic API key"
                    } else {
                        "Paste your Google AI Studio API key"
                    };
                    let value = read_plain_input(label)?;
                    manager.paste_token(&profile, &provider, &value, true)?;
                    println!("Saved profile {profile}");
                    println!("Active profile for {provider}: {profile}");
                    return Ok(());
                }

                let provider = provider.as_str();
                if device_code {
                    println!(
                        "Device-code flow is not available yet. Falling back to browser/paste flow."
                    );
                }

                let preferred_port = if provider == "openai-codex" {
                    1455
                } else {
                    1456
                };
                let callback_port = match allocate_callback_port(preferred_port) {
                    Ok(port) => port,
                    Err(err_text) => {
                        println!(
                            "Callback listener probe unavailable: {err_text}. Continuing with preferred port {preferred_port}."
                        );
                        preferred_port
                    }
                };
                if callback_port != preferred_port {
                    println!(
                        "Preferred callback port {preferred_port} is unavailable. Using http://localhost:{callback_port}/auth/callback"
                    );
                }

                let (state, code_verifier) = generate_pkce_seed();
                let redirect_uri = format!("http://localhost:{callback_port}/auth/callback");
                let pending = PendingOAuthLogin {
                    provider: provider.to_string(),
                    profile: profile.clone(),
                    code_verifier: code_verifier.clone(),
                    state: state.clone(),
                    created_at_epoch_secs: now_epoch_secs(),
                    redirect_uri: Some(redirect_uri.clone()),
                };
                manager.save_pending_oauth_login(&pending)?;

                let authorize_url =
                    build_authorize_url(provider, &state, &code_verifier, callback_port);
                println!("Open this URL in your browser and authorize access:");
                println!("{authorize_url}");
                println!();

                if provider == "openai-codex" {
                    println!(
                        "Waiting for callback at http://localhost:{callback_port}/auth/callback ..."
                    );
                    match receive_loopback_code(callback_port, &state, oauth_callback_timeout()) {
                        Ok(code) => {
                            println!("Received authorization code, exchanging for token...");
                            let tokens =
                                exchange_oauth_code(provider, &code, &code_verifier, &redirect_uri)
                                    .await?;
                            manager.store_oauth_tokens(
                                &profile,
                                provider,
                                &tokens.access_token,
                                tokens.refresh_token.as_deref(),
                                tokens.expires_in,
                                true,
                            )?;
                            manager.clear_pending_oauth_login()?;
                            println!("Saved profile {profile}");
                            println!("Active profile for {provider}: {profile}");
                        }
                        Err(err_text) => {
                            println!("Callback capture failed: {err_text}");
                            println!(
                                "Run `agentzero auth paste-redirect --provider openai-codex --profile {profile}`"
                            );
                            return Ok(());
                        }
                    }
                } else {
                    println!(
                        "Run `agentzero auth paste-redirect --provider gemini --profile {profile}` if callback capture fails."
                    );
                }
            }
            AuthCommands::PasteRedirect {
                provider,
                profile,
                input,
            } => {
                let provider = normalize_oauth_provider(&provider)?;
                let pending = manager.load_pending_oauth_login()?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "No pending {} login found. Run `agentzero auth login --provider {}` first.",
                        provider_to_pending_label(provider),
                        provider
                    )
                })?;
                if !pending.provider.eq_ignore_ascii_case(provider) {
                    bail!(
                        "Pending login provider mismatch: pending={}, requested={}",
                        pending.provider,
                        provider
                    );
                }
                if !pending.profile.eq_ignore_ascii_case(&profile) {
                    bail!(
                        "Pending login profile mismatch: pending={}, requested={}",
                        pending.profile,
                        profile
                    );
                }

                let redirect_input = match input {
                    Some(value) => value,
                    None => read_plain_input("Paste redirect URL or OAuth code")?,
                };
                if let Some(found_state) = agentzero_auth::extract_oauth_state(&redirect_input) {
                    if !found_state.eq(&pending.state) {
                        bail!("OAuth state mismatch");
                    }
                }

                let code = agentzero_auth::extract_oauth_code_from_input(&redirect_input);
                let redirect_uri = pending
                    .redirect_uri
                    .unwrap_or_else(|| "http://localhost:1455/auth/callback".to_string());
                if provider == "openai-codex" {
                    let tokens =
                        exchange_oauth_code(provider, &code, &pending.code_verifier, &redirect_uri)
                            .await?;
                    manager.store_oauth_tokens(
                        &profile,
                        provider,
                        &tokens.access_token,
                        tokens.refresh_token.as_deref(),
                        tokens.expires_in,
                        true,
                    )?;
                } else {
                    manager.paste_redirect(&profile, provider, &redirect_input, true)?;
                }
                manager.clear_pending_oauth_login()?;
                println!("Saved profile {profile}");
                println!("Active profile for {provider}: {profile}");
            }
            AuthCommands::PasteToken {
                profile,
                provider,
                token,
                auth_kind: _auth_kind,
                activate,
            } => {
                let value = match token {
                    Some(value) => value,
                    None => read_plain_input("Paste setup token / auth token")?,
                };
                manager.paste_token(&profile, &provider, &value, activate)?;
                println!(
                    "saved setup token for profile `{}` provider `{}`{}",
                    profile,
                    provider,
                    if activate { " (active)" } else { "" }
                );
            }
            AuthCommands::SetupToken {
                profile,
                provider,
                token,
                activate,
            } => {
                let value = match token {
                    Some(value) => value,
                    None => {
                        let mut stdout = io::stdout();
                        write!(stdout, "enter setup token: ").context("failed to prompt")?;
                        stdout.flush().context("failed to flush prompt")?;
                        let mut input = String::new();
                        io::stdin()
                            .read_line(&mut input)
                            .context("failed to read setup token")?;
                        input.trim().to_string()
                    }
                };
                manager.paste_token(&profile, &provider, &value, activate)?;
                println!(
                    "saved setup token for profile `{}` provider `{}`{}",
                    profile,
                    provider,
                    if activate { " (active)" } else { "" }
                );
            }
            AuthCommands::Refresh { provider, profile } => {
                let provider = normalize_refresh_provider(&provider)?;
                let result = manager.refresh_for_provider(provider, profile.as_deref())?;
                match provider {
                    "openai-codex" => match result {
                        Some(found) if found.status != RefreshStatus::ExpiredNeedsLogin => {
                            println!("OpenAI Codex token is valid (refresh completed if needed).");
                        }
                        _ => {
                            bail!(
                                "No OpenAI Codex auth profile found. Run `agentzero auth login --provider openai-codex`."
                            );
                        }
                    },
                    "gemini" => match result {
                        Some(found) if found.status != RefreshStatus::ExpiredNeedsLogin => {
                            println!("✓ Gemini token refreshed successfully");
                            println!("  Profile: gemini:{}", found.profile);
                        }
                        _ => {
                            bail!(
                                "No Gemini auth profile found. Run `agentzero auth login --provider gemini`."
                            );
                        }
                    },
                    _ => unreachable!("provider normalized above"),
                }
            }
            AuthCommands::Logout { provider, profile } => {
                let provider = provider.trim();
                let profile = profile.as_deref().unwrap_or("default").trim();
                let removed = manager.remove_profile(provider, profile)?;
                if removed {
                    println!("Removed auth profile {provider}:{profile}");
                } else {
                    println!("Auth profile not found: {provider}:{profile}");
                }
            }
            AuthCommands::List { json } => {
                let profiles = manager.list_profiles()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&profiles)?);
                } else if profiles.is_empty() {
                    println!("no auth profiles configured");
                } else {
                    println!("auth profiles ({}):", profiles.len());
                    for profile in profiles {
                        println!(
                            "  {}  provider={}{}",
                            profile.name,
                            profile.provider,
                            if profile.active { " (active)" } else { "" }
                        );
                    }
                }
            }
            AuthCommands::Status { json } => {
                let status = manager.status()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    let profiles = manager.list_profiles()?;
                    let health = manager.token_health()?;
                    println!("{}", render_auth_status_text(&status, &profiles, &health));
                }
            }
            AuthCommands::Use { provider, profile } => {
                let profiles = manager.list_profiles()?;
                let matched = profiles.iter().any(|entry| {
                    entry.name.eq_ignore_ascii_case(&profile)
                        && entry.provider.eq_ignore_ascii_case(&provider)
                });
                if !matched {
                    bail!("auth profile not found for provider={provider} profile={profile}");
                }
                manager
                    .use_profile(&profile)
                    .with_context(|| format!("failed to activate profile `{profile}`"))?;
                println!("active auth profile set to `{profile}` for provider `{provider}`");
            }
        }
        Ok(())
    }
}

fn normalize_refresh_provider(provider: &str) -> anyhow::Result<&str> {
    let trimmed = provider.trim();
    if trimmed.eq_ignore_ascii_case("openai-codex")
        || trimmed.eq_ignore_ascii_case("openai_codex")
        || trimmed.eq_ignore_ascii_case("codex")
    {
        return Ok("openai-codex");
    }
    if trimmed.eq_ignore_ascii_case("gemini") || trimmed.eq_ignore_ascii_case("google-gemini") {
        return Ok("gemini");
    }
    bail!("`auth refresh` supports --provider openai-codex or gemini");
}

fn normalize_oauth_provider(provider: &str) -> anyhow::Result<&str> {
    let trimmed = provider.trim();
    if trimmed.eq_ignore_ascii_case("openai-codex")
        || trimmed.eq_ignore_ascii_case("openai_codex")
        || trimmed.eq_ignore_ascii_case("codex")
    {
        return Ok("openai-codex");
    }
    if trimmed.eq_ignore_ascii_case("gemini") || trimmed.eq_ignore_ascii_case("google-gemini") {
        return Ok("gemini");
    }
    if trimmed.eq_ignore_ascii_case("anthropic") {
        return Ok("anthropic");
    }
    bail!("`auth login` supports --provider openai-codex, gemini, or anthropic");
}

fn provider_to_pending_label(provider: &str) -> &'static str {
    if provider == "openai-codex" {
        "OpenAI"
    } else {
        "Gemini"
    }
}

fn build_authorize_url(
    provider: &str,
    state: &str,
    code_verifier: &str,
    callback_port: u16,
) -> String {
    let redirect_uri = format!("http://localhost:{callback_port}/auth/callback");
    let (base, client_id, scope) = if provider == "openai-codex" {
        (
            "https://auth.openai.com/oauth/authorize",
            openai_client_id(),
            "openid profile email offline_access",
        )
    } else {
        (
            "https://accounts.google.com/o/oauth2/v2/auth",
            "agentzero-cli",
            "openid profile email https://www.googleapis.com/auth/cloud-platform",
        )
    };

    let code_challenge = compute_code_challenge(code_verifier);

    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", &redirect_uri),
        ("scope", scope),
        ("code_challenge", &code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    if provider == "openai-codex" {
        params.push(("id_token_add_organizations", "true"));
        params.push(("codex_cli_simplified_flow", "true"));
        params.push(("originator", "codex_cli_rs"));
    }
    let query = build_query_string_ordered(&params);
    format!("{base}?{query}")
}

fn generate_pkce_seed() -> (String, String) {
    use rand::Rng;

    let now = now_epoch_secs();
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let state = format!("st_{pid:x}_{now:x}_{nanos:x}");

    // RFC 7636: code_verifier must be 43-128 chars of [A-Z]/[a-z]/[0-9]/"-"/"."/"_"/"~".
    // Generate 32 random bytes and base64url-encode (no padding) → 43 characters.
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill(&mut random_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

    (state, verifier)
}

fn compute_code_challenge(code_verifier: &str) -> String {
    let digest = sha2::Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn openai_client_id() -> &'static str {
    "app_EMoamEEZ73f0CkXaXp7hrann"
}

#[derive(serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

async fn exchange_oauth_code(
    provider: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<OAuthTokenResponse> {
    let (token_url, client_id) = if provider == "openai-codex" {
        ("https://auth.openai.com/oauth/token", openai_client_id())
    } else {
        bail!("OAuth token exchange not yet implemented for {provider}");
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .context("failed to exchange authorization code")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("token exchange failed (HTTP {status}): {body}");
    }

    resp.json::<OAuthTokenResponse>()
        .await
        .context("failed to parse token exchange response")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}

fn read_plain_input(prompt: &str) -> anyhow::Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{prompt}: ").context("failed to prompt")?;
    stdout.flush().context("failed to flush prompt")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read redirect input")?;
    Ok(input.trim().to_string())
}

fn receive_loopback_code(
    port: u16,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format_bind_error(port, &err, None))?;
    listener
        .set_nonblocking(false)
        .map_err(|err| format!("Failed to configure callback listener: {err}"))?;
    listener
        .set_ttl(64)
        .map_err(|err| format!("Failed to configure callback listener: {err}"))?;

    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        listener
            .set_nonblocking(true)
            .map_err(|err| format!("Failed to configure callback listener: {err}"))?;
        match listener.accept() {
            Ok((mut stream, _addr)) => {
                let mut buf = [0_u8; 4096];
                let read = stream
                    .read(&mut buf)
                    .map_err(|err| format!("Failed to read callback request: {err}"))?;
                let request = String::from_utf8_lossy(&buf[..read]);
                let path = parse_request_path(&request)
                    .ok_or_else(|| "Failed to parse callback request path".to_string())?;
                let code = parse_query_value(path, "code")
                    .ok_or_else(|| "OAuth callback missing `code` parameter".to_string())?;
                let returned_state = parse_query_value(path, "state")
                    .ok_or_else(|| "OAuth callback missing `state` parameter".to_string())?;
                if returned_state != expected_state {
                    let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 20\r\nConnection: close\r\n\r\nOAuth state mismatch");
                    return Err("OAuth state mismatch".to_string());
                }

                let body = b"AgentZero: authentication complete. You can close this tab.";
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
                return Ok(code);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(format!("Failed waiting for callback: {err}")),
        }
    }

    Err("Timed out waiting for OAuth callback".to_string())
}

fn parse_request_path(request: &str) -> Option<&str> {
    let mut lines = request.lines();
    let first = lines.next()?;
    let mut parts = first.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next()
}

fn parse_query_value(path: &str, key: &str) -> Option<String> {
    let (_, query) = path.split_once('?')?;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.into_owned())
}

fn try_bind_callback_listener(port: u16) -> Result<std::net::TcpListener, String> {
    let primary = format!("127.0.0.1:{port}");
    match std::net::TcpListener::bind(&primary) {
        Ok(listener) => Ok(listener),
        Err(primary_err) => {
            let secondary = format!("localhost:{port}");
            match std::net::TcpListener::bind(&secondary) {
                Ok(listener) => Ok(listener),
                Err(secondary_err) => {
                    Err(format_bind_error(port, &primary_err, Some(&secondary_err)))
                }
            }
        }
    }
}

fn format_bind_error(
    port: u16,
    primary: &std::io::Error,
    secondary: Option<&std::io::Error>,
) -> String {
    let primary_hint = if primary.kind() == std::io::ErrorKind::AddrInUse {
        " (address already in use)"
    } else {
        ""
    };
    match secondary {
        Some(other) => format!(
            "Failed to bind callback listener at 127.0.0.1:{port}: {primary}{primary_hint}; localhost attempt also failed: {other}"
        ),
        None => format!(
            "Failed to bind callback listener at 127.0.0.1:{port}: {primary}{primary_hint}"
        ),
    }
}

fn allocate_callback_port(preferred_port: u16) -> Result<u16, String> {
    let mut last_error = None;
    for candidate in preferred_port..preferred_port.saturating_add(20) {
        match try_bind_callback_listener(candidate) {
            Ok(_) => return Ok(candidate),
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format!("Failed to bind callback listener near preferred port {preferred_port}")
    }))
}

fn render_auth_status_text(
    status: &AuthStatus,
    profiles: &[AuthProfileSummary],
    health: &[ProfileHealth],
) -> String {
    if profiles.is_empty() {
        return "No auth profiles configured.".to_string();
    }

    let mut lines = Vec::new();
    for profile in profiles {
        let marker = if profile.active { "*" } else { " " };
        let profile_id = format!("{}:{}", profile.provider, profile.name);
        let kind = if is_oauth_provider(&profile.provider) {
            "OAuth"
        } else {
            "Token"
        };
        let account = "unknown";
        let expires = format_expiry(profile.token_expires_at_epoch_secs);

        let health_label = health
            .iter()
            .find(|h| {
                h.name.eq_ignore_ascii_case(&profile.name)
                    && h.provider.eq_ignore_ascii_case(&profile.provider)
            })
            .map(|h| h.health.label())
            .unwrap_or("unknown");

        lines.push(format!(
            "{marker} {profile_id} kind={kind} account={account} expires={expires} health={health_label}"
        ));
    }

    lines.push(String::new());
    lines.push("Active profiles:".to_string());
    if let (Some(provider), Some(name)) = (&status.active_provider, &status.active_profile) {
        lines.push(format!("  {provider}: {provider}:{name}"));
    } else {
        lines.push("  none".to_string());
    }

    lines.join("\n")
}

fn is_oauth_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case("openai-codex")
        || provider.eq_ignore_ascii_case("openai_codex")
        || provider.eq_ignore_ascii_case("codex")
        || provider.eq_ignore_ascii_case("gemini")
        || provider.eq_ignore_ascii_case("google-gemini")
}

fn format_expiry(expiry_epoch_secs: Option<u64>) -> String {
    match expiry_epoch_secs {
        Some(value) => {
            let now = now_epoch_secs();
            if value <= now {
                format!("expired at {value}")
            } else {
                let mins = (value - now) / 60;
                format!("expires in {mins}m ({value})")
            }
        }
        None => "n/a".to_string(),
    }
}

#[cfg(test)]
fn oauth_callback_timeout() -> Duration {
    Duration::from_millis(25)
}

#[cfg(not(test))]
fn oauth_callback_timeout() -> Duration {
    Duration::from_secs(180)
}

#[cfg(test)]
mod tests {
    use super::{
        build_authorize_url, format_bind_error, parse_query_value, parse_request_path,
        render_auth_status_text, AuthCommand,
    };
    use crate::cli::AuthCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use agentzero_auth::{ProfileHealth, TokenHealth};
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-cli-auth-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn auth_command_login_and_status_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        AuthCommand::run(
            &ctx,
            AuthCommands::Login {
                provider: Some("openai-codex".to_string()),
                profile: "default".to_string(),
                device_code: false,
            },
        )
        .await
        .expect("auth login should succeed");

        // PasteRedirect with wrong state should fail.
        AuthCommand::run(
            &ctx,
            AuthCommands::PasteRedirect {
                provider: "openai-codex".to_string(),
                profile: "default".to_string(),
                input: Some("https://example.test/callback?code=tok-test&state=stale".to_string()),
            },
        )
        .await
        .expect_err("state mismatch should fail");

        let manager =
            agentzero_auth::AuthManager::in_config_dir(&dir).expect("manager should construct");
        let pending = manager
            .load_pending_oauth_login()
            .expect("pending oauth should be readable")
            .expect("pending oauth should exist");
        assert!(pending.redirect_uri.is_some());

        // Simulate successful token exchange by storing tokens directly
        // (real exchange requires network access to auth.openai.com).
        manager
            .store_oauth_tokens(
                "default",
                "openai-codex",
                "access-tok",
                Some("refresh-tok"),
                Some(3600),
                true,
            )
            .expect("store oauth tokens should succeed");
        manager
            .clear_pending_oauth_login()
            .expect("clear pending should succeed");

        AuthCommand::run(&ctx, AuthCommands::Status { json: true })
            .await
            .expect("auth status should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_use_missing_profile_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = AuthCommand::run(
            &ctx,
            AuthCommands::Use {
                provider: "openai-codex".to_string(),
                profile: "missing".to_string(),
            },
        )
        .await
        .expect_err("using missing profile should fail");
        assert!(err.to_string().contains("auth profile not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_use_with_provider_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let manager =
            agentzero_auth::AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok", true)
            .expect("seed login should succeed");

        AuthCommand::run(
            &ctx,
            AuthCommands::Use {
                provider: "openai-codex".to_string(),
                profile: "default".to_string(),
            },
        )
        .await
        .expect("auth use should succeed");

        let status = manager.status().expect("status should load");
        assert_eq!(status.active_profile.as_deref(), Some("default"));
        assert_eq!(status.active_provider.as_deref(), Some("openai-codex"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_refresh_missing_profile_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = AuthCommand::run(
            &ctx,
            AuthCommands::Refresh {
                provider: "openai-codex".to_string(),
                profile: Some("missing".to_string()),
            },
        )
        .await
        .expect_err("refresh on missing profile should fail");
        assert!(err
            .to_string()
            .contains("No OpenAI Codex auth profile found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_refresh_openai_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        // Seed a profile directly (token exchange requires network).
        let manager =
            agentzero_auth::AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .store_oauth_tokens(
                "default",
                "openai-codex",
                "access-tok",
                Some("refresh-tok"),
                Some(3600),
                true,
            )
            .expect("store oauth tokens should succeed");

        AuthCommand::run(
            &ctx,
            AuthCommands::Refresh {
                provider: "openai-codex".to_string(),
                profile: Some("default".to_string()),
            },
        )
        .await
        .expect("refresh should succeed for existing openai-codex profile");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_refresh_rejects_unsupported_provider_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let err = AuthCommand::run(
            &ctx,
            AuthCommands::Refresh {
                provider: "anthropic".to_string(),
                profile: None,
            },
        )
        .await
        .expect_err("unsupported provider should fail");
        assert!(err
            .to_string()
            .contains("supports --provider openai-codex or gemini"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_logout_with_provider_success_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        let manager =
            agentzero_auth::AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok", true)
            .expect("seed login should succeed");

        AuthCommand::run(
            &ctx,
            AuthCommands::Logout {
                provider: "openai-codex".to_string(),
                profile: None,
            },
        )
        .await
        .expect("logout should succeed");

        let listed = manager.list_profiles().expect("profiles should load");
        assert!(listed.is_empty());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn auth_command_logout_missing_profile_negative_path() {
        let dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: dir.join("agentzero.toml"),
        };

        AuthCommand::run(
            &ctx,
            AuthCommands::Logout {
                provider: "openai-codex".to_string(),
                profile: Some("missing".to_string()),
            },
        )
        .await
        .expect("missing logout still succeeds with not-found message");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn format_bind_error_includes_address_in_use_hint_negative_path() {
        let err = io::Error::new(io::ErrorKind::AddrInUse, "Address already in use");
        let text = format_bind_error(1455, &err, None);
        assert!(text.contains("127.0.0.1:1455"));
        assert!(text.contains("address already in use"));
    }

    #[test]
    fn build_authorize_url_uses_selected_callback_port_success_path() {
        let url = build_authorize_url("openai-codex", "state-1", "verifier-1", 1460);
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1460%2Fauth%2Fcallback"));
        // The code_challenge should be the SHA256 hash of the verifier, not the raw verifier.
        assert!(!url.contains("code_challenge=verifier-1"));
        let expected_challenge = super::compute_code_challenge("verifier-1");
        assert!(url.contains(&format!("code_challenge={expected_challenge}")));
    }

    #[test]
    fn pkce_verifier_meets_rfc7636_length_requirement() {
        let (_state, verifier) = super::generate_pkce_seed();
        // RFC 7636 requires 43-128 characters.
        assert!(
            verifier.len() >= 43 && verifier.len() <= 128,
            "verifier length {} not in 43..=128",
            verifier.len()
        );
    }

    #[test]
    fn pkce_challenge_is_base64url_sha256_of_verifier() {
        use base64::engine::{general_purpose::URL_SAFE_NO_PAD, Engine};
        use sha2::Digest;

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = super::compute_code_challenge(verifier);
        // Manually compute expected value.
        let digest = sha2::Sha256::digest(verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(digest);
        assert_eq!(challenge, expected);
    }

    #[test]
    fn parse_request_path_and_query_extract_code_and_state_success_path() {
        let req = "GET /auth/callback?code=abc123&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let path = parse_request_path(req).expect("path should parse");
        assert_eq!(parse_query_value(path, "code").as_deref(), Some("abc123"));
        assert_eq!(parse_query_value(path, "state").as_deref(), Some("s1"));
    }

    #[test]
    fn parse_request_path_rejects_non_get_negative_path() {
        let req = "POST /auth/callback?code=abc123&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert!(parse_request_path(req).is_none());
    }

    #[test]
    fn render_auth_status_text_formats_human_readable_output_success_path() {
        let status = agentzero_auth::AuthStatus {
            active_profile: Some("default".to_string()),
            active_provider: Some("openai-codex".to_string()),
            active_token_expires_at_epoch_secs: Some(12345),
            active_has_refresh_token: true,
            total_profiles: 2,
        };
        let profiles = vec![
            agentzero_auth::AuthProfileSummary {
                name: "default".to_string(),
                provider: "openai-codex".to_string(),
                active: true,
                created_at_epoch_secs: 1,
                updated_at_epoch_secs: 1,
                has_refresh_token: true,
                token_expires_at_epoch_secs: Some(4_102_444_800),
            },
            agentzero_auth::AuthProfileSummary {
                name: "backup".to_string(),
                provider: "anthropic".to_string(),
                active: false,
                created_at_epoch_secs: 1,
                updated_at_epoch_secs: 1,
                has_refresh_token: false,
                token_expires_at_epoch_secs: None,
            },
        ];

        let health = vec![
            ProfileHealth {
                name: "default".to_string(),
                provider: "openai-codex".to_string(),
                health: TokenHealth::Valid,
                has_refresh_token: true,
                expires_at_epoch_secs: Some(4_102_444_800),
            },
            ProfileHealth {
                name: "backup".to_string(),
                provider: "anthropic".to_string(),
                health: TokenHealth::NoExpiry,
                has_refresh_token: false,
                expires_at_epoch_secs: None,
            },
        ];

        let rendered = render_auth_status_text(&status, &profiles, &health);
        assert!(rendered.contains("* openai-codex:default"));
        assert!(rendered.contains("kind=OAuth"));
        assert!(rendered.contains("health=valid"));
        assert!(rendered.contains("Active profiles:"));
        assert!(rendered.contains("openai-codex: openai-codex:default"));
    }

    #[test]
    fn render_auth_status_text_handles_empty_profiles_negative_path() {
        let status = agentzero_auth::AuthStatus {
            active_profile: None,
            active_provider: None,
            active_token_expires_at_epoch_secs: None,
            active_has_refresh_token: false,
            total_profiles: 0,
        };

        let rendered = render_auth_status_text(&status, &[], &[]);
        assert_eq!(rendered, "No auth profiles configured.");
    }
}
