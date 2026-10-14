//! API authentication: `X-API-Key` header or the admin-panel session cookie.
//!
//! The extractor rejects before the handler runs, so a missing, unknown,
//! revoked or expired credential can never reach business logic. Role gates
//! are explicit per handler via [`AuthenticatedRequest::require`]. Sessions
//! carry the `admin` role — the panel is a single-operator surface.

use axum::{
    extract::FromRequestParts,
    http::{header::COOKIE, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::api::SharedState;
use crate::error::AppError;
use crate::persistence::{
    api_key_repo::{self, Role},
    session_repo,
};

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
    pub(crate) fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
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

impl From<AuthError> for AppError {
    fn from(err: AuthError) -> Self {
        match err.status {
            StatusCode::UNAUTHORIZED => AppError::Unauthorized(err.message),
            StatusCode::FORBIDDEN => AppError::Forbidden(err.message),
            StatusCode::INTERNAL_SERVER_ERROR => AppError::Internal(err.message),
            _ => AppError::Forbidden(err.message),
        }
    }
}

/// The `X-API-Key` header value, if the request carries one.
fn header_api_key(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The raw session token from the panel cookie, if present.
pub(crate) fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == session_repo::COOKIE_NAME)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn verify_api_key(
    state: &SharedState,
    presented: &str,
) -> Result<AuthenticatedRequest, AuthError> {
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

async fn verify_session(
    state: &SharedState,
    token: &str,
) -> Result<AuthenticatedRequest, AuthError> {
    let session = session_repo::verify(&state.pool, token)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "session verification failed");
            AuthError::internal("session verification failed")
        })?
        .ok_or_else(|| AuthError::unauthorized("unauthorized", "Invalid or expired session"))?;
    Ok(AuthenticatedRequest {
        api_key_prefix: format!("session:{}", session.login),
        role: Role::Admin.as_str().to_string(),
    })
}

impl FromRequestParts<SharedState> for AuthenticatedRequest {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        // An explicit header always wins: scripts and the CLI send keys, the
        // browser sends only the cookie.
        if let Some(key) = header_api_key(parts) {
            return verify_api_key(state, key).await;
        }
        if let Some(token) = session_cookie(&parts.headers) {
            return verify_session(state, &token).await;
        }
        Err(AuthError::unauthorized(
            "unauthorized",
            "Missing credentials (X-API-Key header or session cookie)",
        ))
    }
}

/// Constant-time-ish string comparison for the panel password. Both operands
/// are short; the loop length follows the presented value so no early exit
/// leaks the first differing byte.
pub(crate) fn credentials_match(expected: &str, presented: &str) -> bool {
    let (a, b) = (expected.as_bytes(), presented.as_bytes());
    if a.len() != b.len() {
        // Still walk the presented value so timing does not distinguish
        // "wrong length" from "wrong content".
        let mut diff = a.len() ^ b.len();
        for byte in b {
            diff |= usize::from(*byte);
        }
        return diff == 0;
    }
    let mut diff = 0usize;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= usize::from(x ^ y);
    }
    diff == 0
}
