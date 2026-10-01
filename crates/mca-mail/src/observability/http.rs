//! HTTP server events: every request with status, duration and request id.

use tracing::{error, info, warn};

use super::EVENT_TARGET;

/// A request finished successfully (2xx/3xx).
pub fn http_request(method: &str, path: &str, status: u16, duration_ms: u64, request_id: &str) {
    info!(
        target: EVENT_TARGET,
        method,
        path,
        status,
        duration_ms,
        request_id,
        "http_request"
    );
}

/// A request failed: `error_type` is `client_error`, `server_error` or `timeout`.
pub fn http_request_failure(
    method: &str,
    path: &str,
    status: u16,
    error_type: &str,
    duration_ms: u64,
    request_id: &str,
) {
    if status >= 500 || error_type == "timeout" {
        error!(
            target: EVENT_TARGET,
            method,
            path,
            status,
            error_type,
            duration_ms,
            request_id,
            "http_request_failure"
        );
    } else {
        warn!(
            target: EVENT_TARGET,
            method,
            path,
            status,
            error_type,
            duration_ms,
            request_id,
            "http_request_failure"
        );
    }
}
