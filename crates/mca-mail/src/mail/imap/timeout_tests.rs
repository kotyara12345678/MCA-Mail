//! A server that stops answering must not be able to stop the mailbox.
//!
//! This is the production hang: `APPEND` reached the server, the tagged reply
//! never came back, and the session lock stayed held — so every later poll
//! queued behind it while `/health` kept reporting healthy. The contract here
//! is that a silent command fails within its deadline, drops the session, and
//! lets the next call reconnect.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::{
    ImapSettings, MailMode, MailProviderKind, MailSettings, Secret, SmtpSettings, TlsMode,
};
use crate::error::MailError;
use crate::mail::{ImapMailProvider, SmtpTransport, TransportAuth};

/// Commands the script answers. Everything else is swallowed whole, which is
/// exactly what a wedged connection looks like from the client's side.
const ANSWERED: [&str; 4] = ["LOGIN", "SELECT", "EXAMINE", "CAPABILITY"];

struct ScriptedServer {
    address: std::net::SocketAddr,
    connections: Arc<AtomicUsize>,
}

/// Greeting, login and mailbox selection succeed; the first real query hangs.
async fn start_server() -> ScriptedServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("local address");
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&connections);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(answer(stream));
        }
    });
    ScriptedServer {
        address,
        connections,
    }
}

async fn answer(mut stream: TcpStream) {
    if stream.write_all(b"* OK IMAP4rev1 ready\r\n").await.is_err() {
        return;
    }
    let mut pending = String::new();
    let mut chunk = [0u8; 4096];
    let mut silent = false;
    loop {
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        pending.push_str(&String::from_utf8_lossy(&chunk[..read]));
        while let Some(end) = pending.find("\r\n") {
            let line: String = pending.drain(..end + 2).collect();
            let mut words = line.split_whitespace();
            let tag = words.next().unwrap_or_default().to_string();
            let verb = words.next().unwrap_or_default().to_ascii_uppercase();
            if silent {
                continue;
            }
            if !ANSWERED.contains(&verb.as_str()) {
                // Keep the socket open and say nothing: the client is left
                // waiting for a tagged reply that will never arrive.
                silent = true;
                continue;
            }
            let reply = match verb.as_str() {
                "CAPABILITY" => format!("* CAPABILITY IMAP4rev1\r\n{tag} OK\r\n"),
                "EXAMINE" => format!(
                    "* FLAGS (\\Answered \\Deleted \\Seen \\Draft)\r\n\
                     * OK [PERMANENTFLAGS (\\Answered \\Deleted \\Seen \\Draft)] Limited\r\n\
                     * 0 EXISTS\r\n* 0 RECENT\r\n\
                     * OK [UIDVALIDITY 1] UIDs valid\r\n\
                     * OK [UIDNEXT 1] Predicted next UID\r\n\
                     {tag} OK [READ-ONLY] EXAMINE completed\r\n"
                ),
                "SELECT" => format!(
                    "* FLAGS (\\Answered \\Deleted \\Seen \\Draft)\r\n\
                     * OK [PERMANENTFLAGS (\\Answered \\Deleted \\Seen \\Draft)] Limited\r\n\
                     * 0 EXISTS\r\n* 0 RECENT\r\n\
                     * OK [UIDVALIDITY 1] UIDs valid\r\n\
                     * OK [UIDNEXT 1] Predicted next UID\r\n\
                     {tag} OK [READ-WRITE] SELECT completed\r\n"
                ),
                _ => format!("{tag} OK LOGIN completed\r\n"),
            };
            if stream.write_all(reply.as_bytes()).await.is_err() {
                return;
            }
        }
    }
}

fn settings(port: u16) -> MailSettings {
    MailSettings {
        mode: MailMode::Work,
        provider: MailProviderKind::Imap,
        username: "user".to_string(),
        password: Secret::new("pass"),
        imap: ImapSettings {
            host: "127.0.0.1".to_string(),
            port,
            tls: TlsMode::None,
            inbox: "INBOX".to_string(),
            connect_timeout_seconds: 5,
            command_timeout_seconds: 5,
            ..ImapSettings::default()
        },
        smtp: SmtpSettings {
            host: "127.0.0.1".to_string(),
            port: 1,
            tls: TlsMode::None,
            from_address: "agent@example.com".to_string(),
            ..SmtpSettings::default()
        },
        ..MailSettings::default()
    }
}

#[tokio::test]
async fn a_silent_command_times_out_instead_of_wedging_the_session() {
    let server = start_server().await;
    let settings = settings(server.address.port());
    let smtp = SmtpTransport::new(
        &settings,
        TransportAuth::new(settings.username.clone(), "pass".to_string()),
    )
    .expect("smtp transport");
    let provider = ImapMailProvider::new(&settings, smtp);

    let started = Instant::now();
    let first = provider.list_folders().await;
    let elapsed = started.elapsed();

    match first {
        Err(MailError::Unavailable(message)) => {
            assert!(message.contains("timed out"), "unexpected error: {message}");
        }
        Err(other) => panic!("expected a timeout, got {other}"),
        Ok(folders) => panic!("expected a timeout, got {} folders", folders.len()),
    }
    assert!(
        elapsed >= Duration::from_secs(4),
        "gave up after {elapsed:?}, before the command deadline"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "took {elapsed:?}, far past the command deadline"
    );

    // The wedged session must have been dropped: a call that finds it still
    // parked would reuse the same connection and never open a second one.
    let second = provider.list_folders().await;
    assert!(second.is_err(), "the silent server is still silent");
    assert_eq!(
        server.connections.load(Ordering::SeqCst),
        2,
        "the session held by the silent command must be replaced, not reused"
    );
}
