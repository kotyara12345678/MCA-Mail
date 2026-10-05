//! Starting every background loop, and — just as importantly — stopping them.

use std::sync::Arc;

use sqlx::PgPool;
use tracing::{error, info};

use crate::config::AppConfig;
use crate::mail::{MailProvider, MaybeWritable};
use crate::orchestration::Orchestrator;
use crate::shutdown::Signal;

use super::recovery::recover_stuck_emails;
use super::spawn_mail::spawn_mail;

/// The handles of everything running in the background.
///
/// Spawning and joining are deliberately the same object: a worker that cannot
/// be waited for is one that keeps writing to the log after the response has
/// gone out.
pub struct WorkerSet {
    pub(super) shutdown: Signal,
    pub(super) handles: Vec<tokio::task::JoinHandle<()>>,
}

impl WorkerSet {
    /// The same shutdown flag, for whoever learns about Ctrl-C first — in
    /// practice the HTTP server, which must stop accepting before the workers
    /// stop serving it.
    pub fn signal(&self) -> Signal {
        self.shutdown.clone()
    }

    /// Ask every worker to stop, then wait for them all to confirm it.
    pub async fn stop(self) {
        self.shutdown.stop();
        for handle in self.handles {
            if let Err(error) = handle.await {
                error!(%error, "background worker did not finish cleanly");
            }
        }
        info!("all background workers stopped");
    }
}

/// Start all background loops and return their handles.
///
/// The mailbox handle is built by the caller: it fails there, next to the
/// startup banner, rather than inside a task nobody is watching.
pub async fn spawn_all(
    config: &AppConfig,
    pool: PgPool,
    orchestrator: Arc<Orchestrator>,
    mailbox: Option<Arc<dyn MaybeWritable>>,
) -> WorkerSet {
    let (shutdown, _) = Signal::new();
    let mut set = WorkerSet {
        shutdown: shutdown.clone(),
        handles: Vec::new(),
    };

    if let Some(handle) = super::backup::spawn(
        &config.backup,
        &config.database.url,
        pool.clone(),
        shutdown.subscribe(),
    ) {
        set.handles.push(handle);
    }

    // Built writable so the send worker has somewhere to deliver to; the
    // reader only ever sees `dyn MailProvider`, which has no `send` on it.
    let Some(provider) = mailbox else {
        return set;
    };
    let reader: Arc<dyn MailProvider> = provider.clone();

    if let Err(e) = reader.init().await {
        error!(error = %e, "failed to initialise mail provider; poll loop disabled");
        return set;
    }
    info!(provider = %reader.name(), "mail provider initialised");

    if let Err(e) = recover_stuck_emails(&pool).await {
        error!(error = %e, "failed to recover interrupted emails");
    }

    spawn_mail(config, &mut set, reader, pool.clone(), orchestrator);

    set.handles.push(tokio::spawn(super::outbox::outbox_loop(
        pool,
        provider,
        config.clone(),
        shutdown.subscribe(),
    )));
    set
}
