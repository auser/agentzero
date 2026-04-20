//! Custom axum extractors for the gateway.
//!
//! Provides [`AppJson`], a drop-in replacement for [`axum::Json`] that
//! normalises deserialisation errors into the structured [`GatewayError`]
//! format instead of axum's default plain-text 422 response.

use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRequest, Request},
    Json,
};
use serde::de::DeserializeOwned;

use crate::models::GatewayError;

/// A JSON extractor that maps deserialisation failures into [`GatewayError::BadRequest`].
///
/// Usage is identical to [`axum::Json`]:
///
/// ```ignore
/// async fn handler(AppJson(body): AppJson<MyRequest>) -> Result<..., GatewayError> { ... }
/// ```
pub(crate) struct AppJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = GatewayError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err(json_rejection_to_gateway_error(rejection)),
        }
    }
}

fn json_rejection_to_gateway_error(rejection: JsonRejection) -> GatewayError {
    GatewayError::BadRequest {
        message: format!("invalid request body: {rejection}"),
    }
}
