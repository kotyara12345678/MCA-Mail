//! Request logging middleware: request id, 30s timeout, http events.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::observability::http;

const REQUEST_ID_HEADER: &str = "x-request-id";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Outermost middleware for every matched route: keeps or mints
/// `x-request-id`, enforces the 30s timeout and emits
/// `http_request` / `http_request_failure` with the matched route pattern.
pub async fn log_request(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let method = request.method().as_str().to_owned();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| request.uri().path().to_owned());

    let mut request = request;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        request
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }

    let started = Instant::now();
    let outcome = tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let mut response = match outcome {
        Ok(response) => response,
        Err(_) => {
            let status = StatusCode::REQUEST_TIMEOUT;
            http::http_request_failure(
                &method,
                &path,
                status.as_u16(),
                "timeout",
                duration_ms,
                &request_id,
            );
            let mut response = status.into_response();
            insert_request_id(&mut response, &request_id);
            return response;
        }
    };

    insert_request_id(&mut response, &request_id);
    let status = response.status();
    if status.is_success() || status.is_redirection() {
        http::http_request(&method, &path, status.as_u16(), duration_ms, &request_id);
    } else {
        let error_type = if status.is_server_error() {
            "server_error"
        } else {
            "client_error"
        };
        http::http_request_failure(
            &method,
            &path,
            status.as_u16(),
            error_type,
            duration_ms,
            &request_id,
        );
    }
    response
}

fn insert_request_id(response: &mut Response, request_id: &str) {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }
}
