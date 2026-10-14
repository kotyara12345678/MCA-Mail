//! Admin panel login/logout/session — one login/password, cookie bearer.
//!
//! The cookie is `HttpOnly` (JS never sees it), `SameSite=Lax` (no CSRF from
//! cross-site POSTs), and `Secure` whenever the request arrives over HTTPS —
//! detected via `X-Forwarded-Proto`, which Caddy sets.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::api::auth::{credentials_match, session_cookie, AuthenticatedRequest};
use crate::api::SharedState;
use crate::persistence::{api_key_repo, session_repo};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

/// What `GET /api/auth/session` tells the SPA about its current state.
/// Never 401s: an unauthenticated visitor gets `{authenticated:false}` and
/// shows the login form.
#[derive(Debug, Serialize)]
pub struct SessionView {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

fn unauthorized(body: &'static str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"code": "unauthorized", "message": body})),
    )
}

fn build_cookie(raw: &str, secure: bool) -> String {
    let mut value = format!(
        "{}={raw}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        session_repo::COOKIE_NAME,
        session_repo::cookie_max_age_seconds()
    );
    if secure {
        value.push_str("; Secure");
    }
    value
}

fn request_is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}

async fn login(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<SessionView>), (StatusCode, Json<serde_json::Value>)> {
    let Some(creds) = &state.admin else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "code": "not_configured",
                "message": "Admin login is not configured (set ADMIN_LOGIN / ADMIN_PASSWORD)"
            })),
        ));
    };
    let login_id = req.login.trim();
    if login_id.is_empty()
        || !credentials_match(&creds.login, login_id)
        || !credentials_match(&creds.password, &req.password)
    {
        return Err(unauthorized("Неверный логин или пароль"));
    }

    let raw = session_repo::create(&state.pool, login_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "session create failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "code": "internal_error",
                    "message": "Could not create a session"
                })),
            )
        })?;

    let mut set_cookie = HeaderMap::new();
    set_cookie.insert(
        header::SET_COOKIE,
        build_cookie(&raw, request_is_https(&headers))
            .parse()
            .expect("cookie value is valid header bytes"),
    );
    tracing::info!(login = %login_id, "admin panel login");
    Ok((
        set_cookie,
        Json(SessionView {
            authenticated: true,
            login: Some(login_id.to_string()),
            role: Some("admin".into()),
        }),
    ))
}

/// Idempotent: logging out without a cookie is a no-op success.
async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> Json<SessionView> {
    if let Some(token) = session_cookie(&headers) {
        if let Err(e) = session_repo::revoke(&state.pool, &token).await {
            tracing::warn!(error = %e, "session revoke failed");
        }
    }
    Json(SessionView {
        authenticated: false,
        login: None,
        role: None,
    })
}

/// SPA bootstrap: who am I? Accepts either credential kind so a tab opened
/// with a raw API key still renders the panel.
async fn current_session(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Json<SessionView> {
    if let Some(token) = session_cookie(&headers) {
        if let Ok(Some(info)) = session_repo::verify(&state.pool, &token).await {
            return Json(SessionView {
                authenticated: true,
                login: Some(info.login),
                role: Some("admin".into()),
            });
        }
    }
    if let Some(key) = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Ok(Some(verified)) = api_key_repo::verify(&state.pool, key).await {
            return Json(SessionView {
                authenticated: true,
                login: Some(verified.id.to_string()),
                role: Some(verified.role.as_str().to_string()),
            });
        }
    }
    Json(SessionView {
        authenticated: false,
        login: None,
        role: None,
    })
}

/// Convenience re-export so the SPA module can reuse the auth extractor
/// without reaching into `auth` directly.
pub type PanelAuth = AuthenticatedRequest;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/session", get(current_session))
}
