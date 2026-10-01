use std::sync::Arc;

use tokio::sync::Mutex;

use crate::domain::{ChannelHealth, InboundMessage, OutboundMessage};
use crate::error::MailError;

use super::MailProvider;

/// In-memory provider used for development and tests.
///
/// Reads a corpus of `.eml`/`.txt` files from disk once, then serves each
/// message exactly once: a message already returned by [`MailProvider::fetch_new`]
/// is not returned again, which makes repeated poll cycles safe and keeps the
/// idempotency of the real provider honest in tests.
///
/// Sends are recorded in memory and never leave the process. That is the whole
/// point: `EMAIL_MODE=auto` against the mock still cannot mail a customer.
pub struct MockMailProvider {
    corpus: Arc<Mutex<Vec<InboundMessage>>>,
    pending: Arc<Mutex<Vec<InboundMessage>>>,
    sent: Arc<Mutex<Vec<SentRecord>>>,
    fail_next: Arc<Mutex<Option<MailError>>>,
    corpus_dir: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SentRecord {
    pub subject: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub in_reply_to: Option<String>,
    pub body_length: usize,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

impl MockMailProvider {
    pub fn new() -> Self {
        Self {
            corpus: Arc::new(Mutex::new(Vec::new())),
            pending: Arc::new(Mutex::new(Vec::new())),
            sent: Arc::new(Mutex::new(Vec::new())),
            fail_next: Arc::new(Mutex::new(None)),
            corpus_dir: String::new(),
        }
    }

    /// Seed with messages the caller supplies directly.
    pub fn with_messages(messages: Vec<InboundMessage>) -> Self {
        Self {
            corpus: Arc::new(Mutex::new(messages.clone())),
            pending: Arc::new(Mutex::new(messages)),
            sent: Arc::new(Mutex::new(Vec::new())),
            fail_next: Arc::new(Mutex::new(None)),
            corpus_dir: String::new(),
        }
    }

    pub fn from_corpus_dir(dir: String) -> Self {
        Self {
            corpus_dir: dir,
            ..Self::new()
        }
    }

    /// Load the corpus from disk. A missing directory is not an error: a fresh
    /// checkout simply starts with an empty mailbox.
    pub async fn load(&self) -> Result<usize, MailError> {
        if self.corpus_dir.trim().is_empty() {
            return Ok(0);
        }
        let dir = std::path::PathBuf::from(&self.corpus_dir);
        if !dir.is_dir() {
            tracing::info!(dir = %self.corpus_dir, "mock corpus directory absent, mailbox is empty");
            return Ok(0);
        }
        let mut loaded = Vec::new();
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| MailError::Unavailable(format!("read corpus dir: {e}")))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| MailError::Unavailable(format!("walk corpus dir: {e}")))?
        {
            let path = entry.path();
            let is_mail = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "eml" | "txt" | "msg"));
            if !is_mail {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("message")
                .to_string();
            let raw = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| MailError::Protocol(format!("read {}: {e}", path.display())))?;
            loaded.push(parse_corpus_entry(&name, &raw));
        }
        // Oldest first, so a fixture that replays a conversation feeds the
        // turns in the order they were actually sent.
        loaded.sort_by_key(|m| m.date);
        let count = loaded.len();
        *self.corpus.lock().await = loaded.clone();
        *self.pending.lock().await = loaded;
        Ok(count)
    }

    /// Make the next `send` fail, so retry and dead-letter paths are testable.
    pub async fn fail_next_send(&self, error: MailError) {
        *self.fail_next.lock().await = Some(error);
    }

    /// Everything the mock was asked to send. Used by tests and the dry-run
    /// inspection endpoint.
    pub async fn sent_messages(&self) -> Vec<SentRecord> {
        self.sent.lock().await.clone()
    }

    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }

    pub async fn corpus_len(&self) -> usize {
        self.corpus.lock().await.len()
    }
}

impl Default for MockMailProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl MailProvider for MockMailProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn init(&self) -> Result<(), MailError> {
        self.load().await.map(|_| ())
    }

    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, MailError> {
        // Drain rather than peek: a poll cycle hands each message on once, so
        // the pipeline's own dedup key is what guarantees no double processing
        // when a run crashes and the message is re-read from the database.
        let mut pending = self.pending.lock().await;
        Ok(std::mem::take(&mut *pending))
    }

    async fn send(&self, message: &OutboundMessage) -> Result<String, MailError> {
        if let Some(error) = self.fail_next.lock().await.take() {
            return Err(error);
        }
        if message.to.is_empty() {
            return Err(MailError::Rejected("no recipients".into()));
        }
        self.sent.lock().await.push(SentRecord {
            subject: message.subject.clone(),
            to: message.to.iter().map(|a| a.address.clone()).collect(),
            cc: message.cc.iter().map(|a| a.address.clone()).collect(),
            in_reply_to: message.in_reply_to.clone(),
            body_length: message.text_body.len(),
            recorded_at: chrono::Utc::now(),
        });
        Ok(format!("mock-{}", uuid::Uuid::new_v4()))
    }

    async fn health(&self) -> ChannelHealth {
        ChannelHealth {
            connected: true,
            detail: Some(format!(
                "mock provider, {} queued",
                self.pending_count().await
            )),
            last_success_at: Some(chrono::Utc::now()),
        }
    }
}

/// Parse a corpus file into a message.
///
/// Corpus files are plain text in a small, documented format rather than full
/// RFC 5322: the fixture's job is to be readable in a diff, and the real
/// parsing path is exercised against a real server. Missing fields default
/// sensibly rather than failing the load.
fn parse_corpus_entry(name: &str, raw: &str) -> InboundMessage {
    use crate::domain::EmailAddress;
    // A fixture with no blank line has no header block at all, so the whole
    // file is the body. Treating it as headers would drop the text entirely.
    let (headers, body) = match raw.split_once("\n\n") {
        Some((h, b)) => (h, b),
        None => ("", raw),
    };
    let mut from = "unknown@example.invalid".to_string();
    let mut subject = name.to_string();
    let mut date = None;
    let mut message_id = None;
    let mut to = Vec::new();
    for line in headers.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "from" => from = value.to_string(),
            "to" => to = extract_addresses(value),
            "subject" => subject = value.to_string(),
            "message-id" => message_id = Some(value.to_string()),
            "date" => {
                date = chrono::DateTime::parse_from_rfc2822(value)
                    .ok()
                    .map(|d| d.with_timezone(&chrono::Utc))
            }
            _ => {}
        }
    }
    let from_name = from
        .split('<')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let from_addr = from
        .rsplit('<')
        .next()
        .unwrap_or(&from)
        .trim_end_matches('>')
        .trim()
        .to_string();
    let provider_id = format!("mock:{name}");
    InboundMessage {
        provider_message_id: provider_id.clone(),
        internet_message_id: message_id,
        in_reply_to: None,
        references: vec![],
        from: EmailAddress::with_name(from_addr, from_name.map(str::to_string)),
        to: to.into_iter().map(EmailAddress::new).collect(),
        cc: vec![],
        subject,
        date,
        text_body: body.trim().to_string(),
        attachments: vec![],
        total_size: raw.len(),
    }
}

fn extract_addresses(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.rsplit('<')
                .next()
                .unwrap_or(s)
                .trim_end_matches('>')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::EmailAddress;

    fn sample() -> InboundMessage {
        InboundMessage {
            provider_message_id: "pmid-1".into(),
            internet_message_id: Some("<a@example.com>".into()),
            in_reply_to: None,
            references: vec![],
            from: EmailAddress::new("customer@example.com"),
            to: vec![EmailAddress::new("inbox@mca.example")],
            cc: vec![],
            subject: "Need a quote".into(),
            date: None,
            text_body: "please quote".into(),
            attachments: vec![],
            total_size: 12,
        }
    }

    #[tokio::test]
    async fn each_message_is_fetched_once() {
        let provider = MockMailProvider::with_messages(vec![sample()]);
        let first = provider.fetch_new().await.expect("first poll");
        assert_eq!(first.len(), 1);
        let second = provider.fetch_new().await.expect("second poll");
        assert!(second.is_empty(), "a drained message must not reappear");
    }

    #[tokio::test]
    async fn send_is_recorded_and_returns_an_id() {
        let provider = MockMailProvider::new();
        let message = OutboundMessage::plain(
            EmailAddress::new("customer@example.com"),
            "Re: quote",
            "here you go",
        );
        let id = provider.send(&message).await.expect("send");
        assert!(id.starts_with("mock-"));
        let sent = provider.sent_messages().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].subject, "Re: quote");
    }

    #[tokio::test]
    async fn injected_failure_propagates_once() {
        let provider = MockMailProvider::new();
        provider
            .fail_next_send(MailError::Unavailable("smtp down".into()))
            .await;
        let message = OutboundMessage::plain(EmailAddress::new("customer@example.com"), "s", "b");
        assert!(provider.send(&message).await.is_err());
        // The failure is consumed, so the retry path can succeed.
        assert!(provider.send(&message).await.is_ok());
    }

    #[tokio::test]
    async fn a_message_without_recipients_is_rejected() {
        let provider = MockMailProvider::new();
        let mut message = OutboundMessage::plain(EmailAddress::new("a@b.c"), "s", "b");
        message.to.clear();
        assert!(matches!(
            provider.send(&message).await,
            Err(MailError::Rejected(_))
        ));
    }

    #[test]
    fn corpus_entry_parsing_extracts_headers() {
        let raw = "From: Ivan Petrov <ivan@example.com>\nTo: mca@example.com\n\
                   Subject: Freight quote\nDate: Mon, 5 Jan 2026 10:00:00 +0000\n\
                   \nHello, please quote.";
        let msg = parse_corpus_entry("fixture.eml", raw);
        assert_eq!(msg.from.address, "ivan@example.com");
        assert_eq!(msg.from.name.as_deref(), Some("Ivan Petrov"));
        assert_eq!(msg.subject, "Freight quote");
        assert_eq!(msg.text_body, "Hello, please quote.");
        assert_eq!(msg.to.len(), 1);
        assert!(msg.date.is_some());
    }

    #[test]
    fn corpus_entry_without_headers_still_loads() {
        let msg = parse_corpus_entry("bare.txt", "just a body");
        assert_eq!(msg.text_body, "just a body");
        assert_eq!(msg.subject, "bare.txt");
        assert!(!msg.from.address.is_empty());
    }
}
