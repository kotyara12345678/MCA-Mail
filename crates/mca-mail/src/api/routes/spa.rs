//! The admin SPA shell.
//!
//! The three assets are compiled into the binary (`include_str!`), so the
//! deploy needs no static-file directory, no build toolchain on the server,
//! and a missing file is a compile error rather than a runtime 404. Unknown
//! non-API GETs fall back to the shell so a deep link or browser refresh
//! still renders the panel.

use axum::{
    extract::State,
    http::{header, Method, Request, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

use crate::api::SharedState;

const INDEX_HTML: &str = include_str!("../../../static/index.html");
const APP_JS: &str = include_str!("../../../static/app.js");
const STYLE_CSS: &str = include_str!("../../../static/style.css");

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn app_js() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
        .into_response()
}

async fn style_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLE_CSS,
    )
        .into_response()
}

/// Whether this path belongs to the API rather than the SPA. Health, SSE
/// events and OpenAPI keep their real (JSON/404) behaviour.
fn is_api_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path.starts_with("/health")
        || path.starts_with("/events")
        || path.starts_with("/docs")
        || path.starts_with("/openapi")
}

/// SPA fallback: serve the shell for unknown page routes, JSON 404
/// otherwise (so a typo'd API endpoint never returns HTML).
pub async fn fallback(
    State(_state): State<SharedState>,
    request: Request<axum::body::Body>,
) -> Response {
    let path = request.uri().path().to_string();
    if request.method() == Method::GET && !is_api_path(&path) {
        return index().await.into_response();
    }
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"code":"not_found","message":"Not found"}"#,
    )
        .into_response()
}

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/static/app.js", get(app_js))
        .route("/static/style.css", get(style_css))
}
