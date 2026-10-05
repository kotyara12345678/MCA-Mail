use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How much autonomy the system has over the corporate mailbox.
///
/// `DryRun` is the default: it never mutates the real mailbox, which makes it
/// safe to point at production mail before any policy has been approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmailMode {
    /// Analyse and record only. No mailbox mutation, no outbound mail.
    #[default]
    DryRun,
    /// Everything is prepared: drafts, labels, handoffs. Nothing is sent and
    /// the real mailbox is left untouched.
    Review,
    /// Approved actions (labelling, quoting-flow replies, handoffs) execute
    /// automatically, still bounded by the remaining guards.
    Auto,
}

impl EmailMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            EmailMode::DryRun => "dry_run",
            EmailMode::Review => "review",
            EmailMode::Auto => "auto",
        }
    }

    /// May the system send mail to an external party?
    pub const fn allows_sending(&self) -> bool {
        matches!(self, EmailMode::Auto)
    }

    /// May the system mutate the corporate mailbox (labels, folders, flags)?
    pub const fn allows_mailbox_mutation(&self) -> bool {
        matches!(self, EmailMode::Auto)
    }

    /// May the system create drafts for a human to approve?
    pub const fn allows_drafts(&self) -> bool {
        true
    }
}

/// Outbound guard rails. These apply even in `Auto` mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutboundPolicy {
    /// Global kill switch, independent of `EmailMode`.
    pub auto_send: bool,
    /// Per-hour ceiling across all leads.
    pub max_sends_per_hour: u32,
    /// Per-lead ceiling per hour, prevents one chatty lead from eating quota.
    pub max_sends_per_lead_per_hour: u32,
    /// Maximum characters of generated body. Longer output is truncated and the
    /// draft is suppressed instead of sent.
    pub max_body_chars: usize,
    /// Minimum seconds between two outbound messages to the same address.
    pub min_interval_seconds: u64,
    /// Never send more than this many follow-ups without a customer reply.
    pub max_consecutive_replies: u32,
}

impl Default for OutboundPolicy {
    fn default() -> Self {
        Self {
            auto_send: false,
            max_sends_per_hour: 20,
            max_sends_per_lead_per_hour: 5,
            max_body_chars: 8000,
            min_interval_seconds: 60,
            max_consecutive_replies: 6,
        }
    }
}

impl OutboundPolicy {
    pub fn min_interval(&self) -> Duration {
        Duration::from_secs(self.min_interval_seconds)
    }
}

/// Attachments the system is allowed to attach to outbound mail. Anything not
/// listed requires an explicit human action through the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttachmentPolicy {
    /// The agent can never originate attachments at all in this phase.
    pub allow_outbound_attachments: bool,
    pub max_attachment_bytes: u64,
}

impl Default for AttachmentPolicy {
    fn default() -> Self {
        Self {
            allow_outbound_attachments: false,
            max_attachment_bytes: 5 * 1024 * 1024,
        }
    }
}

/// Who receives the manager card and whether it may be sent at all.
///
/// The address is configuration, not code: hard-coding a person's mailbox into
/// the business logic would make the wrong recipient a source change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagerCardSettings {
    /// Off by default: a pilot first proves the card on screen, then turns on
    /// delivery.
    pub enabled: bool,
    /// One address, or several separated by commas or semicolons.
    pub recipient: String,
    /// Ceiling per hour per address, independent of the customer-reply quota.
    pub max_per_hour: u32,
}

impl ManagerCardSettings {
    /// Every address the card goes to, in configuration order.
    ///
    /// The deployment contract is one variable, `MANAGER_CARD_RECIPIENT`, and
    /// an operator names the whole team in it — `a@x.com,b@y.com`. Each entry
    /// becomes its own outbox row, so one manager's hourly ceiling, delivery
    /// failures and audit trail never swallow another's.
    pub fn recipients(&self) -> Vec<&str> {
        self.recipient
            .split([',', ';'])
            .map(str::trim)
            .filter(|address| !address.is_empty())
            .collect()
    }
}

impl Default for ManagerCardSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            recipient: String::new(),
            max_per_hour: 5,
        }
    }
}

/// Inbound size guards, applied before anything is stored or sent to an LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboundPolicy {
    pub max_message_bytes: u64,
    pub max_attachment_bytes: u64,
    pub max_attachments: usize,
    /// Upper bound of characters handed to an LLM in one prompt.
    pub max_llm_body_chars: usize,
    /// Upper bound of characters extracted from one attachment.
    pub max_attachment_text_chars: usize,
}

impl Default for InboundPolicy {
    fn default() -> Self {
        Self {
            max_message_bytes: 10 * 1024 * 1024,
            max_attachment_bytes: 10 * 1024 * 1024,
            max_attachments: 25,
            max_llm_body_chars: 20_000,
            max_attachment_text_chars: 8_000,
        }
    }
}

/// How much context from the conversation is sent to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextPolicy {
    /// Number of most recent messages included in a prompt.
    pub history_messages: usize,
    /// Number of characters of the newest inbound message included.
    pub latest_message_chars: usize,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            history_messages: 6,
            latest_message_chars: 6000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_is_the_default_and_forbids_sending() {
        let mode = EmailMode::default();
        assert_eq!(mode, EmailMode::DryRun);
        assert!(!mode.allows_sending());
        assert!(!mode.allows_mailbox_mutation());
        assert!(mode.allows_drafts());
    }

    #[test]
    fn auto_send_is_off_by_default() {
        assert!(!OutboundPolicy::default().auto_send);
        assert!(!AttachmentPolicy::default().allow_outbound_attachments);
    }

    #[test]
    fn auto_send_requires_auto_mode() {
        let policy = OutboundPolicy {
            auto_send: true,
            ..Default::default()
        };
        assert!(!EmailMode::Review.allows_sending());
        assert!(EmailMode::Auto.allows_sending());
        assert!(policy.auto_send);
    }

    #[test]
    fn the_card_recipient_list_is_one_address_per_manager() {
        let card = ManagerCardSettings {
            recipient: "Savva@Gmail.com, parkin@mca-log.com".to_string(),
            ..Default::default()
        };
        assert_eq!(card.recipients(), ["Savva@Gmail.com", "parkin@mca-log.com"]);

        let spaced = ManagerCardSettings {
            recipient: "a@x.com;  b@y.com ,, ".to_string(),
            ..Default::default()
        };
        assert_eq!(spaced.recipients(), ["a@x.com", "b@y.com"]);
    }

    #[test]
    fn no_recipient_is_an_empty_list_not_one_blank_address() {
        let card = ManagerCardSettings::default();
        assert!(card.recipients().is_empty());
        assert!(ManagerCardSettings {
            recipient: " , ; ".into(),
            ..Default::default()
        }
        .recipients()
        .is_empty());
    }
}
