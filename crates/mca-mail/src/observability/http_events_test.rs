//! HTTP event helpers: success and failure framing.

use super::test_util::{await_marker, run};
use super::{bus, http};

#[test]
fn http_events_carry_status_duration_and_request_id() {
    let mut rx = bus::subscribe();
    run(|| {
        http::http_request("GET", "/health", 200, 12, "rid-1");
        http::http_request_failure(
            "POST",
            "/api/emails/reprocess",
            503,
            "server_error",
            40,
            "rid-2",
        );
        http::http_request_failure("GET", "/slow", 408, "timeout", 30_000, "rid-3");
        http::http_request_failure("GET", "/missing", 404, "client_error", 1, "rid-4");
    });

    let ev = await_marker(&mut rx, "rid-1");
    assert_eq!(ev.event, "http_request");
    assert!(ev.json.contains("\"status\":200"), "{}", ev.json);
    assert!(ev.json.contains("\"duration_ms\":12"), "{}", ev.json);

    let ev = await_marker(&mut rx, "rid-2");
    assert_eq!(ev.event, "http_request_failure");
    assert!(
        ev.json.contains("\"error_type\":\"server_error\""),
        "{}",
        ev.json
    );

    let ev = await_marker(&mut rx, "rid-3");
    assert_eq!(ev.event, "http_request_failure");
    assert!(
        ev.json.contains("\"error_type\":\"timeout\""),
        "{}",
        ev.json
    );

    let ev = await_marker(&mut rx, "rid-4");
    assert_eq!(ev.event, "http_request_failure");
    assert!(
        ev.json.contains("\"error_type\":\"client_error\""),
        "{}",
        ev.json
    );
}
