//! Noise Protocol transport middleware for the gateway.
//!
//! Detects the `X-Noise-Session` header on incoming requests, decrypts
//! the request body using the associated session, passes the plaintext
//! to downstream handlers, then encrypts the response body.
//!
//! Requests without the header pass through unmodified (backwards compatible).

use crate::privacy_state::NoiseSessionStore;
use axum::{
    body::Body,
    extract::Request,
    http::{header::HeaderName, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// Custom header for Noise session identification.
static NOISE_SESSION_HEADER: HeaderName = HeaderName::from_static("x-noise-session");

/// Middleware that transparently decrypts/encrypts Noise-protected requests.
pub(crate) async fn noise_transport_middleware(
    req: Request<Body>,
    next: Next,
    sessions: Arc<NoiseSessionStore>,
) -> Response {
    // Check for the Noise session header.
    let session_id_hex = match req.headers().get(&NOISE_SESSION_HEADER) {
        Some(v) => match v.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid X-Noise-Session header").into_response()
            }
        },
        None => {
            // No Noise session — pass through unmodified.
            return next.run(req).await;
        }
    };

    // Parse the hex session ID.
    let session_id = match parse_hex_session_id(&session_id_hex) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "invalid session ID format").into_response(),
    };

    // Read the encrypted request body.
    let (parts, body) = req.into_parts();
    let encrypted_body = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "failed to read request body").into_response(),
    };

    // Verify session exists before processing.
    let session_exists = sessions.with_session(&session_id, |_| ()).is_some();
    if !session_exists {
        return (
            StatusCode::UNAUTHORIZED,
            "noise session not found or expired",
        )
            .into_response();
    }

    // If body is empty (e.g. GET requests), pass through without decryption
    // but still encrypt the response below.
    let req = if encrypted_body.is_empty() {
        Request::from_parts(parts, Body::from(encrypted_body))
    } else {
        // Decrypt the request body.
        let decrypt_start = std::time::Instant::now();
        let plaintext = match sessions
            .with_session(&session_id, |session| session.decrypt(&encrypted_body))
        {
            Some(Ok(pt)) => {
                crate::gateway_metrics::record_encrypt_duration(
                    decrypt_start.elapsed().as_secs_f64(),
                );
                pt
            }
            Some(Err(_)) => return (StatusCode::BAD_REQUEST, "decryption failed").into_response(),
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    "noise session not found or expired",
                )
                    .into_response()
            }
        };
        Request::from_parts(parts, Body::from(plaintext))
    };

    // Run the downstream handler.
    let response = next.run(req).await;

    // Encrypt the response body.
    let (resp_parts, resp_body) = response.into_parts();
    let response_bytes = match axum::body::to_bytes(resp_body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to read response body",
            )
                .into_response()
        }
    };

    if response_bytes.is_empty() {
        return Response::from_parts(resp_parts, Body::from(response_bytes));
    }

    let encrypt_start = std::time::Instant::now();
    match sessions.with_session(&session_id, |session| session.encrypt(&response_bytes)) {
        Some(Ok(ciphertext)) => {
            crate::gateway_metrics::record_encrypt_duration(encrypt_start.elapsed().as_secs_f64());
            Response::from_parts(resp_parts, Body::from(ciphertext))
        }
        Some(Err(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "response encryption failed",
        )
            .into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "session lost during response",
        )
            .into_response(),
    }
}

/// Parse a 64-character hex string into a 32-byte session ID.
fn parse_hex_session_id(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut id = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        id[i] = (hi << 4) | lo;
    }
    Some(id)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_hex_session_id() {
        let hex = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let result = parse_hex_session_id(hex);
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0], 0xa1);
        assert_eq!(result.unwrap()[1], 0xb2);
    }

    #[test]
    fn parse_rejects_short_hex() {
        assert!(parse_hex_session_id("abcd").is_none());
    }

    #[test]
    fn parse_rejects_invalid_chars() {
        let hex = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(parse_hex_session_id(hex).is_none());
    }
}
