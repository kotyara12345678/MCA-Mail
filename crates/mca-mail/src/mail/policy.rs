use chrono::{DateTime, Duration, Utc};

use crate::config::{EmailMode, OutboundPolicy};
use crate::error::PolicyError;

/// Everything the guard rails need to know about one proposed send.
///
/// Deliberately a struct: the rules below are only meaningful together, and a
/// positional argument list would let a caller silently swap, say, the per-lead
/// count with the global one.
#[derive(Debug, Clone)]
pub struct PolicyContext {
    pub mode: EmailMode,
    /// Recipient address, lowercased.
    pub recipient: String,
    pub lead_id: Option<uuid::Uuid>,
    pub body_chars: usize,
    /// Number of outbound messages sent to this recipient in the last hour.
    pub recipient_sends_last_hour: i64,
    /// Number of outbound messages sent by any lead in the last hour.
    pub total_sends_last_hour: i64,
    /// Messages this lead has sent to this recipient since the last inbound.
    pub consecutive_replies: i64,
    /// Timestamp of the last message sent to this recipient.
    pub last_sent_at: Option<DateTime<Utc>>,
    pub has_attachment: bool,
    /// Whether a human approved this specific draft.
    pub human_approved: bool,
    /// `true` while a lead's automation is locked, i.e. a handoff is open.
    pub automation_locked: bool,
    /// Live global kill switch from `system_settings`.
    pub automation_enabled: bool,
    pub now: DateTime<Utc>,
}

impl PolicyContext {
    /// A context that permits nothing, for callers that have no data yet.
    pub fn denied_by_default(
        mode: EmailMode,
        recipient: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            mode,
            recipient: recipient.into().to_ascii_lowercase(),
            lead_id: None,
            body_chars: 0,
            recipient_sends_last_hour: 0,
            total_sends_last_hour: 0,
            consecutive_replies: 0,
            last_sent_at: None,
            has_attachment: false,
            human_approved: false,
            automation_locked: false,
            automation_enabled: true,
            now,
        }
    }

    pub fn with_sends(mut self, recipient: i64, total: i64) -> Self {
        self.recipient_sends_last_hour = recipient;
        self.total_sends_last_hour = total;
        self
    }

    pub fn with_body(mut self, chars: usize) -> Self {
        self.body_chars = chars;
        self
    }

    pub fn approved(mut self) -> Self {
        self.human_approved = true;
        self
    }
}

/// The outcome of a policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundDecision {
    /// Send now.
    Send,
    /// Hold as a draft for a human. `reason` is recorded on the draft.
    Draft(PolicyDenial),
    /// Suppress entirely: sending a follow-up would be harmful.
    Suppress(PolicyDenial),
}

impl OutboundDecision {
    pub const fn is_send(&self) -> bool {
        matches!(self, OutboundDecision::Send)
    }

    pub fn reason(&self) -> Option<&PolicyDenial> {
        match self {
            OutboundDecision::Send => None,
            OutboundDecision::Draft(r) | OutboundDecision::Suppress(r) => Some(r),
        }
    }
}

/// Why a send was not allowed. Recorded verbatim on the draft so a manager can
/// see the reason without reading the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDenial {
    ModeIsNotAuto,
    AutoSendDisabled,
    AutomationDisabled,
    AutomationLocked,
    BodyTooLong { limit: usize, actual: usize },
    GlobalRateLimit { limit: u32 },
    RecipientRateLimit { limit: u32 },
    MinIntervalNotElapsed { wait_seconds: i64 },
    TooManyConsecutiveReplies { limit: u32 },
    AttachmentNotPermitted,
    NoRecipient,
}

impl PolicyDenial {
    pub const fn as_str(&self) -> &'static str {
        match self {
            PolicyDenial::ModeIsNotAuto => "mode_is_not_auto",
            PolicyDenial::AutoSendDisabled => "auto_send_disabled",
            PolicyDenial::AutomationDisabled => "automation_disabled",
            PolicyDenial::AutomationLocked => "automation_locked",
            PolicyDenial::BodyTooLong { .. } => "body_too_long",
            PolicyDenial::GlobalRateLimit { .. } => "global_rate_limit",
            PolicyDenial::RecipientRateLimit { .. } => "recipient_rate_limit",
            PolicyDenial::MinIntervalNotElapsed { .. } => "min_interval_not_elapsed",
            PolicyDenial::TooManyConsecutiveReplies { .. } => "too_many_consecutive_replies",
            PolicyDenial::AttachmentNotPermitted => "attachment_not_permitted",
            PolicyDenial::NoRecipient => "no_recipient",
        }
    }

    /// Whether the condition is permanent, so the orchestrator suppresses
    /// instead of retrying later.
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            PolicyDenial::BodyTooLong { .. }
                | PolicyDenial::AttachmentNotPermitted
                | PolicyDenial::NoRecipient
                | PolicyDenial::ModeIsNotAuto
                | PolicyDenial::AutoSendDisabled
        )
    }
}

impl std::fmt::Display for PolicyDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyDenial::BodyTooLong { limit, actual } => {
                write!(f, "body is {actual} characters, limit is {limit}")
            }
            PolicyDenial::GlobalRateLimit { limit } => {
                write!(f, "global hourly send limit of {limit} reached")
            }
            PolicyDenial::RecipientRateLimit { limit } => {
                write!(f, "recipient hourly send limit of {limit} reached")
            }
            PolicyDenial::MinIntervalNotElapsed { wait_seconds } => {
                write!(
                    f,
                    "wait {wait_seconds}s before sending again to this address"
                )
            }
            PolicyDenial::TooManyConsecutiveReplies { limit } => {
                write!(f, "more than {limit} replies without a customer answer")
            }
            other => f.write_str(other.as_str()),
        }
    }
}

impl std::error::Error for PolicyDenial {}

/// Guard rails applied to every outbound message.
///
/// The order is deliberate: cheap, permanent checks first, so a permanently
/// invalid message is suppressed in one pass, and a human-approval override is
/// considered only after the structural checks have passed.
#[derive(Debug, Clone)]
pub struct OutboundPolicyGuard {
    mode: EmailMode,
    policy: OutboundPolicy,
    /// Outbound attachments are refused outright in this phase.
    allow_attachments: bool,
}

impl OutboundPolicyGuard {
    pub fn new(mode: EmailMode, policy: OutboundPolicy, allow_attachments: bool) -> Self {
        Self {
            mode,
            policy,
            allow_attachments,
        }
    }

    pub fn from_config(config: &crate::config::AppConfig) -> Self {
        Self::new(
            config.security.email_mode,
            config.security.outbound.clone(),
            config.security.attachments.allow_outbound_attachments,
        )
    }

    pub fn mode(&self) -> EmailMode {
        self.mode
    }

    pub fn evaluate(&self, context: &PolicyContext) -> OutboundDecision {
        if context.recipient.trim().is_empty() {
            return OutboundDecision::Suppress(PolicyDenial::NoRecipient);
        }
        if let Some(denial) = self.structural_denial(context) {
            return OutboundDecision::Suppress(denial);
        }
        if let Some(denial) = self.mode_denial(context) {
            return OutboundDecision::Draft(denial);
        }
        if let Some(denial) = self.rate_denial(context) {
            return OutboundDecision::Draft(denial);
        }
        OutboundDecision::Send
    }

    /// Checks that no mode or approval can rescue.
    fn structural_denial(&self, context: &PolicyContext) -> Option<PolicyDenial> {
        if context.body_chars > self.policy.max_body_chars {
            return Some(PolicyDenial::BodyTooLong {
                limit: self.policy.max_body_chars,
                actual: context.body_chars,
            });
        }
        if context.has_attachment && !self.allow_attachments {
            return Some(PolicyDenial::AttachmentNotPermitted);
        }
        None
    }

    /// Mode, kill switches and the automation lock.
    fn mode_denial(&self, context: &PolicyContext) -> Option<PolicyDenial> {
        if context.automation_locked {
            return Some(PolicyDenial::AutomationLocked);
        }
        if !context.automation_enabled {
            return Some(PolicyDenial::AutomationDisabled);
        }
        if context.human_approved {
            // A manager pressing "send" overrides the mode and the auto-send
            // switch, but never the automation lock: an open handoff means a
            // human already owns the conversation.
            return None;
        }
        if !self.mode.allows_sending() {
            return Some(PolicyDenial::ModeIsNotAuto);
        }
        if !self.policy.auto_send {
            return Some(PolicyDenial::AutoSendDisabled);
        }
        None
    }

    /// Volume and pacing limits.
    fn rate_denial(&self, context: &PolicyContext) -> Option<PolicyDenial> {
        if context.total_sends_last_hour >= i64::from(self.policy.max_sends_per_hour) {
            return Some(PolicyDenial::GlobalRateLimit {
                limit: self.policy.max_sends_per_hour,
            });
        }
        if context.recipient_sends_last_hour >= i64::from(self.policy.max_sends_per_lead_per_hour) {
            return Some(PolicyDenial::RecipientRateLimit {
                limit: self.policy.max_sends_per_lead_per_hour,
            });
        }
        if context.consecutive_replies >= i64::from(self.policy.max_consecutive_replies) {
            return Some(PolicyDenial::TooManyConsecutiveReplies {
                limit: self.policy.max_consecutive_replies,
            });
        }
        if let Some(last) = context.last_sent_at {
            let elapsed = (context.now - last).num_seconds();
            let wait = self.policy.min_interval().as_secs() as i64 - elapsed;
            if wait > 0 {
                return Some(PolicyDenial::MinIntervalNotElapsed { wait_seconds: wait });
            }
        }
        None
    }

    /// Convenience wrapper returning a `PolicyError` for callers that treat a
    /// denial as a failure rather than a branch.
    pub fn require_send(&self, context: &PolicyContext) -> Result<(), PolicyError> {
        match self.evaluate(context) {
            OutboundDecision::Send => Ok(()),
            OutboundDecision::Draft(d) | OutboundDecision::Suppress(d) => {
                Err(PolicyError::ContentRejected {
                    reason: format!("{}: {d}", d.as_str()),
                })
            }
        }
    }

    /// Seconds until the pacing limit allows a retry, if that is what blocked.
    pub fn retry_after(&self, context: &PolicyContext) -> Option<Duration> {
        match self.evaluate(context) {
            OutboundDecision::Draft(PolicyDenial::MinIntervalNotElapsed { wait_seconds }) => {
                Some(Duration::seconds(wait_seconds))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> OutboundPolicyGuard {
        OutboundPolicyGuard::new(
            EmailMode::Auto,
            OutboundPolicy {
                auto_send: true,
                ..Default::default()
            },
            false,
        )
    }

    fn context() -> PolicyContext {
        let mut ctx =
            PolicyContext::denied_by_default(EmailMode::Auto, "customer@example.com", Utc::now());
        ctx.body_chars = 100;
        ctx
    }

    #[test]
    fn auto_mode_with_auto_send_permits_a_normal_send() {
        assert_eq!(guard().evaluate(&context()), OutboundDecision::Send);
    }

    #[test]
    fn dry_run_drafts_instead_of_sending() {
        let g = OutboundPolicyGuard::new(
            EmailMode::DryRun,
            OutboundPolicy {
                auto_send: true,
                ..Default::default()
            },
            false,
        );
        assert_eq!(
            g.evaluate(&context()),
            OutboundDecision::Draft(PolicyDenial::ModeIsNotAuto)
        );
    }

    #[test]
    fn auto_send_off_drafts_even_in_auto_mode() {
        let g = OutboundPolicyGuard::new(
            EmailMode::Auto,
            OutboundPolicy {
                auto_send: false,
                ..Default::default()
            },
            false,
        );
        assert_eq!(
            g.evaluate(&context()),
            OutboundDecision::Draft(PolicyDenial::AutoSendDisabled)
        );
    }

    #[test]
    fn an_open_handoff_blocks_automatic_sends() {
        let mut ctx = context();
        ctx.automation_locked = true;
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::AutomationLocked)
        );
    }

    #[test]
    fn the_global_kill_switch_beats_a_manager_approval() {
        let mut ctx = context().approved();
        ctx.automation_enabled = false;
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::AutomationDisabled)
        );
    }

    #[test]
    fn approval_overrides_mode_but_not_the_lock() {
        let g = OutboundPolicyGuard::new(EmailMode::Review, OutboundPolicy::default(), false);
        let mut ctx =
            PolicyContext::denied_by_default(EmailMode::Review, "customer@example.com", Utc::now());
        ctx.body_chars = 100;
        ctx.human_approved = true;
        assert_eq!(g.evaluate(&ctx), OutboundDecision::Send);

        ctx.automation_locked = true;
        assert_eq!(
            g.evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::AutomationLocked)
        );
    }

    #[test]
    fn an_oversized_body_is_suppressed_not_drafted() {
        let mut ctx = context();
        ctx.body_chars = 100_000;
        match guard().evaluate(&ctx) {
            OutboundDecision::Suppress(PolicyDenial::BodyTooLong { limit, actual }) => {
                assert_eq!(limit, 8000);
                assert_eq!(actual, 100_000);
            }
            other => panic!("expected suppression, got {other:?}"),
        }
    }

    #[test]
    fn attachments_are_refused_in_this_phase() {
        let mut ctx = context();
        ctx.has_attachment = true;
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Suppress(PolicyDenial::AttachmentNotPermitted)
        );
    }

    #[test]
    fn a_missing_recipient_is_suppressed() {
        let mut ctx = context();
        ctx.recipient = "  ".into();
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Suppress(PolicyDenial::NoRecipient)
        );
    }

    #[test]
    fn rate_limits_are_drafted_for_later() {
        let ctx = context().with_sends(0, 100);
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::GlobalRateLimit { limit: 20 })
        );

        let ctx = context().with_sends(50, 0);
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::RecipientRateLimit { limit: 5 })
        );
    }

    #[test]
    fn pacing_waits_and_reports_the_delay() {
        let now = Utc::now();
        let mut ctx = context();
        ctx.last_sent_at = Some(now - Duration::seconds(10));
        let g = guard();
        assert_eq!(
            g.evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::MinIntervalNotElapsed { wait_seconds: 50 })
        );
        assert_eq!(g.retry_after(&ctx), Some(Duration::seconds(50)));
    }

    #[test]
    fn too_many_replies_without_an_answer_stops_the_loop() {
        let mut ctx = context();
        ctx.consecutive_replies = 10;
        assert_eq!(
            guard().evaluate(&ctx),
            OutboundDecision::Draft(PolicyDenial::TooManyConsecutiveReplies { limit: 6 })
        );
    }

    #[test]
    fn denials_report_whether_a_retry_can_help() {
        assert!(PolicyDenial::BodyTooLong {
            limit: 1,
            actual: 2
        }
        .is_terminal());
        assert!(!PolicyDenial::GlobalRateLimit { limit: 1 }.is_terminal());
    }
}
