use super::*;
use crate::models::{PairRequest, PairResponse, PingRequest, PingResponse};
use crate::util::generate_session_token;

pub(crate) async fn pair(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    _body: Option<Json<PairRequest>>,
) -> Result<Json<PairResponse>, GatewayError> {
    let Some(expected_code) = state.pairing_code_valid() else {
        return Err(GatewayError::AuthRequired);
    };

    let Some(code_header) = headers.get("X-Pairing-Code") else {
        return Err(GatewayError::AuthRequired);
    };
    let Ok(code) = code_header.to_str() else {
        return Err(GatewayError::AuthRequired);
    };
    if code.trim() != expected_code {
        crate::audit::audit(
            crate::audit::AuditEvent::PairFailure,
            "invalid pairing code",
            "",
            "/pair",
        );
        return Err(GatewayError::AuthFailed);
    }

    let token = generate_session_token();
    if state.add_paired_token(token.clone()).is_err() {
        return Err(GatewayError::AgentExecutionFailed {
            message: "failed to persist pairing token".to_string(),
        });
    }

    crate::audit::audit(
        crate::audit::AuditEvent::PairSuccess,
        "pairing code exchanged for token",
        "",
        "/pair",
    );

    Ok(Json(PairResponse {
        paired: true,
        token,
    }))
}

pub(crate) async fn ping(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    AppJson(req): AppJson<PingRequest>,
) -> Result<Json<PingResponse>, GatewayError> {
    authorize_with_scope(&state, &headers, false, &Scope::RunsWrite)?;

    Ok(Json(PingResponse {
        ok: true,
        echo: req.message,
    }))
}
