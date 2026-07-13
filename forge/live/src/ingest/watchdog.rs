//! Process watchdog for the ingest pipeline (ported from hunter).
//!
//! Watches the [`DbWriter`](super::db_writer::DbWriter) heartbeat (stamped on
//! each committed batch) and force-exits the process if the write queue has
//! pending work but no commit arrives within the stall timeout — the signature
//! of a wedged downstream (hung DB socket, saturated pool). The supervisor
//! restarts us and gap-replay re-fetches the missed slots, which is strictly
//! better than silently stalling ingest forever.
//!
//! Runs on a dedicated OS thread (not a tokio task) so a starved runtime cannot
//! freeze the watchdog alongside the thing it's watching.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing::error;

/// How often the watchdog wakes to check the heartbeat.
const CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// Queue-backed-up-with-no-commit window that trips a force-exit.
const STALL_TIMEOUT: Duration = Duration::from_secs(120);

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
    fn default() -> Self {
        Self::new()
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pure stall predicate (unit-testable). Only trips while the stream is live and
/// there is queued work that isn't being committed.
fn is_stalled(live: bool, work_pending: bool, idle: Duration, timeout: Duration) -> bool {
    live && work_pending && idle >= timeout
}

/// Spawn the watchdog on a dedicated OS thread.
///
/// - `is_live`: reads the ingest live/paused toggle (a paused stream never stalls
///   — it legitimately writes nothing).
/// - `work_pending`: true when the write queue is non-empty (typically a
///   `WeakSender::upgrade().map(|tx| tx.max_capacity() - tx.capacity() > 0)`); a
///   drained queue with no writes is idle, not stalled.
pub fn spawn_watchdog(
    heartbeat: DbHeartbeat,
    is_live: impl Fn() -> bool + Send + 'static,
    work_pending: impl Fn() -> bool + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ingest-watchdog".into())
        .spawn(move || {
            let mut prev_live = is_live();
            loop {
                std::thread::sleep(CHECK_INTERVAL);

                let live = is_live();
                if live && !prev_live {
                    heartbeat.stamp(); // resumed — reset the window
                }
                prev_live = live;

                let idle = Duration::from_millis(heartbeat.idle_millis());
                if is_stalled(live, work_pending(), idle, STALL_TIMEOUT) {
                    error!(
                        "Ingest watchdog: write queue backed up with no commit for {idle:?} \
                         (>= {STALL_TIMEOUT:?}) while live — downstream wedged; forcing process exit"
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

    #[test]
    fn paused_never_stalls() {
        assert!(!is_stalled(false, true, Duration::from_secs(10_000), STALL_TIMEOUT));
    }
    #[test]
    fn idle_queue_never_stalls() {
        assert!(!is_stalled(true, false, Duration::from_secs(10_000), STALL_TIMEOUT));
    }
    #[test]
    fn live_backed_up_stalls() {
        assert!(is_stalled(true, true, Duration::from_secs(121), STALL_TIMEOUT));
    }
    #[test]
    fn live_backed_up_within_window_ok() {
        assert!(!is_stalled(true, true, Duration::from_secs(10), STALL_TIMEOUT));
    }
}
