use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::config::{MailSettings, TlsMode};
use crate::domain::{ChannelHealth, OutboundMessage};
use crate::error::MailError;

use super::render;

/// SMTP credentials, cloned into each transport that needs them.
#[derive(Debug, Clone)]
pub struct TransportAuth {
    username: String,
    password: String,
}

impl TransportAuth {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }

    /// Build credentials, refusing an empty username or password.
    ///
    /// A silently unauthenticated relay would accept mail from this process on
    /// behalf of anyone, so this is an error rather than a default.
    pub fn credentials(&self) -> Result<Credentials, MailError> {
        if self.password.is_empty() {
            return Err(MailError::Auth("empty SMTP password".into()));
        }
        if self.username.is_empty() {
            return Err(MailError::Auth("empty SMTP username".into()));
        }
        Ok(Credentials::new(
            self.username.clone(),
            self.password.clone(),
        ))
    }
}

/// Async SMTP sender.
///
/// Built once at startup and reused: the transport pools connections, and
/// rebuilding it per message would add a TLS handshake to every reply.
pub struct SmtpTransport {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from_address: String,
    from_name: String,
}

impl SmtpTransport {
    pub fn new(settings: &MailSettings, auth: TransportAuth) -> Result<Self, MailError> {
        let smtp = &settings.smtp;
        if smtp.host.trim().is_empty() {
            return Err(MailError::Connect("MAIL_SMTP_HOST is empty".into()));
        }
        if smtp.from_address.trim().is_empty() {
            return Err(MailError::Rejected(
                "MAIL_SMTP_FROM_ADDRESS is empty".into(),
            ));
        }
        let tls = tls_mode(smtp.tls, &smtp.host)?;
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
            .port(smtp.port)
            .credentials(auth.credentials()?)
            .tls(tls)
            .build();
        Ok(Self {
            transport,
            from_address: smtp.from_address.clone(),
            from_name: smtp.from_name.clone(),
        })
    }

    /// Send without re-checking policy. The caller runs the guard rails and
    /// records the decision on the draft; evaluating again here could report a
    /// different verdict from the one already audited.
    pub async fn send_raw(&self, message: &OutboundMessage) -> Result<String, MailError> {
        let rendered = render::build(message, &self.from_address, &self.from_name)?;
        let response = self
            .transport
            .send(rendered)
            .await
            .map_err(map_smtp_error)?;
        if !response.is_positive() {
            return Err(MailError::Rejected(format!(
                "smtp {}: {}",
                response.code(),
                response.message().collect::<Vec<_>>().join(" ")
            )));
        }
        Ok(response
            .message()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string())
    }

    pub fn from_address(&self) -> &str {
        &self.from_address
    }

    pub fn from_name(&self) -> &str {
        &self.from_name
    }

    pub async fn health(&self) -> ChannelHealth {
        ChannelHealth {
            connected: true,
            detail: Some(format!("smtp {}", self.from_address)),
            last_success_at: None,
        }
    }
}

fn tls_mode(mode: TlsMode, host: &str) -> Result<Tls, MailError> {
    let params = || {
        TlsParameters::new(host.to_string())
            .map_err(|e| MailError::Connect(format!("smtp tls parameters: {e}")))
    };
    match mode {
        TlsMode::Implicit => Ok(Tls::Wrapper(params()?)),
        // `Required`, not `Opportunistic`: opportunistic silently downgrades to
        // plaintext when a server omits STARTTLS, which would put credentials
        // and customer content on the wire in the clear.
        TlsMode::StartTls => Ok(Tls::Required(params()?)),
        // Plaintext relay, reachable only outside production, where
        // `validate_mail` refuses it.
        TlsMode::None => Ok(Tls::None),
    }
}

fn map_smtp_error(error: lettre::transport::smtp::Error) -> MailError {
    if error.is_tls() {
        return MailError::Connect(format!("smtp tls failure: {error}"));
    }
    // A transient or timed-out failure will succeed on retry; a permanent one
    // will be rejected identically every time, so it must not be requeued.
    if error.is_transient() || error.is_timeout() || error.is_transport_shutdown() {
        return MailError::Unavailable(format!("smtp temporarily unavailable: {error}"));
    }
    MailError::Rejected(format!("smtp rejected the message: {error}"))
}

#[cfg(test)]
#[path = "smtp_test.rs"]
mod tests;
