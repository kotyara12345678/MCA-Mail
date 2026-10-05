//! Read-only IMAP smoke test: proves the agent can reach the real mailbox and
//! read messages, without mutating anything.
//!
//! Uses exactly the code path the poll worker uses: `AppConfig::load` →
//! `mail::build` → `MailProvider::init` (capabilities + folder list) →
//! `MailProvider::fetch_new` (`EXAMINE` + `BODY.PEEK[]`). Under
//! `MAIL_MODE=read_only` the session is server-side read-only, `\Seen` is never
//! set and quarantine attempts are refused by the guard.
//!
//! Run from the repository root (so `.env` is picked up):
//!
//! ```bash
//! cargo run --example imap_smoke
//! ```

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    mca_mail::observability::init();

    let config = mca_mail::AppConfig::load()?;
    println!("== IMAP smoke test (read-only) ==");
    println!("provider : {:?}", config.mail.provider);
    println!("mode     : {}", config.mail.mode);
    println!(
        "host     : {}:{}",
        config.mail.imap.host, config.mail.imap.port
    );
    println!("inbox    : {}", config.mail.imap.inbox);

    // Same startup gate the server uses: refuses missing credentials, plaintext
    // IMAP, mock-in-production, etc. Never echoes a secret.
    config.validate()?;
    println!("config   : validation OK");

    let provider = mca_mail::mail::build(&config.mail)?;
    println!("built    : {} transport", provider.name());

    // Capabilities + folder list — the first proof of a live session.
    provider.init().await?;
    println!("init     : session opened, capabilities + folders logged above");

    // Read-only fetch: UNSEEN search + BODY.PEEK[] (no \Seen flag set).
    let started = std::time::Instant::now();
    let messages = provider.fetch_new().await?;
    println!(
        "fetch    : {} unseen message(s) in {:?}",
        messages.len(),
        started.elapsed()
    );

    for (i, message) in messages.iter().enumerate().take(10) {
        let from = match message.from.name.as_deref().filter(|n| !n.is_empty()) {
            Some(name) => format!("{name} <{}>", message.from.address),
            None => message.from.address.clone(),
        };
        let subject: String = message.subject.chars().take(80).collect();
        let body_chars = message.text_body.chars().count();
        println!(
            "  [{}] {} | {} | body {} chars | {}",
            i + 1,
            message.provider_message_id,
            from,
            body_chars,
            subject
        );
    }
    if messages.len() > 10 {
        println!("  ... and {} more", messages.len() - 10);
    }

    let health = provider.health().await;
    println!(
        "health   : connected={} detail={:?}",
        health.connected, health.detail
    );

    println!("== OK: agent connected and read the mailbox; no mutations attempted ==");
    Ok(())
}
