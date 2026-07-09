//! Process watchdog for the ingest pipeline.
//!
//! Watches the `DbWriter` heartbeat (stamped on each committed batch) and
//! force-exits if the DB queue has pending work but no commit arrives within the
//! configured timeout — indicating a wedged downstream (hung DB socket, etc.).
//!
//! Runs on a dedicated OS thread (not a tokio task) so a starved runtime cannot
//! freeze the watchdog alongside the thing it's watching.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::error;

use trading_core::storage::repositories::settings_repo::AppSettings;

pub const WATCHDOG_CHECK_INTERVAL_FLOOR_SECS: u64 =
    trading_core::config::constants::WATCHDOG_CHECK_INTERVAL_FLOOR_SECS;
pub const WATCHDOG_STALL_TIMEOUT_FLOOR_SECS: u64 =
    trading_core::config::constants::WATCHDOG_STALL_TIMEOUT_FLOOR_SECS;

/// Shared atomic stamp: unix-millis of the last DbWriter batch commit.
#[derive(Clone)]
pub struct DbHeartbeat(Arc<AtomicU64>);

impl DbHeartbeat {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(now_millis())))
    }

    /// Stamp — called by the `DbWriter` at the end of each committed flush.
    #[inline]
    pub fn stamp(&self) {
        self.0.store(now_millis(), Ordering::Relaxed);
    }

    pub fn idle_millis(&self) -> u64 {
        now_millis().saturating_sub(self.0.load(Ordering::Relaxed))
    }
}

impl Default for DbHeartbeat {
    fn default() -> Self { Self::new() }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_stalled(enabled: bool, live: bool, work_pending: bool, idle: Duration, timeout: Duration) -> bool {
    enabled && live && work_pending && idle >= timeout
}

/// Spawn the watchdog on a dedicated OS thread.
///
/// `work_pending` closure: returns true when the DB queue is non-empty
/// (typically a `WeakSender::upgrade().map(|tx| tx.capacity() < tx.max_capacity())`).
pub fn spawn_watchdog(
    heartbeat: DbHeartbeat,
    live_rx: watch::Receiver<bool>,
    settings_rx: watch::Receiver<AppSettings>,
    work_pending: impl Fn() -> bool + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ingest-watchdog".into())
        .spawn(move || {
            let mut prev_live = *live_rx.borrow();
            loop {
                let (enabled, timeout, check_interval) = {
                    let s = settings_rx.borrow();
                    (
                        s.watchdog_enabled,
                        Duration::from_secs(
                            s.watchdog_stall_timeout_secs.max(WATCHDOG_STALL_TIMEOUT_FLOOR_SECS),
                        ),
                        Duration::from_secs(
                            s.watchdog_check_interval_secs.max(WATCHDOG_CHECK_INTERVAL_FLOOR_SECS),
                        ),
                    )
                };
                std::thread::sleep(check_interval);

                let live = *live_rx.borrow();
                if live && !prev_live {
                    heartbeat.stamp(); // resumed — reset window
                }
                prev_live = live;

                let idle = Duration::from_millis(heartbeat.idle_millis());
                if is_stalled(enabled, live, work_pending(), idle, timeout) {
                    error!(
                        "Ingest watchdog: DB queue backed up with no commit for {idle:?} \
                         (>= {timeout:?}) while live — downstream wedged; forcing process exit"
                    );
                    std::process::exit(1);
                }
            }
        })
        .expect("spawn ingest-watchdog thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn paused_never_stalls() {
        assert!(!is_stalled(true, false, true, Duration::from_secs(10_000), Duration::from_secs(120)));
    }
    #[test] fn disabled_never_stalls() {
        assert!(!is_stalled(false, true, true, Duration::from_secs(10_000), Duration::from_secs(120)));
    }
    #[test] fn idle_queue_never_stalls() {
        assert!(!is_stalled(true, true, false, Duration::from_secs(10_000), Duration::from_secs(120)));
    }
    #[test] fn live_backed_up_stalls() {
        assert!(is_stalled(true, true, true, Duration::from_secs(121), Duration::from_secs(120)));
    }
}
