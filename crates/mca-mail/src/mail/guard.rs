//! The single enforcement point for mailbox write access.
//!
//! Every operation that would change server-side state calls [`MailGuard::check`]
//! *before* building a request. A refusal therefore costs no network round trip,
//! and no prompt, agent, orchestrator or worker can reach the transport without
//! passing through here — the guard lives in the mail layer, below all of them.

use crate::config::MailMode;
use crate::error::MailError;
use crate::mail::ops::MailboxOp;

#[derive(Debug, Clone, Copy)]
pub struct MailGuard {
    mode: MailMode,
}

impl MailGuard {
    pub fn new(mode: MailMode) -> Self {
        Self { mode }
    }

    pub fn mode(&self) -> MailMode {
        self.mode
    }

    pub fn allows_write(&self) -> bool {
        self.mode.allows_mailbox_write()
    }

    /// Refuse an operation unless the mode permits it.
    ///
    /// SMTP send is not a mailbox mutation — nothing stored on the server
    /// changes — but it is the one operation that reaches an outside party, so
    /// it is held to the same bar as a write: the deployment must have opted
    /// into `read_write`/`work` explicitly. The actual decision to email
    /// someone then still has to survive `OutboundPolicyGuard`, which owns
    /// mode, auto-send, rate limits and the automation lock.
    ///
    /// The audit line carries the mode, the operation and a message id when one
    /// is known. Message *content*, recipients and credentials never appear.
    pub fn check(&self, op: MailboxOp, subject: Option<&str>) -> Result<(), MailError> {
        let allowed = match self.mode {
            MailMode::ReadOnly => false,
            MailMode::ReadWrite | MailMode::Work => matches!(
                op,
                MailboxOp::Move
                    | MailboxOp::Copy
                    | MailboxOp::SetFlag
                    | MailboxOp::ClearFlag
                    | MailboxOp::Send
                    // Keeping a copy of a message that already left is the
                    // client behaviour every mail program performs; it changes
                    // nothing the customer can see and cannot reach anyone.
                    | MailboxOp::AppendSent
            ),
        };
        if allowed {
            return Ok(());
        }
        tracing::warn!(
            mode = self.mode.as_str(),
            operation = op.as_str(),
            subject = subject.unwrap_or("-"),
            "refused mailbox mutation under read-only mode"
        );
        Err(MailError::OperationNotAllowed {
            op: op.as_str().to_string(),
            mode: self.mode.as_str().to_string(),
        })
    }

    pub fn check_flag(
        &self,
        op: MailboxOp,
        subject: Option<&str>,
        flag: &str,
    ) -> Result<(), MailError> {
        self.check(op, subject)?;
        if matches!(flag.to_ascii_uppercase().as_str(), "\\SEEN" | "$JUNK") {
            return Ok(());
        }
        Err(MailError::OperationNotAllowed {
            op: op.as_str().to_string(),
            mode: self.mode.as_str().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[MailboxOp] = &[
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

    #[test]
    fn read_only_refuses_every_mutation() {
        let guard = MailGuard::new(MailMode::ReadOnly);
        for op in ALL {
            let err = guard.check(*op, None).expect_err("must refuse");
            match err {
                MailError::OperationNotAllowed { op: name, mode } => {
                    assert_eq!(name, op.as_str());
                    assert_eq!(mode, "read_only");
                }
                other => panic!("unexpected error: {other}"),
            }
        }
    }

    #[test]
    fn read_write_allows_moves_flags_and_send() {
        let guard = MailGuard::new(MailMode::ReadWrite);
        for op in [
            MailboxOp::Move,
            MailboxOp::Copy,
            MailboxOp::SetFlag,
            MailboxOp::ClearFlag,
            MailboxOp::Send,
            MailboxOp::AppendSent,
        ] {
            assert!(guard.check(op, None).is_ok());
        }
        for op in [MailboxOp::AppendDraft, MailboxOp::Delete] {
            assert!(guard.check(op, None).is_err());
        }
    }

    #[test]
    fn read_only_refuses_outbound_send_too() {
        let err = MailGuard::new(MailMode::ReadOnly)
            .check(MailboxOp::Send, None)
            .expect_err("read-only must never reach SMTP");
        assert!(err.to_string().contains("smtp.send"));
    }

    #[test]
    fn legacy_work_mode_still_denies_delete() {
        let guard = MailGuard::new(MailMode::Work);
        assert!(guard.check(MailboxOp::Move, None).is_ok());
        assert!(guard.check(MailboxOp::Delete, None).is_err());
        assert!(guard.check(MailboxOp::AppendDraft, None).is_err());
        // `Send` is a capability, not a policy: whether a queued message may
        // actually leave is decided by `OutboundPolicyGuard`, not by the
        // mailbox mode. Read-only still refuses it outright.
        assert!(guard.check(MailboxOp::Send, None).is_ok());
    }

    #[test]
    fn deleted_flag_is_never_allowed() {
        let guard = MailGuard::new(MailMode::ReadWrite);
        assert!(guard.check_flag(MailboxOp::SetFlag, None, "\\Seen").is_ok());
        assert!(guard.check_flag(MailboxOp::SetFlag, None, "$Junk").is_ok());
        assert!(guard
            .check_flag(MailboxOp::SetFlag, None, "\\Deleted")
            .is_err());
    }

    #[test]
    fn refusal_does_not_leak_message_content() {
        let guard = MailGuard::new(MailMode::ReadOnly);
        let err = guard
            .check(MailboxOp::Move, Some("uid-42"))
            .expect_err("must refuse");
        let text = err.to_string();
        assert!(text.contains("imap.move"));
        assert!(
            !text.contains("uid-42"),
            "uid must not be in the error text"
        );
    }
}
