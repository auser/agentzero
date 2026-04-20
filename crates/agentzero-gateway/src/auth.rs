use crate::api_keys::{ApiKeyInfo, Scope};
use crate::state::GatewayState;
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use secrecy::ExposeSecret;
use std::collections::HashSet;
use subtle::ConstantTimeEq;

/// Identity resolved from authentication. Bearer/paired tokens get all scopes
/// (they are the legacy auth path). API keys carry their specific scopes.
#[derive(Debug, Clone)]
pub(crate) struct AuthIdentity {
    pub(crate) scopes: HashSet<Scope>,
    /// API key identity info, if authenticated via API key.
    pub(crate) api_key: Option<ApiKeyInfo>,
    /// Capability ceiling for this request (Sprint 89 — Phase I).
    ///
    /// Derived from `ApiKeyInfo.capability_ceiling` when authenticated via API key.
    /// Empty for bearer/paired-token authentication (no restriction).
    pub(crate) capability_ceiling: agentzero_core::security::CapabilitySet,
}

impl AuthIdentity {
    /// An identity with all scopes granted (bearer/paired token auth).
    fn full_access() -> Self {
        Self {
            scopes: [
                Scope::RunsRead,
                Scope::RunsWrite,
                Scope::RunsManage,
                Scope::Admin,
            ]
            .into(),
            api_key: None,
            capability_ceiling: agentzero_core::security::CapabilitySet::default(),
        }
    }

    /// Check if the identity has the required scope.
    pub(crate) fn has_scope(&self, scope: &Scope) -> bool {
        self.scopes.contains(scope)
    }
}

/// Authenticate a request. Returns `AuthIdentity` on success with the resolved scopes.
///
/// Authentication priority:
/// 1. Bearer token matching `AGENTZERO_GATEWAY_BEARER_TOKEN` → full access
/// 2. Paired session token → full access
/// 3. API key (via `ApiKeyStore`) → scoped access
/// 4. Open mode (no auth configured, no API key store) → full access
pub(crate) fn authorize_request(
    state: &GatewayState,
    headers: &HeaderMap,
    always_require_pairing: bool,
) -> Result<AuthIdentity, StatusCode> {
    let token = parse_bearer(headers);
    let path = ""; // caller context not available here; audit path is set at handler level

    if always_require_pairing {
        let Some(token) = token else {
            crate::audit::audit(
                crate::audit::AuditEvent::AuthFailure,
                "no bearer token (pairing required)",
                "",
                path,
            );
            return Err(StatusCode::UNAUTHORIZED);
        };
        if token_in_state(state, token) {
            return Ok(AuthIdentity::full_access());
        }
        // Try API key as fallback.
        if let Some(info) = try_api_key(state, token) {
            return Ok(AuthIdentity {
                capability_ceiling: info.capability_ceiling.clone(),
                scopes: info.scopes.clone(),
                api_key: Some(info),
            });
        }
        crate::audit::audit(
            crate::audit::AuditEvent::AuthFailure,
            "invalid token (pairing required)",
            "",
            path,
        );
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Open mode: no bearer, no paired tokens, and no API key store.
    let has_bearer = state.bearer_token.is_some();
    let has_paired = !state
        .paired_tokens
        .lock()
        .expect("pairing lock poisoned")
        .is_empty();
    let has_api_keys = state.api_key_store.is_some();

    if !has_bearer && !has_paired && !has_api_keys {
        return Ok(AuthIdentity::full_access());
    }

    let Some(token) = token else {
        crate::audit::audit(
            crate::audit::AuditEvent::AuthFailure,
            "no bearer token",
            "",
            path,
        );
        return Err(StatusCode::UNAUTHORIZED);
    };

    // Check bearer/paired tokens first (full access).
    if token_in_state(state, token) {
        return Ok(AuthIdentity::full_access());
    }

    // Check API key store.
    if let Some(info) = try_api_key(state, token) {
        return Ok(AuthIdentity {
            capability_ceiling: info.capability_ceiling.clone(),
            scopes: info.scopes.clone(),
            api_key: Some(info),
        });
    }

    crate::audit::audit(
        crate::audit::AuditEvent::AuthFailure,
        "invalid token",
        "",
        path,
    );
    Err(StatusCode::UNAUTHORIZED)
}

/// Authorize a request and verify it has the required scope.
/// Returns 403 with `InsufficientScope` if authenticated but missing the scope.
pub(crate) fn authorize_with_scope(
    state: &GatewayState,
    headers: &HeaderMap,
    always_require_pairing: bool,
    required_scope: &Scope,
) -> Result<AuthIdentity, crate::models::GatewayError> {
    let identity = authorize_request(state, headers, always_require_pairing)
        .map_err(crate::models::GatewayError::from)?;
    if !identity.has_scope(required_scope) {
        let key_id = identity
            .api_key
            .as_ref()
            .map(|k| k.key_id.as_str())
            .unwrap_or("");
        crate::audit::audit(
            crate::audit::AuditEvent::ScopeDenied,
            &format!("missing scope: {}", required_scope.as_str()),
            key_id,
            "",
        );
        return Err(crate::models::GatewayError::InsufficientScope {
            scope: required_scope.as_str().to_string(),
        });
    }
    Ok(identity)
}

/// Try to validate a token as an API key. Returns `Some(ApiKeyInfo)` if valid.
fn try_api_key(state: &GatewayState, token: &str) -> Option<ApiKeyInfo> {
    state
        .api_key_store
        .as_ref()
        .and_then(|store| store.validate(token))
}

/// Constant-time token comparison to prevent timing side-channel attacks.
fn ct_eq(a: &str, b: &str) -> bool {
    // ConstantTimeEq requires equal-length slices; pad the shorter one to
    // avoid leaking length information through early-exit timing.
    if a.len() != b.len() {
        // Still run a constant-time comparison on dummy data to avoid
        // leaking that lengths differ through timing.
        let dummy = vec![0u8; a.len()];
        let _ = dummy.ct_eq(a.as_bytes());
        return false;
    }
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn token_in_state(state: &GatewayState, token: &str) -> bool {
    // Bearer tokens (env var) never expire.
    if state
        .bearer_token
        .as_ref()
        .is_some_and(|expected| ct_eq(expected.expose_secret(), token))
    {
        return true;
    }
    // For paired tokens, we must iterate and compare each one in constant time.
    // If session_ttl_secs is set, also check the token's creation timestamp.
    let paired = state.paired_tokens.lock().expect("pairing lock poisoned");
    let timestamps = state
        .paired_token_timestamps
        .lock()
        .expect("token timestamp lock poisoned");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    paired.iter().any(|stored| {
        if !ct_eq(stored, token) {
            return false;
        }
        // Check session TTL if configured.
        if let Some(ttl) = state.session_ttl_secs {
            if let Some(&created_at) = timestamps.get(stored) {
                if now.saturating_sub(created_at) >= ttl {
                    tracing::debug!("paired token expired (session TTL exceeded)");
                    return false;
                }
            }
            // Tokens without timestamps (legacy) are treated as valid.
        }
        true
    })
}

fn parse_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_keys::ApiKeyStore;
    use crate::state::GatewayState;
    use std::sync::Arc;

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        headers
    }

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    // --- parse_bearer ---

    #[test]
    fn parse_bearer_extracts_token() {
        let headers = bearer_headers("tok-123");
        assert_eq!(parse_bearer(&headers), Some("tok-123"));
    }

    #[test]
    fn parse_bearer_returns_none_for_missing_header() {
        assert_eq!(parse_bearer(&empty_headers()), None);
    }

    #[test]
    fn parse_bearer_returns_none_for_non_bearer_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Basic dXNlcjpwYXNz".parse().unwrap());
        assert_eq!(parse_bearer(&headers), None);
    }

    #[test]
    fn parse_bearer_returns_empty_string_for_empty_token() {
        let headers = bearer_headers("");
        assert_eq!(parse_bearer(&headers), Some(""));
    }

    // --- authorize_request: open mode (no secrets) ---

    #[test]
    fn open_mode_allows_any_request() {
        // No bearer token, no paired tokens → open mode.
        let state = GatewayState::test_with_bearer(None);
        // Clear paired tokens (test helper sets a pairing code but no paired tokens).
        state.paired_tokens.lock().unwrap().clear();
        let identity = authorize_request(&state, &empty_headers(), false).unwrap();
        assert!(identity.has_scope(&Scope::Admin));
    }

    // --- authorize_request: bearer token ---

    #[test]
    fn correct_bearer_token_succeeds() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        let headers = bearer_headers("secret-tok");
        let identity = authorize_request(&state, &headers, false).unwrap();
        assert!(identity.has_scope(&Scope::RunsRead));
        assert!(identity.has_scope(&Scope::Admin));
        assert!(identity.api_key.is_none());
    }

    #[test]
    fn wrong_bearer_token_returns_401() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        let headers = bearer_headers("wrong-tok");
        assert_eq!(
            authorize_request(&state, &headers, false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn no_header_with_bearer_configured_returns_401() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        assert_eq!(
            authorize_request(&state, &empty_headers(), false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    // --- authorize_request: paired tokens ---

    #[test]
    fn correct_paired_token_succeeds() {
        let state = GatewayState::test_with_existing_pair("paired-tok");
        let headers = bearer_headers("paired-tok");
        assert!(authorize_request(&state, &headers, false).is_ok());
    }

    #[test]
    fn wrong_paired_token_returns_401() {
        let state = GatewayState::test_with_existing_pair("paired-tok");
        let headers = bearer_headers("wrong-tok");
        assert_eq!(
            authorize_request(&state, &headers, false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    // --- authorize_request: always_require_pairing ---

    #[test]
    fn pairing_required_with_no_token_returns_401() {
        let state = GatewayState::test_with_bearer(None);
        assert_eq!(
            authorize_request(&state, &empty_headers(), true).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn pairing_required_with_correct_bearer_succeeds() {
        let state = GatewayState::test_with_bearer(Some("secret"));
        let headers = bearer_headers("secret");
        assert!(authorize_request(&state, &headers, true).is_ok());
    }

    #[test]
    fn pairing_required_with_wrong_token_returns_401() {
        let state = GatewayState::test_with_bearer(Some("secret"));
        let headers = bearer_headers("wrong");
        assert_eq!(
            authorize_request(&state, &headers, true).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn pairing_required_with_paired_token_succeeds() {
        let state = GatewayState::test_with_existing_pair("paired-tok");
        let headers = bearer_headers("paired-tok");
        assert!(authorize_request(&state, &headers, true).is_ok());
    }

    // --- token_in_state ---

    #[test]
    fn token_in_state_matches_bearer() {
        let state = GatewayState::test_with_bearer(Some("bearer-tok"));
        assert!(token_in_state(&state, "bearer-tok"));
    }

    #[test]
    fn token_in_state_matches_paired() {
        let state = GatewayState::test_with_existing_pair("paired-tok");
        assert!(token_in_state(&state, "paired-tok"));
    }

    #[test]
    fn token_in_state_rejects_unknown() {
        let state = GatewayState::test_with_bearer(Some("bearer-tok"));
        assert!(!token_in_state(&state, "unknown-tok"));
    }

    // --- constant-time comparison ---

    #[test]
    fn ct_eq_equal_strings() {
        assert!(ct_eq("hello", "hello"));
    }

    #[test]
    fn ct_eq_different_strings_same_length() {
        assert!(!ct_eq("hello", "world"));
    }

    #[test]
    fn ct_eq_different_lengths() {
        assert!(!ct_eq("short", "longer-string"));
    }

    #[test]
    fn ct_eq_empty_strings() {
        assert!(ct_eq("", ""));
    }

    // --- API key authentication ---

    #[test]
    fn api_key_auth_returns_scoped_identity() {
        let store = Arc::new(ApiKeyStore::new());
        let (raw_key, _) = store
            .create("org-1", "user-1", [Scope::RunsRead].into(), None)
            .unwrap();

        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(store);

        let headers = bearer_headers(&raw_key);
        let identity = authorize_request(&state, &headers, false).unwrap();
        assert!(identity.has_scope(&Scope::RunsRead));
        assert!(!identity.has_scope(&Scope::RunsWrite));
        assert!(!identity.has_scope(&Scope::Admin));
        assert!(identity.api_key.is_some());
        assert_eq!(identity.api_key.as_ref().unwrap().org_id, "org-1");
    }

    #[test]
    fn api_key_insufficient_scope_detected() {
        let store = Arc::new(ApiKeyStore::new());
        let (raw_key, _) = store
            .create("org-1", "user-1", [Scope::RunsRead].into(), None)
            .unwrap();

        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(store);

        let headers = bearer_headers(&raw_key);
        let identity = authorize_request(&state, &headers, false).unwrap();
        // Auth succeeds but scope check should fail for Admin.
        assert!(!identity.has_scope(&Scope::Admin));
    }

    #[test]
    fn bearer_takes_priority_over_api_key() {
        let store = Arc::new(ApiKeyStore::new());
        // Create an API key with limited scopes.
        store
            .create("org-1", "user-1", [Scope::RunsRead].into(), None)
            .unwrap();

        let mut state = GatewayState::test_with_bearer(Some("bearer-tok"));
        state.api_key_store = Some(store);

        // Authenticate with bearer token → full access.
        let headers = bearer_headers("bearer-tok");
        let identity = authorize_request(&state, &headers, false).unwrap();
        assert!(identity.has_scope(&Scope::Admin));
        assert!(identity.api_key.is_none());
    }

    #[test]
    fn api_key_with_pairing_required() {
        let store = Arc::new(ApiKeyStore::new());
        let (raw_key, _) = store
            .create(
                "org-1",
                "user-1",
                [Scope::RunsRead, Scope::RunsWrite].into(),
                None,
            )
            .unwrap();

        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(store);

        let headers = bearer_headers(&raw_key);
        let identity = authorize_request(&state, &headers, true).unwrap();
        assert!(identity.has_scope(&Scope::RunsRead));
        assert!(identity.api_key.is_some());
    }

    #[test]
    fn invalid_api_key_returns_401() {
        let store = Arc::new(ApiKeyStore::new());
        // Don't create any keys.
        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(store);

        let headers = bearer_headers("az_invalid_key");
        assert_eq!(
            authorize_request(&state, &headers, false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn api_key_store_present_blocks_open_mode() {
        // When an API key store is configured, open mode is disabled.
        let store = Arc::new(ApiKeyStore::new());
        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(store);

        // No token → 401 (not open mode).
        assert_eq!(
            authorize_request(&state, &empty_headers(), false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    // --- Session TTL for paired tokens ---

    #[test]
    fn paired_token_with_no_ttl_never_expires() {
        let mut state = GatewayState::test_with_existing_pair("session-tok");
        state.session_ttl_secs = None; // No TTL
        let headers = bearer_headers("session-tok");
        assert!(authorize_request(&state, &headers, false).is_ok());
    }

    #[test]
    fn paired_token_within_ttl_succeeds() {
        let mut state = GatewayState::test_with_existing_pair("session-tok");
        state.session_ttl_secs = Some(3600); // 1 hour

        // Record a recent timestamp.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        state
            .paired_token_timestamps
            .lock()
            .unwrap()
            .insert("session-tok".to_string(), now);

        let headers = bearer_headers("session-tok");
        assert!(authorize_request(&state, &headers, false).is_ok());
    }

    #[test]
    fn paired_token_past_ttl_returns_401() {
        let mut state = GatewayState::test_with_existing_pair("expired-tok");
        state.session_ttl_secs = Some(3600); // 1 hour

        // Record a timestamp from 2 hours ago.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        state
            .paired_token_timestamps
            .lock()
            .unwrap()
            .insert("expired-tok".to_string(), now - 7200);

        let headers = bearer_headers("expired-tok");
        assert_eq!(
            authorize_request(&state, &headers, false).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn legacy_paired_token_without_timestamp_still_valid() {
        // Tokens from before session TTL was introduced have no timestamp.
        // They should still be valid (backward compatible).
        let mut state = GatewayState::test_with_existing_pair("legacy-tok");
        state.session_ttl_secs = Some(3600);
        // No timestamp recorded for this token.

        let headers = bearer_headers("legacy-tok");
        assert!(authorize_request(&state, &headers, false).is_ok());
    }
    #[test]
    fn full_access_identity_has_empty_ceiling() {
        // Bearer/paired token identity should have an unrestricted (empty) ceiling.
        // Empty CapabilitySet means "no restriction" by convention.
        let state = GatewayState::test_with_bearer(Some("secret"));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer secret".parse().unwrap(),
        );
        let identity = authorize_request(&state, &headers, false).expect("should auth");
        assert!(
            identity.capability_ceiling.is_empty(),
            "bearer token identity should have empty (unrestricted) capability ceiling"
        );
    }

    #[test]
    fn api_key_identity_carries_capability_ceiling() {
        use crate::api_keys::{ApiKeyStore, Scope};
        use agentzero_core::security::capability::Capability;
        use std::{collections::HashSet, sync::Arc};

        let store = ApiKeyStore::new();
        let scopes: HashSet<Scope> = [Scope::RunsWrite].into();
        let ceiling = vec![Capability::Tool {
            name: "web_search".to_string(),
        }];
        let (raw_key, _) = store
            .create_with_ceiling("org-1", "u-1", scopes, None, ceiling)
            .unwrap();

        let mut state = GatewayState::test_with_bearer(None);
        state.paired_tokens.lock().unwrap().clear();
        state.api_key_store = Some(Arc::new(store));

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {raw_key}").parse().unwrap(),
        );
        let identity = authorize_request(&state, &headers, false).expect("should auth");
        assert!(
            !identity.capability_ceiling.is_empty(),
            "api key identity should carry the capability ceiling"
        );
        assert!(identity.capability_ceiling.allows_tool("web_search"));
        assert!(!identity.capability_ceiling.allows_tool("shell"));
    }
}
