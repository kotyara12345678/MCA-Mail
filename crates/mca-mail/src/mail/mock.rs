use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::MailMode;
use crate::domain::{ChannelHealth, InboundMessage, OutboundMessage};
use crate::error::MailError;

use super::guard::MailGuard;
use super::writer::MailboxWriter;
use super::{FetchBatch, MailProvider, UidState};

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
    guard: MailGuard,
    corpus: Arc<Mutex<Vec<InboundMessage>>>,
    pending: Arc<Mutex<Vec<QueuedMessage>>>,
    sent: Arc<Mutex<Vec<SentRecord>>>,
    fail_next: Arc<Mutex<Option<MailError>>>,
    /// One-shot fetch failure, so retry and resume paths are testable.
    fail_next_fetch: Arc<Mutex<Option<MailError>>>,
    /// UID the next arriving message receives; never reused.
    next_uid: Arc<AtomicU32>,
    /// UIDVALIDITY reported to the poller; bumped by tests to simulate the
    /// server re-binding the mailbox.
    uid_validity: Arc<AtomicU32>,
    /// Cap for one `fetch_after`, mirroring `MAIL_FETCH_BATCH_SIZE`.
    fetch_limit: Arc<AtomicUsize>,
    /// Counts every mailbox mutation the code under test actually attempted.
    ///
    /// `Arc` + `AtomicUsize` because the tests that read it live on another
    /// thread than the worker performing the operation, and a write attempt must
    /// be observable even when the guard refused it.
    mutations: Arc<MailboxMutationLog>,
    corpus_dir: String,
}

/// A queued message with the UID a server would have assigned it.
struct QueuedMessage {
    uid: u32,
    message: InboundMessage,
}

/// Per-operation attempt counters, used by tests to prove that a refused
/// operation never reached the transport.
#[derive(Debug, Default)]
pub struct MailboxMutationLog {
    pub send: AtomicUsize,
    pub append_draft: AtomicUsize,
    pub append_sent: AtomicUsize,
    pub move_message: AtomicUsize,
    pub copy_message: AtomicUsize,
    pub delete_message: AtomicUsize,
    pub set_flag: AtomicUsize,
    pub clear_flag: AtomicUsize,
    pub quarantine: AtomicUsize,
    pub mark_processed: AtomicUsize,
    /// Mutations that were allowed through and actually recorded.
    pub applied: AtomicUsize,
    /// Attempts refused by the read-only guard.
    pub refused: AtomicUsize,
}

impl MailboxMutationLog {
    pub fn total(&self) -> usize {
        self.send.load(Ordering::SeqCst)
            + self.append_draft.load(Ordering::SeqCst)
            + self.append_sent.load(Ordering::SeqCst)
            + self.move_message.load(Ordering::SeqCst)
            + self.copy_message.load(Ordering::SeqCst)
            + self.delete_message.load(Ordering::SeqCst)
            + self.set_flag.load(Ordering::SeqCst)
            + self.clear_flag.load(Ordering::SeqCst)
            + self.quarantine.load(Ordering::SeqCst)
            + self.mark_processed.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        for counter in [
            &self.send,
            &self.append_draft,
            &self.append_sent,
            &self.move_message,
            &self.copy_message,
            &self.delete_message,
            &self.set_flag,
            &self.clear_flag,
            &self.quarantine,
            &self.mark_processed,
            &self.applied,
            &self.refused,
        ] {
            counter.store(0, Ordering::SeqCst);
        }
    }
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
        Self::with_mode(MailMode::default())
    }

    /// A mock bound to an explicit mode, so read-only tests exercise the same
    /// guard the real transport uses rather than a mock-only shortcut.
    pub fn with_mode(mode: MailMode) -> Self {
        Self {
            guard: MailGuard::new(mode),
            corpus: Arc::new(Mutex::new(Vec::new())),
            pending: Arc::new(Mutex::new(Vec::new())),
            sent: Arc::new(Mutex::new(Vec::new())),
            fail_next: Arc::new(Mutex::new(None)),
            fail_next_fetch: Arc::new(Mutex::new(None)),
            next_uid: Arc::new(AtomicU32::new(1)),
            uid_validity: Arc::new(AtomicU32::new(1)),
            fetch_limit: Arc::new(AtomicUsize::new(50)),
            mutations: Arc::new(MailboxMutationLog::default()),
            corpus_dir: String::new(),
        }
    }

    /// Seed with messages the caller supplies directly.
    ///
    /// They receive UIDs `1..=n` in order — the way a server hands them out —
    /// and UIDNEXT becomes `n+1`, which is exactly the first-run boundary the
    /// poller records.
    pub fn with_messages(messages: Vec<InboundMessage>) -> Self {
        let queued = queue_all(&messages);
        let next_uid = messages.len() as u32 + 1;
        Self {
            corpus: Arc::new(Mutex::new(messages)),
            pending: Arc::new(Mutex::new(queued)),
            next_uid: Arc::new(AtomicU32::new(next_uid)),
            ..Self::new()
        }
    }

    pub fn from_corpus_dir(dir: String, mode: MailMode) -> Self {
        Self {
            corpus_dir: dir,
            ..Self::with_mode(mode)
        }
    }

    /// Per-operation counters, for assertions about what was attempted.
    pub fn mutation_log(&self) -> Arc<MailboxMutationLog> {
        Arc::clone(&self.mutations)
    }

    /// Ask the guard, recording a refusal when the mode forbids the operation.
    ///
    /// Counting here rather than inside [`MailGuard`] keeps the guard free of
    /// test instrumentation, while still giving every mutation one path that
    /// updates `refused` consistently.
    fn permitted(&self, op: super::ops::MailboxOp, subject: Option<&str>) -> Result<(), MailError> {
        match self.guard.check(op, subject) {
            Ok(()) => Ok(()),
            Err(refusal) => {
                self.mutations.refused.fetch_add(1, Ordering::SeqCst);
                Err(refusal)
            }
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
        let queued = queue_all(&loaded);
        self.next_uid.store(count as u32 + 1, Ordering::SeqCst);
        *self.corpus.lock().await = loaded;
        *self.pending.lock().await = queued;
        Ok(count)
    }

    /// Deliver a message as if it had just arrived: it gets the next UID and
    /// joins the queue. Returns the assigned UID.
    pub async fn enqueue(&self, message: InboundMessage) -> u32 {
        let uid = self.next_uid.fetch_add(1, Ordering::SeqCst);
        self.pending.lock().await.push(QueuedMessage {
            uid,
            message: message.clone(),
        });
        self.corpus.lock().await.push(message);
        uid
    }

    /// Simulate the server re-binding the mailbox (a UIDVALIDITY bump).
    pub fn set_uid_validity(&self, value: u32) {
        self.uid_validity.store(value, Ordering::SeqCst);
    }

    /// Cap what a single `fetch_after` may return, for batch-boundary tests.
    pub fn set_fetch_limit(&self, limit: usize) {
        self.fetch_limit.store(limit, Ordering::SeqCst);
    }

    /// Make the next fetch fail once, so retry paths are testable.
    pub async fn fail_next_fetch(&self, error: MailError) {
        *self.fail_next_fetch.lock().await = Some(error);
    }

    async fn take_fetch_failure(&self) -> Result<(), MailError> {
        match self.fail_next_fetch.lock().await.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
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

/// Fake mailbox mutations. Each one records the attempt, consults the guard and
/// only then "applies" — mirroring the real transport's ordering, so a test that
/// asserts `applied == 0` is asserting the real thing.
#[async_trait::async_trait]
impl MailboxWriter for MockMailProvider {
    fn guard(&self) -> MailGuard {
        self.guard
    }

    async fn send(&self, message: &OutboundMessage) -> Result<String, MailError> {
        self.mutations.send.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::Send, None)?;
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
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        Ok(format!("mock-{}", uuid::Uuid::new_v4()))
    }

    async fn append_draft(&self, message: &OutboundMessage) -> Result<(), MailError> {
        self.mutations.append_draft.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::AppendDraft, None)?;
        self.sent.lock().await.push(SentRecord {
            subject: message.subject.clone(),
            to: message.to.iter().map(|a| a.address.clone()).collect(),
            cc: vec![],
            in_reply_to: message.in_reply_to.clone(),
            body_length: message.text_body.len(),
            recorded_at: chrono::Utc::now(),
        });
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn append_sent(&self, _message: &OutboundMessage) -> Result<(), MailError> {
        self.mutations.append_sent.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::AppendSent, None)?;
        // Deliberately not pushed onto `sent`: that list is the record of
        // deliveries, and a copy of one is not a second delivery. The counter
        // is what a test reads to know the archive step ran.
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        tracing::debug!("mock append to the sent folder");
        Ok(())
    }

    async fn move_to_role(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        self.mutations.move_message.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::Move, Some(&uid.to_string()))?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(uid, role = role.as_str(), "mock move");
        Ok(())
    }

    async fn copy_to_role(
        &self,
        uid: u32,
        role: crate::mail::folders::FolderRole,
    ) -> Result<(), MailError> {
        self.mutations.copy_message.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::Copy, Some(&uid.to_string()))?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(uid, role = role.as_str(), "mock copy");
        Ok(())
    }

    async fn delete_message(&self, uid: u32) -> Result<(), MailError> {
        self.mutations.delete_message.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::Delete, Some(&uid.to_string()))?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn set_flag(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        self.mutations.set_flag.fetch_add(1, Ordering::SeqCst);
        self.guard
            .check_flag(super::ops::MailboxOp::SetFlag, Some(&uid.to_string()), flag)?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(uid, flag, "mock set flag");
        Ok(())
    }

    async fn clear_flag(&self, uid: u32, flag: &str) -> Result<(), MailError> {
        self.mutations.clear_flag.fetch_add(1, Ordering::SeqCst);
        self.guard.check_flag(
            super::ops::MailboxOp::ClearFlag,
            Some(&uid.to_string()),
            flag,
        )?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn quarantine(&self, uid: u32, reason: &str) -> Result<(), MailError> {
        self.mutations.quarantine.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::Move, Some(&uid.to_string()))?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(uid, reason, "mock quarantine");
        Ok(())
    }

    async fn mark_processed(&self, uid: u32) -> Result<(), MailError> {
        self.mutations.mark_processed.fetch_add(1, Ordering::SeqCst);
        self.permitted(super::ops::MailboxOp::SetFlag, Some(&uid.to_string()))?;
        self.mutations.applied.fetch_add(1, Ordering::SeqCst);
        Ok(())
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

    /// Fetch new messages, mirroring the real provider's per-mode semantics.
    ///
    /// Under `work` the queue is drained, matching the `\Seen` flag the real
    /// IMAP provider sets via `mark_processed`. Under read-only nothing may set
    /// that flag, so the real provider's `BODY.PEEK` fetch returns the same
    /// message again on the next poll. The mock must do the same, or a read-only
    /// test would "pass" only because the mock had quietly diverged from the
    /// transport it stands in for.
    async fn fetch_new(&self) -> Result<Vec<InboundMessage>, MailError> {
        self.take_fetch_failure().await?;
        let mut pending = self.pending.lock().await;
        if self.guard.allows_write() {
            return Ok(std::mem::take(&mut *pending)
                .into_iter()
                .map(|q| q.message)
                .collect());
        }
        Ok(pending.iter().map(|q| q.message.clone()).collect())
    }

    /// Server-side UID state: validity as set by the test, UIDNEXT as the next
    /// UID an arriving message would receive.
    async fn uid_state(&self) -> Result<UidState, MailError> {
        Ok(UidState {
            uid_validity: self.uid_validity.load(Ordering::SeqCst),
            uid_next: self.next_uid.load(Ordering::SeqCst),
        })
    }

    /// Watermark-bounded fetch, mirroring the real transport: queued messages
    /// with `uid > after_uid`, oldest first, capped by the batch limit.
    async fn fetch_after(&self, after_uid: u32) -> Result<FetchBatch, MailError> {
        self.take_fetch_failure().await?;
        let pending = self.pending.lock().await;
        let mut matched: Vec<&QueuedMessage> =
            pending.iter().filter(|q| q.uid > after_uid).collect();
        matched.sort_by_key(|q| q.uid);
        matched.truncate(self.fetch_limit.load(Ordering::SeqCst).max(1));
        let highest_uid = matched.last().map(|q| q.uid);
        Ok(FetchBatch {
            messages: matched.iter().map(|q| q.message.clone()).collect(),
            highest_uid,
        })
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

/// Assign UIDs `1..=n` in order, the way a server hands them out.
fn queue_all(messages: &[InboundMessage]) -> Vec<QueuedMessage> {
    messages
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, message)| QueuedMessage {
            uid: i as u32 + 1,
            message,
        })
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
    async fn each_message_is_fetched_once_in_work_mode() {
        let provider = MockMailProvider::with_mode(MailMode::Work);
        let provider = MockMailProvider {
            corpus: Arc::new(Mutex::new(vec![sample()])),
            pending: Arc::new(Mutex::new(vec![QueuedMessage {
                uid: 1,
                message: sample(),
            }])),
            next_uid: Arc::new(AtomicU32::new(2)),
            ..provider
        };
        let first = provider.fetch_new().await.expect("first poll");
        assert_eq!(first.len(), 1);
        let second = provider.fetch_new().await.expect("second poll");
        assert!(second.is_empty(), "a drained message must not reappear");
    }

    /// Read-only cannot set `\Seen`, so the message stays unseen and the next
    /// poll returns it again — the same thing `BODY.PEEK` does on a real server.
    /// Losing it here would mean a read-only deployment could skip mail.
    #[tokio::test]
    async fn read_only_re_reads_an_unflagged_message() {
        let provider = MockMailProvider::with_messages(vec![sample()]);
        assert_eq!(provider.fetch_new().await.expect("first").len(), 1);
        assert_eq!(
            provider.fetch_new().await.expect("second").len(),
            1,
            "read-only must not consume a message it cannot flag"
        );
    }

    #[tokio::test]
    async fn legacy_work_mode_reaches_the_transport_but_records_it() {
        let provider = MockMailProvider::with_mode(MailMode::Work);
        let message = OutboundMessage::plain(EmailAddress::new("customer@example.com"), "s", "b");
        // `work` opts the deployment into outbound mail at the transport guard;
        // whether it is actually sent is still `OutboundPolicyGuard`'s call.
        provider.send(&message).await.expect("transport accepts");
        assert_eq!(provider.sent_messages().await.len(), 1);
    }

    #[tokio::test]
    async fn read_only_mode_never_reaches_the_transport() {
        let provider = MockMailProvider::with_mode(MailMode::ReadOnly);
        let message = OutboundMessage::plain(EmailAddress::new("customer@example.com"), "s", "b");
        assert!(matches!(
            provider.send(&message).await,
            Err(MailError::OperationNotAllowed { .. })
        ));
        assert!(provider.sent_messages().await.is_empty());
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
