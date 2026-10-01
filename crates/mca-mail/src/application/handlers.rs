//! Tool handler factories used during bootstrap.
//!
//! The handlers are stubs that acknowledge the call; real implementations
//! will use persistence once the tool loop is wired end to end.

use std::sync::Arc;

use crate::error::ToolError;
use crate::tools::ToolResult;

fn stub(name: &str, args: Option<&serde_json::Value>) -> Result<ToolResult, ToolError> {
    let args_count = args.map(|a| a.as_object().map(|o| o.len()).unwrap_or(0));
    let summary = match args_count {
        Some(n) => format!("{name} called with {n} args"),
        None => format!("{name} called"),
    };
    let data = match args {
        Some(a) => serde_json::json!({"called": name, "args": a}),
        None => serde_json::json!({"called": name}),
    };
    Ok(ToolResult {
        success: true,
        summary,
        data,
        error: None,
    })
}

pub(super) fn make_crm_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |args| stub(&n, Some(&args)))
}

pub(super) fn make_mail_tool_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |_args| stub(&n, None))
}

pub(super) fn make_support_tool_handler(
    name: &str,
) -> Arc<dyn Fn(serde_json::Value) -> Result<ToolResult, ToolError> + Send + Sync> {
    let n = name.to_string();
    Arc::new(move |args| stub(&n, Some(&args)))
}
