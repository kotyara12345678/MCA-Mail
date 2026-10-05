//! Safety-net polling that runs alongside the IDLE watcher.

use tokio::time::Duration;
use tracing::info;

use super::sync::MailSync;
use crate::shutdown::{pause, Rx};

/// Nudge the sync loop on a fixed period, whatever the watcher is doing.
///
/// IDLE is the fast path; this is the one that keeps mail moving if the fast
/// path fails *silently*. A dropped connection normally shows up as an error
/// and a reconnect, but a server that simply stops announcing `EXISTS` — or a
/// NAT entry that expires without a FIN — would otherwise stop mail forever
/// while every status check still reads "connected".
///
/// The period is deliberately long: in the normal case each tick only confirms
/// that nothing was missed.
pub async fn fallback_loop(sync: MailSync, period: Duration, mut shutdown: Rx) {
    info!(
        period_secs = period.as_secs(),
        "idle fallback poller started"
    );

    loop {
        if pause(&mut shutdown, period).await {
            break;
        }
        sync.notify();
    }
    info!("idle fallback poller stopped");
}
