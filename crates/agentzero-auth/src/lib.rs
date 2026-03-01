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

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
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

#[cfg(test)]
mod tests {
    use super::{extract_oauth_state, AuthManager, PendingOAuthLogin, RefreshStatus};
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
        let dir = std::env::temp_dir().join(format!("agentzero-auth-{nanos}-{seq}"));
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
}
