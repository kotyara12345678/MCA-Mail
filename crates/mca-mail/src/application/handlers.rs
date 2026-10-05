//! Tool handler factories used during bootstrap.
//!
//! Every handler here is a placeholder: the tool loop does not call them yet,
//! and no backend stands behind them. They still have to answer honestly. An
//! agent that has just "sent" an email or "handed off" a lead will tell the
//! customer or the manager that it did, so a stub that reports `success: true`
//! is a stub that produces a false statement downstream.

use std::sync::Arc;

use serde_json::json;

use crate::error::ToolError;
use crate::tools::ToolResult;

/// Acknowledge the call without pretending anything happened.
///
/// The failure is deliberate and terminal for this call: the caller sees the
/// tool is unwired instead of a summary it could repeat as a completed action.
fn unavailable(name: &str, args: Option<&serde_json::Value>) -> Result<ToolResult, ToolError> {
    let message = format!("{name} has no backend wired up yet; nothing was performed");
    Ok(ToolResult {
        success: false,
        summary: message.clone(),
        data: json!({
            "tool": name,
            "performed": false,
            "args": args.cloned().unwrap_or(serde_json::Value::Null),
        }),
        error: Some(message),
    })
}

pub(super) fn make_crm_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |args| unavailable(&n, Some(&args)))
}

pub(super) fn make_mail_tool_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |args| unavailable(&n, Some(&args)))
}

pub(super) fn make_support_tool_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |args| unavailable(&n, Some(&args)))
}
