//! Tests for `smtp.rs`.

#![cfg(test)]

use super::*;
use super::*;

fn auth(user: &str, pass: &str) -> TransportAuth {
    TransportAuth::new(user.to_string(), pass.to_string())
}

#[test]
fn empty_credentials_are_refused() {
    assert!(auth("user", "").credentials().is_err());
    assert!(auth("", "pass").credentials().is_err());
    assert!(auth("user", "pass").credentials().is_ok());
}

#[test]
fn missing_host_and_sender_are_refused_before_any_socket() {
    let mut settings = MailSettings::default();
    assert!(SmtpTransport::new(&settings, auth("u", "p")).is_err());

    settings.smtp.host = "smtp.example.com".into();
    assert!(SmtpTransport::new(&settings, auth("u", "p")).is_err());
}

#[test]
fn starttls_is_required_not_opportunistic() {
    // Opportunistic STARTTLS would downgrade to plaintext on a server that
    // does not advertise it.
    assert!(matches!(
        tls_mode(TlsMode::StartTls, "smtp.example.com"),
        Ok(Tls::Required(_))
    ));
    assert!(matches!(
        tls_mode(TlsMode::Implicit, "smtp.example.com"),
        Ok(Tls::Wrapper(_))
    ));
    assert!(matches!(
        tls_mode(TlsMode::None, "smtp.example.com"),
        Ok(Tls::None)
    ));
}
