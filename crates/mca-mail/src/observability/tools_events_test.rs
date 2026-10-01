//! Event helper tests: tool execution and error classification.

use super::test_util::{await_marker, run};
use super::{bus, errors, tools};

#[test]
fn tool_events_report_success_and_timeout() {
    let mut rx = bus::subscribe();
    run(|| {
        tools::tool_started("handoff", "crm_lookup");
        tools::tool_completed("handoff", "crm_lookup", "found 1 lead", 30);
        tools::tool_timeout("handoff", "web_enrich", 5000);
        tools::tool_failed("handoff", "smtp_send", "mail", "tool-mark-fail", 10);
    });
    let ev = await_marker(&mut rx, "crm_lookup");
    assert_eq!(ev.event, "tool_started");
    let ev = await_marker(&mut rx, "found 1 lead");
    assert_eq!(ev.event, "tool_completed");
    let ev = await_marker(&mut rx, "web_enrich");
    assert_eq!(ev.event, "tool_timeout");
    let ev = await_marker(&mut rx, "tool-mark-fail");
    assert_eq!(ev.event, "tool_failed");
}

#[test]
fn error_types_are_short_and_stable() {
    use crate::error::{AppError, LlmError};
    assert_eq!(errors::app_error_type(&AppError::internal("x")), "internal");
    assert_eq!(
        errors::app_error_type(&AppError::NotFound("e".into())),
        "not_found"
    );
    assert_eq!(errors::llm_error_type(&LlmError::Timeout(1000)), "timeout");
    assert_eq!(
        errors::llm_error_type(&LlmError::CircuitOpen("x".into())),
        "circuit_open"
    );
    assert_eq!(
        errors::sqlx_error_type(&sqlx::Error::PoolClosed),
        "pool_closed"
    );
    assert_eq!(errors::clip("abcdef", 3), "abc…");
    assert_eq!(errors::clip("ab", 3), "ab");
}
