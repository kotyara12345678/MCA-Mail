//! TLS plumbing for IMAP.
//!
//! Split out from [`super::imap`] so the connection state machine stays
//! readable: this file knows how to obtain a byte stream, not what to do with
//! messages once it has one.

use std::pin::Pin;
use std::task::{Context, Poll};

use async_native_tls::{TlsConnector, TlsStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::config::{ImapSettings, TlsMode};
use crate::error::MailError;

/// A plain socket or a TLS one. `async-imap` is generic over the stream, so one
/// enum covers both configurations and the session type stays constant.
///
/// The `Tls` variant is much larger, which is why the TLS state is boxed: one
/// connection per mailbox, not per message, so the size does not matter.
pub enum ImapStreamKind {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl ImapStreamKind {
    /// Unwrap the socket, for a post-`STARTTLS` upgrade.
    ///
    /// Only a plaintext stream can be upgraded, so a `Tls` variant here means
    /// the session was already encrypted and a second `STARTTLS` was requested.
    pub fn into_inner(self) -> Result<TcpStream, MailError> {
        match self {
            ImapStreamKind::Plain(socket) => Ok(socket),
            ImapStreamKind::Tls(_) => Err(MailError::Protocol(
                "session is already encrypted; STARTTLS must not be repeated".into(),
            )),
        }
    }

    pub fn is_encrypted(&self) -> bool {
        matches!(self, ImapStreamKind::Tls(_))
    }
}

impl std::fmt::Debug for ImapStreamKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImapStreamKind::Plain(_) => f.write_str("ImapStream(plain)"),
            ImapStreamKind::Tls(_) => f.write_str("ImapStream(tls)"),
        }
    }
}

/// Open a socket to the IMAP host, wrapping it for implicit TLS.
///
/// For `StartTls` the returned stream is still plain: the caller must issue
/// `STARTTLS` and then call [`upgrade_to_tls`], because the protocol upgrade is
/// a command, not a connection option.
pub async fn connect(settings: &ImapSettings) -> Result<ImapStreamKind, MailError> {
    if settings.host.trim().is_empty() {
        return Err(MailError::Connect("MAIL_IMAP_HOST is empty".into()));
    }
    let address = format!("{}:{}", settings.host, settings.port);
    let socket = tokio::time::timeout(
        std::time::Duration::from_secs(settings.connect_timeout_seconds.max(1)),
        TcpStream::connect(&address),
    )
    .await
    .map_err(|_| MailError::Connect(format!("connect to {address} timed out")))?
    .map_err(|e| MailError::Connect(format!("connect to {address}: {e}")))?;
    let _ = socket.set_nodelay(true);

    if settings.tls == TlsMode::Implicit {
        wrap_tls(&address, socket, settings.allow_invalid_certs).await
    } else {
        Ok(ImapStreamKind::Plain(socket))
    }
}

/// Wrap an already-upgraded socket in TLS after a successful `STARTTLS`.
pub async fn upgrade_to_tls(
    settings: &ImapSettings,
    socket: TcpStream,
) -> Result<ImapStreamKind, MailError> {
    wrap_tls(
        &format!("{}:{}", settings.host, settings.port),
        socket,
        settings.allow_invalid_certs,
    )
    .await
}

async fn wrap_tls(
    address: &str,
    socket: TcpStream,
    allow_invalid_certs: bool,
) -> Result<ImapStreamKind, MailError> {
    let connector = if allow_invalid_certs {
        tracing::warn!(%address, "IMAP certificate validation disabled by configuration");
        TlsConnector::new().danger_accept_invalid_certs(true)
    } else {
        TlsConnector::new()
    };
    let tls = connector
        .connect(address, socket)
        .await
        .map_err(|e| MailError::Connect(format!("tls handshake with {address}: {e}")))?;
    Ok(ImapStreamKind::Tls(Box::new(tls)))
}

impl AsyncRead for ImapStreamKind {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStreamKind::Plain(socket) => Pin::new(socket).poll_read(cx, buf),
            ImapStreamKind::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ImapStreamKind {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ImapStreamKind::Plain(socket) => Pin::new(socket).poll_write(cx, buf),
            ImapStreamKind::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStreamKind::Plain(socket) => Pin::new(socket).poll_flush(cx),
            ImapStreamKind::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ImapStreamKind::Plain(socket) => Pin::new(socket).poll_shutdown(cx),
            ImapStreamKind::Tls(tls) => Pin::new(tls).poll_shutdown(cx),
        }
    }
}
