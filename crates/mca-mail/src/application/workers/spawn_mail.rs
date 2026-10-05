//! The loops that read the mailbox: the reader itself, the watcher that
//! announces new mail, the safety-net poller behind it, and the two loops that
//! process and retain what was read.

use std::sync::Arc;

use sqlx::PgPool;

use crate::config::AppConfig;
use crate::mail::idle::{Backoff, IdleSource};
use crate::mail::MailProvider;
use crate::orchestration::Orchestrator;

use super::{fallback, idle as idle_worker, queue, retention_loop, sync, WorkerSet};

pub(super) fn spawn_mail(
    config: &AppConfig,
    set: &mut WorkerSet,
    provider: Arc<dyn MailProvider>,
    pool: PgPool,
    orchestrator: Arc<Orchestrator>,
) {
    let shutdown = set.shutdown.subscribe();
    let (sync, trigger) = sync::sync_channel();
    let mailbox = config.mail.inbox().to_string();

    set.handles.push(tokio::spawn(sync::sync_loop(
        provider,
        pool.clone(),
        mailbox,
        config.mail.poll_interval_seconds,
        trigger,
        shutdown.clone(),
    )));

    if let Some(source) = crate::mail::idle::build(&config.mail) {
        spawn_watchers(config, set, source, sync, shutdown.clone());
    }

    set.handles.push(tokio::spawn(queue::queue_loop(
        pool.clone(),
        orchestrator,
        shutdown.clone(),
    )));

    if config.retention.enabled {
        set.handles.push(tokio::spawn(retention_loop(
            pool,
            config.retention.clone(),
            shutdown,
        )));
    }
}

/// The watcher and the safety-net poller that keeps it honest.
///
/// They are spawned together because they only make sense together: without
/// the poller a silently dead IDLE connection stops mail for ever, and without
/// the watcher the poller is just a slower version of the old timer.
fn spawn_watchers(
    config: &AppConfig,
    set: &mut WorkerSet,
    source: Arc<dyn IdleSource>,
    sync: sync::MailSync,
    shutdown: crate::shutdown::Rx,
) {
    let idle = &config.mail.imap.idle;
    let backoff = Backoff::new(
        idle.reconnect_min(),
        idle.reconnect_max(),
        idle.jitter_percent,
    );
    set.handles.push(tokio::spawn(idle_worker::idle_loop(
        source,
        sync.clone(),
        backoff,
        shutdown.clone(),
    )));
    set.handles.push(tokio::spawn(fallback::fallback_loop(
        sync,
        config.mail.idle_fallback_interval(),
        shutdown,
    )));
}
