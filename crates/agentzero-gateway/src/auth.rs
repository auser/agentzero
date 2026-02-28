use crate::state::GatewayState;
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};

pub(crate) fn authorize_request(
    state: &GatewayState,
    headers: &HeaderMap,
    always_require_pairing: bool,
) -> Result<(), StatusCode> {
    let token = parse_bearer(headers);

    if always_require_pairing {
        let Some(token) = token else {
            return Err(StatusCode::UNAUTHORIZED);
        };
        if token_in_state(state, token) {
            return Ok(());
        }
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
        return Err(StatusCode::UNAUTHORIZED);
    };

    if token_in_state(state, token) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn token_in_state(state: &GatewayState, token: &str) -> bool {
    if state
        .bearer_token
        .as_deref()
        .is_some_and(|expected| expected.as_str() == token)
    {
        return true;
    }
    state
        .paired_tokens
        .lock()
        .expect("pairing lock poisoned")
        .contains(token)
}

fn parse_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}
