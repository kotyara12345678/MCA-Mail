//! API authentication: `X-API-Key` verified against the `api_keys` table.
//!
//! The extractor rejects before the handler runs, so a missing, unknown,
//! revoked or expired key can never reach business logic. Role gates are
//! explicit per handler via [`AuthenticatedRequest::require`].

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::api::SharedState;
use crate::persistence::api_key_repo::{self, Role};

/// Identity attached to an authenticated request.
#[derive(Debug, Clone)]
pub struct AuthenticatedRequest {
    pub api_key_prefix: String,
    pub role: String,
}

impl AuthenticatedRequest {
    /// Enforce a minimum role; `viewer < operator < manager < admin`.
    pub fn require(&self, min: Role) -> Result<(), AuthError> {
        let role = self
            .role
            .parse::<Role>()
            .map_err(|_| AuthError::forbidden("key has an unknown role"))?;
        if role.at_least(min) {
            Ok(())
        } else {
            Err(AuthError::forbidden(format!(
                "requires role `{}` or higher",
                min.as_str()
            )))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthError {
    #[serde(skip)]
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl AuthError {
    fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        AuthError {
            status: StatusCode::UNAUTHORIZED,
            code,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        AuthError {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        AuthError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

impl FromRequestParts<SharedState> for AuthenticatedRequest {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let presented = parts
            .headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError::unauthorized("unauthorized", "Missing X-API-Key header"))?;

        let verified = api_key_repo::verify(&state.pool, presented)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "api key verification failed");
                AuthError::internal("credential verification failed")
            })?
            .ok_or_else(|| AuthError::unauthorized("unauthorized", "Invalid API key"))?;

        Ok(AuthenticatedRequest {
            api_key_prefix: api_key_repo::key_prefix(presented),
            role: verified.role.as_str().to_string(),
        })
    }
}
