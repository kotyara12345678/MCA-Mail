//! Which tools exist in each mail mode.
//!
//! The mail-layer guard is the real enforcement, but an agent should not even be
//! offered a tool that is guaranteed to fail: a refused call wastes an iteration
//! and, worse, teaches the model that refusals are noise to retry. So the
//! registry filters by mode as a second, advisory line of defence.

use crate::config::MailMode;

use super::ToolDef;

/// Tools that only read. Always available.
const READ_ONLY_TOOLS: &[&str] = &[
    "list_emails",
    "read_email",
    "read_thread",
    "search_emails",
    "get_email_attachments",
    "get_email_status",
];

/// Whether a tool changes server-side mailbox state.
///
/// `create_email_draft` is the subtle one: the draft row lives in the agent's
/// own database, so it is allowed in read-only. Only the IMAP `APPEND` behind it
/// is not, and that path is guarded separately.
pub const MUTATING_TOOLS: &[&str] = &[
    "send_email",
    "move_email",
    "label_email",
    "archive_email",
    "mark_as_read",
];

const READ_WRITE_TOOLS: &[&str] = &["move_email", "label_email", "archive_email", "mark_as_read"];

/// Whether a tool changes server-side mailbox state.
pub fn mutates_mailbox(tool: &str) -> bool {
    MUTATING_TOOLS.contains(&tool)
}

/// Whether a tool is permitted in this mode.
pub fn allowed_in(tool: &str, mode: MailMode) -> bool {
    match mode {
        MailMode::Work | MailMode::ReadWrite => {
            !mutates_mailbox(tool) || READ_WRITE_TOOLS.contains(&tool)
        }
        MailMode::ReadOnly => !mutates_mailbox(tool),
    }
}

/// Drop the tools a mode forbids.
pub fn filter_for_mode(tools: Vec<ToolDef>, mode: MailMode) -> Vec<ToolDef> {
    tools
        .into_iter()
        .filter(|t| allowed_in(&t.name, mode))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_hides_every_mailbox_mutation() {
        for tool in [
            "send_email",
            "move_email",
            "label_email",
            "archive_email",
            "mark_as_read",
        ] {
            assert!(mutates_mailbox(tool), "{tool} should be a mutation");
            assert!(
                !allowed_in(tool, MailMode::ReadOnly),
                "{tool} must be hidden"
            );
            assert_eq!(
                allowed_in(tool, MailMode::Work),
                READ_WRITE_TOOLS.contains(&tool),
                "unexpected read_write permission for {tool}"
            );
        }
        assert!(!allowed_in("send_email", MailMode::ReadWrite));
    }

    #[test]
    fn read_only_keeps_every_read_and_the_database_draft() {
        for tool in READ_ONLY_TOOLS {
            assert!(allowed_in(tool, MailMode::ReadOnly), "{tool} must stay");
        }
        // A draft in the agent's own database is not a mailbox mutation.
        assert!(!mutates_mailbox("create_email_draft"));
        assert!(allowed_in("create_email_draft", MailMode::ReadOnly));
    }

    #[test]
    fn read_write_does_not_enable_smtp_tools() {
        assert!(!allowed_in("send_email", MailMode::ReadWrite));
        for tool in READ_WRITE_TOOLS {
            assert!(allowed_in(tool, MailMode::ReadWrite));
        }
    }
    #[test]
    fn filtering_the_real_registry_removes_exactly_the_mutations() {
        let all = super::super::mail::all_tools();
        let read_only = filter_for_mode(all.clone(), MailMode::ReadOnly);
        assert!(read_only.len() < all.len());
        assert!(read_only.iter().all(|t| !mutates_mailbox(&t.name)));
        assert_eq!(
            filter_for_mode(all, MailMode::Work).len(),
            read_only.len() + READ_WRITE_TOOLS.len(),
            "legacy work mode should map to safe read_write tools"
        );
    }
}
