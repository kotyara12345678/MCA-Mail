//! Which mailbox-mutating operation a guard check refers to.
//!
//! A typed enum, not a string: the whole point of the read-only guard is that
//! the log line, the error and the test assertions all name the same operation,
//! and a free-form string would let a typo silently create an unprotected path.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailboxOp {
    Send,
    AppendDraft,
    AppendSent,
    Move,
    Copy,
    Delete,
    Archive,
    SetFlag,
    ClearFlag,
    AddLabel,
    RemoveLabel,
}

impl MailboxOp {
    pub const fn as_str(&self) -> &'static str {
        match self {
            MailboxOp::Send => "smtp.send",
            MailboxOp::AppendDraft => "imap.append_draft",
            MailboxOp::AppendSent => "imap.append_sent",
            MailboxOp::Move => "imap.move",
            MailboxOp::Copy => "imap.copy",
            MailboxOp::Delete => "imap.delete",
            MailboxOp::Archive => "imap.archive",
            MailboxOp::SetFlag => "imap.set_flag",
            MailboxOp::ClearFlag => "imap.clear_flag",
            MailboxOp::AddLabel => "imap.add_label",
            MailboxOp::RemoveLabel => "imap.remove_label",
        }
    }
}

impl fmt::Display for MailboxOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_has_a_distinct_name() {
        let all = [
            MailboxOp::Send,
            MailboxOp::AppendDraft,
            MailboxOp::AppendSent,
            MailboxOp::Move,
            MailboxOp::Copy,
            MailboxOp::Delete,
            MailboxOp::Archive,
            MailboxOp::SetFlag,
            MailboxOp::ClearFlag,
            MailboxOp::AddLabel,
            MailboxOp::RemoveLabel,
        ];
        let mut names: Vec<&str> = all.iter().map(|op| op.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "operation names must be unique");
    }
}
