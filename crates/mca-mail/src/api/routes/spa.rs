//! The admin SPA shell.
//!
//! The assets are compiled into the binary (`include_str!`), so the deploy
//! needs no static-file directory, no build toolchain on the server, and a
//! missing file is a compile error rather than a runtime 404. The panel is
//! plain ES modules — the browser imports them directly, no bundler. Unknown
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
const API_JS: &str = include_str!("../../../static/js/api.js");
const UI_JS: &str = include_str!("../../../static/js/ui.js");
const DIALOGS_JS: &str = include_str!("../../../static/js/views/dialogs.js");
const SETTINGS_JS: &str = include_str!("../../../static/js/views/settings.js");
const MODELS_JS: &str = include_str!("../../../static/js/views/models.js");
const COSTS_JS: &str = include_str!("../../../static/js/views/costs.js");

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn js_response(body: &'static str) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

async fn app_js() -> Response {
    js_response(APP_JS)
}

async fn api_js() -> Response {
    js_response(API_JS)
}

async fn ui_js() -> Response {
    js_response(UI_JS)
}

async fn dialogs_js() -> Response {
    js_response(DIALOGS_JS)
}

async fn settings_js() -> Response {
    js_response(SETTINGS_JS)
}

async fn models_js() -> Response {
    js_response(MODELS_JS)
}

async fn costs_js() -> Response {
    js_response(COSTS_JS)
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
        .route("/static/js/api.js", get(api_js))
        .route("/static/js/ui.js", get(ui_js))
        .route("/static/js/views/dialogs.js", get(dialogs_js))
        .route("/static/js/views/settings.js", get(settings_js))
        .route("/static/js/views/models.js", get(models_js))
        .route("/static/js/views/costs.js", get(costs_js))
}
