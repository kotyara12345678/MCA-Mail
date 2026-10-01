//! API authentication middleware.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Simple API key auth from the X-API-Key header.
#[derive(Debug, Clone)]
pub struct AuthenticatedRequest {
    pub api_key_prefix: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthError {
    code: &'static str,
    message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, Json(self)).into_response()
    }
}

// Placeholder extractor — will be fleshed out with real auth
impl<S> FromRequestParts<S> for AuthenticatedRequest
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        match key {
            Some(_key) => Ok(AuthenticatedRequest {
                api_key_prefix: "mca_".into(),
                role: "admin".into(),
            }),
            None => Err(AuthError {
                code: "unauthorized",
                message: "Missing X-API-Key header".into(),
            }),
        }
    }
}
