use super::build;
use crate::config::{ImapSettings, MailProviderKind, MailSettings};

fn imap_settings() -> MailSettings {
    MailSettings {
        provider: MailProviderKind::Imap,
        imap: ImapSettings {
            use_idle: true,
            ..ImapSettings::default()
        },
        ..MailSettings::default()
    }
}

/// IDLE is only wired up where it can actually be used.
#[test]
fn idle_is_built_for_an_imap_mailbox_with_the_switch_on() {
    assert!(build(&imap_settings()).is_some());
}

/// `MAIL_IDLE=false` is the operator's way out, and must be honoured even when
/// the rest of the settings still describe an IDLE-capable server.
#[test]
fn idle_is_not_built_when_the_switch_is_off() {
    let mut mail = imap_settings();
    mail.imap.use_idle = false;
    assert!(build(&mail).is_none());
}

/// The mock has no connection to idle on; asking it for one would be a bug.
#[test]
fn idle_is_not_built_for_the_mock_provider() {
    let mut mail = imap_settings();
    mail.provider = MailProviderKind::Mock;
    assert!(build(&mail).is_none());
}
