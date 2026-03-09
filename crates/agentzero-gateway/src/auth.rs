use crate::state::GatewayState;
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use subtle::ConstantTimeEq;

pub(crate) fn authorize_request(
    state: &GatewayState,
    headers: &HeaderMap,
    always_require_pairing: bool,
) -> Result<(), StatusCode> {
    let token = parse_bearer(headers);

    if always_require_pairing {
        let Some(token) = token else {
            tracing::warn!("auth denied: no bearer token (pairing required)");
            return Err(StatusCode::UNAUTHORIZED);
        };
        if token_in_state(state, token) {
            return Ok(());
        }
        tracing::warn!("auth denied: invalid token (pairing required)");
        return Err(StatusCode::UNAUTHORIZED);
    }

    if state.bearer_token.is_none()
        && state
            .paired_tokens
            .lock()
            .expect("pairing lock poisoned")
            .is_empty()
    {
        return Ok(());
    }

    let Some(token) = token else {
        tracing::warn!("auth denied: no bearer token");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if token_in_state(state, token) {
        Ok(())
    } else {
        tracing::warn!("auth denied: invalid token");
        Err(StatusCode::UNAUTHORIZED)
    }
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
    if state
        .bearer_token
        .as_deref()
        .is_some_and(|expected| ct_eq(expected.as_str(), token))
    {
        return true;
    }
    // For paired tokens, we must iterate and compare each one in constant time.
    let paired = state.paired_tokens.lock().expect("pairing lock poisoned");
    paired.iter().any(|stored| ct_eq(stored, token))
}

fn parse_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GatewayState;

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
        assert!(authorize_request(&state, &empty_headers(), false).is_ok());
    }

    // --- authorize_request: bearer token ---

    #[test]
    fn correct_bearer_token_succeeds() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        let headers = bearer_headers("secret-tok");
        assert!(authorize_request(&state, &headers, false).is_ok());
    }

    #[test]
    fn wrong_bearer_token_returns_401() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        let headers = bearer_headers("wrong-tok");
        assert_eq!(
            authorize_request(&state, &headers, false),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn no_header_with_bearer_configured_returns_401() {
        let state = GatewayState::test_with_bearer(Some("secret-tok"));
        assert_eq!(
            authorize_request(&state, &empty_headers(), false),
            Err(StatusCode::UNAUTHORIZED)
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
            authorize_request(&state, &headers, false),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    // --- authorize_request: always_require_pairing ---

    #[test]
    fn pairing_required_with_no_token_returns_401() {
        let state = GatewayState::test_with_bearer(None);
        assert_eq!(
            authorize_request(&state, &empty_headers(), true),
            Err(StatusCode::UNAUTHORIZED)
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
            authorize_request(&state, &headers, true),
            Err(StatusCode::UNAUTHORIZED)
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
}
