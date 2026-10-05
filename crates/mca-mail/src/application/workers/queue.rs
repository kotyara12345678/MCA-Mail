//! Queue loop: claim pending emails and process them concurrently.

use std::sync::Arc;
use std::time::Instant;

use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

use crate::observability::queue;
use crate::orchestration::Orchestrator;
use crate::persistence::email_repo;
use crate::shutdown::{stopping, Rx};

/// Claim and process pending emails, `concurrency` at a time.
///
/// `claim_batch` flips rows to `processing` under `FOR UPDATE SKIP LOCKED`, so
/// several queue loops can run without double-processing, and `process_email`
/// accepts the already-claimed `processing` state.
pub async fn queue_loop(pool: PgPool, orchestrator: Arc<Orchestrator>, mut shutdown: Rx) {
    let concurrency: i64 = 4;
    let max_attempts: i32 = 3;
    let mut ticker = interval(Duration::from_secs(2));
    info!(concurrency, "queue loop started");

    loop {
        tokio::select! {
            _ = stopping(&mut shutdown) => break,
            _ = ticker.tick() => {}
        }
        let batch = match email_repo::claim_batch(&pool, concurrency, max_attempts).await {
            Ok(b) if b.is_empty() => continue,
            Ok(b) => b,
            Err(e) => {
                error!(error = %e, "claim_batch failed");
                continue;
            }
        };

        let size = batch.len() as i64;
        queue::batch_claimed(size);
        let started = Instant::now();

        let tasks = batch.into_iter().map(|row| {
            let orch = orchestrator.clone();
            async move {
                queue::worker_started(row.id);
                let worker_started = Instant::now();
                match orch.process_email(row.id).await {
                    Ok(()) => {
                        queue::worker_completed(
                            row.id,
                            worker_started.elapsed().as_millis() as u64,
                        );
                        true
                    }
                    Err(e) => {
                        queue::worker_failed(
                            row.id,
                            &e.to_string(),
                            worker_started.elapsed().as_millis() as u64,
                        );
                        false
                    }
                }
            }
        });
        let results = futures::future::join_all(tasks).await;
        let ok = results.iter().filter(|r| **r).count() as i64;
        queue::batch_completed(size, ok, size - ok, started.elapsed().as_millis() as u64);
    }
    info!("queue loop stopped");
}
