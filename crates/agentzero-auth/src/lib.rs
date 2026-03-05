//! Credential management for AgentZero.
//!
//! Handles API key storage and authentication profiles for multiple LLM
//! providers. Credentials are persisted in an encrypted JSON store.

use agentzero_storage::EncryptedJsonStore;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    pub name: String,
    pub provider: String,
    pub token: String,
    pub created_at_epoch_secs: u64,
    pub updated_at_epoch_secs: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_expires_at_epoch_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AuthState {
    active_profile: Option<String>,
    profiles: Vec<AuthProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PendingOAuthLogin {
    pub provider: String,
    pub profile: String,
    pub code_verifier: String,
    pub state: String,
    pub created_at_epoch_secs: u64,
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthProfileSummary {
    pub name: String,
    pub provider: String,
    pub active: bool,
    pub created_at_epoch_secs: u64,
    pub updated_at_epoch_secs: u64,
    pub has_refresh_token: bool,
    pub token_expires_at_epoch_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub active_profile: Option<String>,
    pub active_provider: Option<String>,
    pub active_token_expires_at_epoch_secs: Option<u64>,
    pub active_has_refresh_token: bool,
    pub total_profiles: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RefreshStatus {
    Valid,
    Refreshed,
    ExpiredNeedsLogin,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RefreshResult {
    pub profile: String,
    pub status: RefreshStatus,
}

/// The result of credential resolution from auth profiles.
#[derive(Debug, Clone)]
pub struct ResolvedCredential {
    /// The API token / access token.
    pub token: String,
    /// The provider kind the token belongs to (e.g. "openai-codex").
    pub provider: String,
    /// How the credential was resolved.
    pub source: CredentialSource,
}

/// Describes which resolution path produced the credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// An explicitly requested profile by name.
    ExplicitProfile(String),
    /// The active profile matched the requested provider.
    ProviderMatch,
    /// The active profile is for a different provider than requested.
    /// The caller should update its provider config to match.
    ActiveProfile(String),
}

pub struct AuthManager {
    state_store: EncryptedJsonStore,
    pending_store: EncryptedJsonStore,
}

impl AuthManager {
    pub fn in_config_dir(config_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            state_store: EncryptedJsonStore::in_config_dir(config_dir, "auth_profiles.json")?,
            pending_store: EncryptedJsonStore::in_config_dir(
                config_dir,
                "auth_pending_oauth.json",
            )?,
        })
    }

    pub fn login(
        &self,
        profile_name: &str,
        provider: &str,
        token: &str,
        activate: bool,
    ) -> anyhow::Result<()> {
        self.upsert_token(profile_name, provider, token, None, None, activate)
    }

    pub fn paste_token(
        &self,
        profile_name: &str,
        provider: &str,
        token: &str,
        activate: bool,
    ) -> anyhow::Result<()> {
        self.upsert_token(profile_name, provider, token, None, None, activate)
    }

    pub fn paste_redirect(
        &self,
        profile_name: &str,
        provider: &str,
        redirect_or_code: &str,
        activate: bool,
    ) -> anyhow::Result<()> {
        let code = extract_oauth_code(redirect_or_code);
        self.upsert_token(profile_name, provider, &code, None, None, activate)
    }

    pub fn store_oauth_tokens(
        &self,
        profile_name: &str,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_in_secs: Option<u64>,
        activate: bool,
    ) -> anyhow::Result<()> {
        self.upsert_token(
            profile_name,
            provider,
            access_token,
            refresh_token,
            expires_in_secs,
            activate,
        )
    }

    pub fn save_pending_oauth_login(&self, pending: &PendingOAuthLogin) -> anyhow::Result<()> {
        self.pending_store.save(pending)
    }

    pub fn load_pending_oauth_login(&self) -> anyhow::Result<Option<PendingOAuthLogin>> {
        self.pending_store.load_optional()
    }

    pub fn clear_pending_oauth_login(&self) -> anyhow::Result<()> {
        self.pending_store.delete()
    }

    pub fn refresh(
        &self,
        profile_name: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_in_secs: Option<u64>,
        activate: bool,
    ) -> anyhow::Result<()> {
        if profile_name.trim().is_empty() {
            return Err(anyhow!("profile name must not be empty"));
        }
        if access_token.trim().is_empty() {
            return Err(anyhow!("access token must not be empty"));
        }

        let mut state = self.load_state()?;
        let Some(existing) = state
            .profiles
            .iter_mut()
            .find(|profile| profile.name.eq_ignore_ascii_case(profile_name))
        else {
            return Err(anyhow!("profile `{profile_name}` not found"));
        };

        let now = now_epoch_secs();
        existing.token = access_token.trim().to_string();
        if let Some(value) = refresh_token {
            if !value.trim().is_empty() {
                existing.refresh_token = Some(value.trim().to_string());
            }
        }
        existing.token_expires_at_epoch_secs =
            expires_in_secs.map(|ttl| now.saturating_add(ttl.max(1)));
        existing.updated_at_epoch_secs = now;
        if activate {
            state.active_profile = Some(profile_name.trim().to_string());
        }
        self.persist_state(&state)
    }

    pub fn refresh_for_provider(
        &self,
        provider: &str,
        profile_name: Option<&str>,
    ) -> anyhow::Result<Option<RefreshResult>> {
        if provider.trim().is_empty() {
            return Err(anyhow!("provider must not be empty"));
        }

        let mut state = self.load_state()?;
        let now = now_epoch_secs();
        let selected_idx = self.find_refresh_profile_index(&state, provider, profile_name);

        let Some(idx) = selected_idx else {
            return Ok(None);
        };

        let selected = &mut state.profiles[idx];
        let expiry = selected.token_expires_at_epoch_secs;
        let is_expired = expiry.is_some_and(|value| value <= now.saturating_add(60));
        if !is_expired {
            return Ok(Some(RefreshResult {
                profile: selected.name.clone(),
                status: RefreshStatus::Valid,
            }));
        }

        let has_refresh = selected
            .refresh_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if !has_refresh {
            return Ok(Some(RefreshResult {
                profile: selected.name.clone(),
                status: RefreshStatus::ExpiredNeedsLogin,
            }));
        }

        selected.token_expires_at_epoch_secs = Some(now.saturating_add(3600));
        selected.updated_at_epoch_secs = now;
        let profile = selected.name.clone();
        self.persist_state(&state)?;
        Ok(Some(RefreshResult {
            profile,
            status: RefreshStatus::Refreshed,
        }))
    }

    pub fn logout(&self, profile_name: Option<&str>) -> anyhow::Result<bool> {
        let mut state = self.load_state()?;
        match profile_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(name) => {
                let before = state.profiles.len();
                state
                    .profiles
                    .retain(|profile| !profile.name.eq_ignore_ascii_case(name));
                if state
                    .active_profile
                    .as_deref()
                    .is_some_and(|active| active.eq_ignore_ascii_case(name))
                {
                    state.active_profile = None;
                }
                let changed = before != state.profiles.len();
                if changed {
                    self.persist_state(&state)?;
                }
                Ok(changed)
            }
            None => {
                let had_active = state.active_profile.take().is_some();
                if had_active {
                    self.persist_state(&state)?;
                }
                Ok(had_active)
            }
        }
    }

    pub fn remove_profile(&self, provider: &str, profile_name: &str) -> anyhow::Result<bool> {
        let provider = provider.trim();
        let profile_name = profile_name.trim();
        if provider.is_empty() || profile_name.is_empty() {
            return Ok(false);
        }

        let mut state = self.load_state()?;
        let before = state.profiles.len();
        state.profiles.retain(|profile| {
            !(profile.provider.eq_ignore_ascii_case(provider)
                && profile.name.eq_ignore_ascii_case(profile_name))
        });

        if state
            .active_profile
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(profile_name))
            && !state
                .profiles
                .iter()
                .any(|profile| profile.name.eq_ignore_ascii_case(profile_name))
        {
            state.active_profile = None;
        }

        let changed = before != state.profiles.len();
        if changed {
            self.persist_state(&state)?;
        }
        Ok(changed)
    }

    pub fn use_profile(&self, profile_name: &str) -> anyhow::Result<()> {
        if profile_name.trim().is_empty() {
            return Err(anyhow!("profile name must not be empty"));
        }

        let mut state = self.load_state()?;
        if !state
            .profiles
            .iter()
            .any(|profile| profile.name.eq_ignore_ascii_case(profile_name))
        {
            return Err(anyhow!("profile `{profile_name}` not found"));
        }
        state.active_profile = Some(profile_name.trim().to_string());
        self.persist_state(&state)
    }

    pub fn list_profiles(&self) -> anyhow::Result<Vec<AuthProfileSummary>> {
        let state = self.load_state()?;
        let active = state.active_profile.unwrap_or_default();
        Ok(state
            .profiles
            .into_iter()
            .map(|profile| AuthProfileSummary {
                active: profile.name.eq_ignore_ascii_case(&active),
                name: profile.name,
                provider: profile.provider,
                created_at_epoch_secs: profile.created_at_epoch_secs,
                updated_at_epoch_secs: profile.updated_at_epoch_secs,
                has_refresh_token: profile
                    .refresh_token
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false),
                token_expires_at_epoch_secs: profile.token_expires_at_epoch_secs,
            })
            .collect())
    }

    /// Return the stored token for the given provider, preferring the active
    /// profile, then a profile named "default", then any matching profile.
    pub fn active_token_for_provider(&self, provider: &str) -> anyhow::Result<Option<String>> {
        let state = self.load_state()?;
        let idx = self.find_refresh_profile_index(&state, provider, None);
        Ok(idx.map(|i| state.profiles[i].token.clone()))
    }

    /// Look up a profile by name (regardless of provider kind).
    /// Returns `(provider_kind, token)` if found.
    pub fn token_for_profile(
        &self,
        profile_name: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let state = self.load_state()?;
        let found = state
            .profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(profile_name));
        Ok(found.map(|p| (p.provider.clone(), p.token.clone())))
    }

    /// Resolve credentials from stored auth profiles.
    ///
    /// Resolution order:
    /// 1. If `profile_name` is `Some`, look up that profile by name.
    /// 2. Active profile matching `current_provider`.
    /// 3. Any active profile (provider may differ — caller should update config).
    ///
    /// Returns `None` if no usable credential is found.
    pub fn resolve_credential(
        &self,
        profile_name: Option<&str>,
        current_provider: &str,
    ) -> anyhow::Result<Option<ResolvedCredential>> {
        // 1. Explicit profile by name.
        if let Some(name) = profile_name {
            let (provider, token) = self.token_for_profile(name)?.ok_or_else(|| {
                anyhow!(
                    "auth profile '{name}' not found — run `agentzero auth list` to see available profiles"
                )
            })?;
            anyhow::ensure!(
                !token.trim().is_empty(),
                "auth profile '{name}' has an empty token — re-authenticate with `agentzero auth login`"
            );
            return Ok(Some(ResolvedCredential {
                token,
                provider,
                source: CredentialSource::ExplicitProfile(name.to_string()),
            }));
        }

        // 2. Profile matching current provider.
        if let Some(token) = self.active_token_for_provider(current_provider)? {
            if !token.trim().is_empty() {
                return Ok(Some(ResolvedCredential {
                    token,
                    provider: current_provider.to_string(),
                    source: CredentialSource::ProviderMatch,
                }));
            }
        }

        // 3. Any active profile (may differ from current_provider).
        let status = self.status()?;
        if let Some(ref active_name) = status.active_profile {
            if let Some((provider, token)) = self.token_for_profile(active_name)? {
                if !token.trim().is_empty() {
                    return Ok(Some(ResolvedCredential {
                        token,
                        provider,
                        source: CredentialSource::ActiveProfile(active_name.clone()),
                    }));
                }
            }
        }

        Ok(None)
    }

    pub fn status(&self) -> anyhow::Result<AuthStatus> {
        let state = self.load_state()?;
        let active_profile = state.active_profile.clone();
        let active = active_profile.as_deref().and_then(|name| {
            state
                .profiles
                .iter()
                .find(|profile| profile.name.eq_ignore_ascii_case(name))
        });

        Ok(AuthStatus {
            active_profile,
            active_provider: active.map(|profile| profile.provider.clone()),
            active_token_expires_at_epoch_secs: active
                .and_then(|profile| profile.token_expires_at_epoch_secs),
            active_has_refresh_token: active
                .and_then(|profile| profile.refresh_token.as_deref())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            total_profiles: state.profiles.len(),
        })
    }

    /// Return the health of all profiles (for `auth status` display).
    pub fn token_health(&self) -> anyhow::Result<Vec<ProfileHealth>> {
        let state = self.load_state()?;
        let now = now_epoch_secs();
        Ok(state
            .profiles
            .iter()
            .map(|profile| ProfileHealth {
                name: profile.name.clone(),
                provider: profile.provider.clone(),
                health: assess_token_health(profile.token_expires_at_epoch_secs, now),
                has_refresh_token: profile
                    .refresh_token
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty()),
                expires_at_epoch_secs: profile.token_expires_at_epoch_secs,
            })
            .collect())
    }

    /// Check that the active profile for `provider` has a valid token.
    /// If the token is expired and has a refresh token, attempts a local
    /// refresh (extends expiry). Returns the token if valid/refreshed, or
    /// an error if the token is expired and cannot be refreshed.
    ///
    /// This is designed to be called by the runtime before each provider call.
    pub fn ensure_valid_token(&self, provider: &str) -> anyhow::Result<Option<String>> {
        let result = self.refresh_for_provider(provider, None)?;
        match result {
            None => Ok(None),
            Some(ref r) if r.status == RefreshStatus::Valid => {
                self.active_token_for_provider(provider)
            }
            Some(ref r) if r.status == RefreshStatus::Refreshed => {
                self.active_token_for_provider(provider)
            }
            Some(r) => Err(anyhow!(
                "auth token for profile '{}' has expired and cannot be auto-refreshed — \
                 run `agentzero auth login --provider {}`",
                r.profile,
                provider
            )),
        }
    }

    fn upsert_token(
        &self,
        profile_name: &str,
        provider: &str,
        token: &str,
        refresh_token: Option<&str>,
        expires_in_secs: Option<u64>,
        activate: bool,
    ) -> anyhow::Result<()> {
        if profile_name.trim().is_empty() {
            return Err(anyhow!("profile name must not be empty"));
        }
        if provider.trim().is_empty() {
            return Err(anyhow!("provider must not be empty"));
        }
        if token.trim().is_empty() {
            return Err(anyhow!("token must not be empty"));
        }

        let mut state = self.load_state()?;
        let now = now_epoch_secs();
        let expires = expires_in_secs.map(|ttl| now.saturating_add(ttl.max(1)));
        let refresh = refresh_token.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        if let Some(existing) = state
            .profiles
            .iter_mut()
            .find(|profile| profile.name.eq_ignore_ascii_case(profile_name))
        {
            existing.provider = provider.trim().to_string();
            existing.token = token.trim().to_string();
            if let Some(value) = refresh {
                existing.refresh_token = Some(value);
            }
            if expires.is_some() {
                existing.token_expires_at_epoch_secs = expires;
            }
            existing.updated_at_epoch_secs = now;
        } else {
            state.profiles.push(AuthProfile {
                name: profile_name.trim().to_string(),
                provider: provider.trim().to_string(),
                token: token.trim().to_string(),
                created_at_epoch_secs: now,
                updated_at_epoch_secs: now,
                refresh_token: refresh,
                token_expires_at_epoch_secs: expires,
            });
        }

        if activate {
            state.active_profile = Some(profile_name.trim().to_string());
        }

        self.persist_state(&state)
    }

    fn load_state(&self) -> anyhow::Result<AuthState> {
        self.state_store.load_or_default()
    }

    fn persist_state(&self, state: &AuthState) -> anyhow::Result<()> {
        self.state_store.save(state)
    }

    fn find_refresh_profile_index(
        &self,
        state: &AuthState,
        provider: &str,
        profile_name: Option<&str>,
    ) -> Option<usize> {
        if let Some(profile) = profile_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return state.profiles.iter().position(|candidate| {
                candidate.provider.eq_ignore_ascii_case(provider)
                    && candidate.name.eq_ignore_ascii_case(profile)
            });
        }

        if let Some(active_profile) = state
            .active_profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(idx) = state.profiles.iter().position(|candidate| {
                candidate.provider.eq_ignore_ascii_case(provider)
                    && candidate.name.eq_ignore_ascii_case(active_profile)
            }) {
                return Some(idx);
            }
        }

        if let Some(idx) = state.profiles.iter().position(|candidate| {
            candidate.provider.eq_ignore_ascii_case(provider)
                && candidate.name.eq_ignore_ascii_case("default")
        }) {
            return Some(idx);
        }

        state
            .profiles
            .iter()
            .position(|candidate| candidate.provider.eq_ignore_ascii_case(provider))
    }
}

/// Token health status for display and pre-call validation.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TokenHealth {
    /// Token has no expiry or expiry is more than 5 minutes away.
    Valid,
    /// Token expires within 5 minutes (but is not yet expired).
    ExpiringSoon,
    /// Token has expired.
    Expired,
    /// No token expiry information available (API key flow).
    NoExpiry,
}

impl TokenHealth {
    pub fn label(&self) -> &'static str {
        match self {
            TokenHealth::Valid => "valid",
            TokenHealth::ExpiringSoon => "expiring soon",
            TokenHealth::Expired => "expired",
            TokenHealth::NoExpiry => "no expiry",
        }
    }
}

/// Per-profile health report returned by `token_health`.
#[derive(Debug, Clone)]
pub struct ProfileHealth {
    pub name: String,
    pub provider: String,
    pub health: TokenHealth,
    pub has_refresh_token: bool,
    pub expires_at_epoch_secs: Option<u64>,
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

fn assess_token_health(expires_at: Option<u64>, now: u64) -> TokenHealth {
    match expires_at {
        None => TokenHealth::NoExpiry,
        Some(exp) if exp <= now => TokenHealth::Expired,
        Some(exp) if exp <= now.saturating_add(300) => TokenHealth::ExpiringSoon,
        Some(_) => TokenHealth::Valid,
    }
}

pub fn extract_oauth_code_from_input(redirect_or_code: &str) -> String {
    extract_oauth_code(redirect_or_code)
}

fn extract_oauth_code(redirect_or_code: &str) -> String {
    let raw = redirect_or_code.trim();
    if let Ok(parsed) = Url::parse(raw) {
        if let Some((_, value)) = parsed
            .query_pairs()
            .find(|(key, _)| key.eq_ignore_ascii_case("code"))
        {
            return value.to_string();
        }
    }
    raw.to_string()
}

pub fn extract_oauth_state(redirect_or_code: &str) -> Option<String> {
    let raw = redirect_or_code.trim();
    Url::parse(raw).ok().and_then(|parsed| {
        parsed
            .query_pairs()
            .find(|(key, _)| key.eq_ignore_ascii_case("state"))
            .map(|(_, value)| value.to_string())
    })
}

// ---------------------------------------------------------------------------
// Gemini OAuth helpers
// ---------------------------------------------------------------------------

/// Google Gemini OAuth configuration.
pub struct GeminiOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// Build the Google OAuth2 authorization URL for Gemini API access.
pub fn gemini_authorize_url(config: &GeminiOAuthConfig, state: &str) -> String {
    let scope = "https://www.googleapis.com/auth/generative-language";
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={client_id}&\
         redirect_uri={redirect_uri}&\
         response_type=code&\
         scope={scope}&\
         state={state}&\
         access_type=offline&\
         prompt=consent",
        client_id =
            url::form_urlencoded::byte_serialize(config.client_id.as_bytes()).collect::<String>(),
        redirect_uri = url::form_urlencoded::byte_serialize(config.redirect_uri.as_bytes())
            .collect::<String>(),
        scope = url::form_urlencoded::byte_serialize(scope.as_bytes()).collect::<String>(),
        state = url::form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>(),
    )
}

/// Exchange a Google OAuth authorization code for tokens.
/// Returns `(access_token, refresh_token, expires_in_secs)`.
pub async fn gemini_exchange_code(
    config: &GeminiOAuthConfig,
    code: &str,
) -> anyhow::Result<(String, Option<String>, Option<u64>)> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("redirect_uri", &config.redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Gemini token exchange failed: {body}");
    }

    let json: serde_json::Value = response.json().await?;
    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("missing access_token in Gemini response"))?
        .to_string();
    let refresh_token = json["refresh_token"].as_str().map(|s| s.to_string());
    let expires_in = json["expires_in"].as_u64();

    Ok((access_token, refresh_token, expires_in))
}

/// Refresh a Google OAuth access token using a refresh token.
/// Returns `(new_access_token, expires_in_secs)`.
pub async fn gemini_refresh_token(
    config: &GeminiOAuthConfig,
    refresh_token: &str,
) -> anyhow::Result<(String, Option<u64>)> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Gemini token refresh failed: {body}");
    }

    let json: serde_json::Value = response.json().await?;
    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("missing access_token in refresh response"))?
        .to_string();
    let expires_in = json["expires_in"].as_u64();

    Ok((access_token, expires_in))
}

// ---------------------------------------------------------------------------
// Token storage migration
// ---------------------------------------------------------------------------

const AUTH_STATE_VERSION: u32 = 2;

/// Internal versioned wrapper for auth state persistence.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionedAuthState {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(flatten)]
    state: AuthState,
}

#[allow(dead_code)]
fn default_version() -> u32 {
    1
}

impl AuthManager {
    /// Migrate auth state from v1 to v2 format if needed.
    /// v1 → v2: adds `refresh_token` and `token_expires_at_epoch_secs` fields
    /// to profiles (handled by serde `#[serde(default)]`). The migration
    /// just bumps the version marker.
    pub fn migrate_if_needed(&self) -> anyhow::Result<bool> {
        let raw: Option<serde_json::Value> = self.state_store.load_optional()?;
        let Some(mut value) = raw else {
            return Ok(false);
        };

        let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

        if version >= AUTH_STATE_VERSION {
            return Ok(false);
        }

        // v1 → v2: just stamp the new version. The serde defaults handle
        // missing fields (refresh_token, token_expires_at_epoch_secs).
        value["version"] = serde_json::json!(AUTH_STATE_VERSION);
        self.state_store.save(&value)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_oauth_state, AuthManager, CredentialSource, PendingOAuthLogin, RefreshStatus,
    };
    use std::fs;
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
            "agentzero-auth-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn login_and_status_round_trip_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openrouter", "tok-test", true)
            .expect("login should succeed");

        let status = manager.status().expect("status should be readable");
        assert_eq!(status.active_profile.as_deref(), Some("default"));
        assert_eq!(status.active_provider.as_deref(), Some("openrouter"));
        assert_eq!(status.total_profiles, 1);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn paste_redirect_extracts_code_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .paste_redirect(
                "oauth",
                "openai-codex",
                "https://example.com/callback?code=abc123",
                true,
            )
            .expect("paste redirect should succeed");

        let listed = manager.list_profiles().expect("profiles should load");
        let profile = listed
            .iter()
            .find(|profile| profile.name == "oauth")
            .expect("oauth profile should exist");
        assert_eq!(profile.provider, "openai-codex");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn refresh_updates_expiry_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok-old", true)
            .expect("seed login should succeed");
        manager
            .refresh("default", "tok-new", Some("refresh-1"), Some(3600), true)
            .expect("refresh should succeed");

        let status = manager.status().expect("status should load");
        assert!(status.active_token_expires_at_epoch_secs.is_some());
        assert!(status.active_has_refresh_token);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn login_rejects_empty_token_negative_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        let err = manager
            .login("default", "openrouter", "   ", true)
            .expect_err("empty token should fail");
        assert!(err.to_string().contains("token must not be empty"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn use_profile_fails_when_profile_missing_negative_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        let err = manager
            .use_profile("missing")
            .expect_err("missing profile should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn refresh_fails_when_profile_missing_negative_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        let err = manager
            .refresh("missing", "tok", None, Some(10), true)
            .expect_err("refresh on missing profile should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn refresh_for_provider_uses_default_profile_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok", true)
            .expect("seed login should succeed");

        let result = manager
            .refresh_for_provider("openai-codex", None)
            .expect("refresh should succeed")
            .expect("profile should be found");
        assert_eq!(result.profile, "default");
        assert_eq!(result.status, RefreshStatus::Valid);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn refresh_for_provider_reports_missing_provider_profile_negative_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openrouter", "tok", true)
            .expect("seed login should succeed");

        let result = manager
            .refresh_for_provider("gemini", None)
            .expect("lookup should succeed");
        assert!(result.is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn remove_profile_removes_provider_profile_pair_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok", true)
            .expect("seed login should succeed");
        manager
            .login("backup", "anthropic", "tok2", false)
            .expect("seed second profile should succeed");

        let removed = manager
            .remove_profile("openai-codex", "default")
            .expect("remove should succeed");
        assert!(removed);

        let listed = manager.list_profiles().expect("profiles should load");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].provider, "anthropic");
        assert_eq!(listed[0].name, "backup");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn remove_profile_returns_false_when_missing_negative_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok", true)
            .expect("seed login should succeed");

        let removed = manager
            .remove_profile("gemini", "default")
            .expect("remove should succeed");
        assert!(!removed);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn pending_oauth_round_trip_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        let pending = PendingOAuthLogin {
            provider: "openai-codex".to_string(),
            profile: "default".to_string(),
            code_verifier: "v1".to_string(),
            state: "s1".to_string(),
            created_at_epoch_secs: 1,
            redirect_uri: Some("http://localhost:1455/auth/callback".to_string()),
        };
        manager
            .save_pending_oauth_login(&pending)
            .expect("save pending oauth should succeed");
        let loaded = manager
            .load_pending_oauth_login()
            .expect("load pending oauth should succeed")
            .expect("pending oauth should exist");
        assert_eq!(loaded, pending);
        manager
            .clear_pending_oauth_login()
            .expect("clear pending oauth should succeed");
        assert!(manager
            .load_pending_oauth_login()
            .expect("load after clear should succeed")
            .is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn extract_oauth_state_returns_none_without_state_negative_path() {
        assert_eq!(
            extract_oauth_state("https://example.test/callback?code=abc"),
            None
        );
    }

    #[test]
    fn resolve_credential_explicit_profile_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok-explicit", true)
            .expect("login should succeed");

        let cred = manager
            .resolve_credential(Some("default"), "openrouter")
            .expect("resolve should succeed")
            .expect("credential should be found");
        assert_eq!(cred.token, "tok-explicit");
        assert_eq!(cred.provider, "openai-codex");
        assert_eq!(
            cred.source,
            CredentialSource::ExplicitProfile("default".to_string())
        );

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_credential_provider_match_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openrouter", "tok-match", true)
            .expect("login should succeed");

        let cred = manager
            .resolve_credential(None, "openrouter")
            .expect("resolve should succeed")
            .expect("credential should be found");
        assert_eq!(cred.token, "tok-match");
        assert_eq!(cred.provider, "openrouter");
        assert_eq!(cred.source, CredentialSource::ProviderMatch);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_credential_active_profile_fallback_success_path() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai-codex", "tok-fallback", true)
            .expect("login should succeed");

        // Config says "openrouter" but active profile is "openai-codex".
        let cred = manager
            .resolve_credential(None, "openrouter")
            .expect("resolve should succeed")
            .expect("credential should be found");
        assert_eq!(cred.token, "tok-fallback");
        assert_eq!(cred.provider, "openai-codex");
        assert_eq!(
            cred.source,
            CredentialSource::ActiveProfile("default".to_string())
        );

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_credential_returns_none_when_empty() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");

        let result = manager
            .resolve_credential(None, "openrouter")
            .expect("resolve should succeed");
        assert!(result.is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_credential_explicit_missing_profile_fails() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");

        let err = manager
            .resolve_credential(Some("nonexistent"), "openrouter")
            .expect_err("missing profile should fail");
        assert!(err.to_string().contains("not found"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    // --- Token health tests ---

    #[test]
    fn assess_token_health_valid_when_no_expiry() {
        assert_eq!(
            super::assess_token_health(None, 1000),
            super::TokenHealth::NoExpiry
        );
    }

    #[test]
    fn assess_token_health_valid_when_far_future() {
        assert_eq!(
            super::assess_token_health(Some(2000), 1000),
            super::TokenHealth::Valid
        );
    }

    #[test]
    fn assess_token_health_expiring_soon_within_5_minutes() {
        // 200 seconds from now is within 300 seconds (5 min) threshold.
        assert_eq!(
            super::assess_token_health(Some(1200), 1000),
            super::TokenHealth::ExpiringSoon
        );
    }

    #[test]
    fn assess_token_health_expired_when_past() {
        assert_eq!(
            super::assess_token_health(Some(999), 1000),
            super::TokenHealth::Expired
        );
    }

    #[test]
    fn token_health_returns_health_for_all_profiles() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");

        // Profile with no expiry (API key flow).
        manager
            .login("key-profile", "anthropic", "sk-ant-test", true)
            .expect("login should succeed");

        // Profile with future expiry (OAuth flow).
        manager
            .store_oauth_tokens(
                "oauth-profile",
                "openai-codex",
                "access-tok",
                Some("refresh-tok"),
                Some(7200),
                false,
            )
            .expect("store oauth tokens should succeed");

        let health = manager.token_health().expect("health should succeed");
        assert_eq!(health.len(), 2);

        let key_health = health
            .iter()
            .find(|h| h.name == "key-profile")
            .expect("key profile should be in health");
        assert_eq!(key_health.health, super::TokenHealth::NoExpiry);
        assert!(!key_health.has_refresh_token);

        let oauth_health = health
            .iter()
            .find(|h| h.name == "oauth-profile")
            .expect("oauth profile should be in health");
        assert_eq!(oauth_health.health, super::TokenHealth::Valid);
        assert!(oauth_health.has_refresh_token);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn ensure_valid_token_returns_none_when_no_profile() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");

        let result = manager
            .ensure_valid_token("openrouter")
            .expect("ensure should succeed");
        assert!(result.is_none());

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn ensure_valid_token_returns_token_when_valid() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openrouter", "sk-valid", true)
            .expect("login should succeed");

        let token = manager
            .ensure_valid_token("openrouter")
            .expect("ensure should succeed")
            .expect("token should be returned");
        assert_eq!(token, "sk-valid");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    // --- Gemini OAuth ---

    #[test]
    fn gemini_authorize_url_contains_required_params() {
        let config = super::GeminiOAuthConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost:8080/callback".to_string(),
        };
        let url = super::gemini_authorize_url(&config, "test-state-123");
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("state=test-state-123"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("generative-language"));
    }

    // --- Token storage migration ---

    #[test]
    fn migrate_if_needed_returns_false_on_empty_store() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        let migrated = manager.migrate_if_needed().expect("migrate should succeed");
        assert!(!migrated);
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn migrate_if_needed_returns_false_when_already_current() {
        let dir = temp_dir();
        let manager = AuthManager::in_config_dir(&dir).expect("manager should construct");
        manager
            .login("default", "openai", "tok-1", true)
            .expect("login should succeed");
        // First migration stamps v2.
        let _ = manager.migrate_if_needed();
        // Second call should return false.
        let migrated = manager.migrate_if_needed().expect("migrate should succeed");
        assert!(!migrated);
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
