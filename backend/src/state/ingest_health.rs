//! End-to-end ingest liveness heartbeat + process watchdog.
//!
//! The ingest chain (LaserStream client → pipeline → DbWriter / StrategyRunner) is
//! a series of bounded channels joined by `send().await`. If any downstream
//! consumer wedges on an unbounded `.await` (a hung DB socket, a stuck lock),
//! backpressure propagates up to the client's `update_tx.send().await` — which
//! parks the client task *outside* its `select!`, so the in-stream idle watchdog
//! can never fire. The task-supervision `select!` in `main` only catches a task
//! that returns/panics, never one deadlocked on an await. The result is a silent
//! freeze that only a manual restart clears (observed: recurring multi-hour total
//! ingest stalls with no log).
//!
//! This module closes that gap. The client stamps [`IngestHeartbeat`] every time it
//! successfully hands a transaction to the pipeline (real forward progress). A
//! dedicated **OS thread** — not a tokio task, so a runtime starved by the very
//! wedge we're detecting can't also freeze the watchdog — checks the heartbeat age
//! and force-exits the process when it goes stale while live mode is on. `main`
//! already exits non-zero for the supervisor to restart, so a wedge self-heals in
//! ~one timeout window instead of hours-until-noticed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::error;

/// Shared progress clock: unix-millis of the last observed ingest forward progress.
/// Cloning shares the same atomic (it's an `Arc` bump).
#[derive(Clone)]
pub struct IngestHeartbeat(Arc<AtomicU64>);

impl IngestHeartbeat {
    /// Construct stamped to "now", so the first timeout window starts at creation.
    /// Build this immediately before spawning the producer task.
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(now_millis())))
    }

    /// Record forward progress — one relaxed store on the hot path. Called by the
    /// client on every transaction it successfully forwards to the pipeline.
    #[inline]
    pub fn stamp(&self) {
        self.0.store(now_millis(), Ordering::Relaxed);
    }

    /// Milliseconds since the last [`stamp`](Self::stamp). Saturates at 0 if the
    /// wall clock went backwards, which only ever shortens the measured idle —
    /// never manufactures a false stall.
    fn idle_millis(&self) -> u64 {
        now_millis().saturating_sub(self.0.load(Ordering::Relaxed))
    }
}

impl Default for IngestHeartbeat {
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

/// Pure stall predicate, factored out for unit testing: a stall is only declared
/// while live (a paused operator expects silence) and once the idle gap reaches the
/// timeout.
fn is_stalled(live: bool, idle: Duration, timeout: Duration) -> bool {
    live && idle >= timeout
}

/// Spawn the liveness watchdog on a dedicated OS thread. Wakes every
/// `check_interval`; if the heartbeat has been stale for `timeout` while live mode
/// is on, it logs and force-exits the process (exit code 1) so the supervisor
/// restarts a wedged ingest. Off the tokio runtime on purpose: a runtime starved by
/// the wedge we're detecting must not be able to freeze the watchdog too.
///
/// Gated on `live_rx`: while live mode is off the operator paused ingest on purpose,
/// so silence is expected and the watchdog holds fire. On the off→on edge it resets
/// the window so a freshly-resumed stream gets a full timeout to reconnect before it
/// can be judged stalled.
pub fn spawn_watchdog(
    heartbeat: IngestHeartbeat,
    live_rx: watch::Receiver<bool>,
    timeout: Duration,
    check_interval: Duration,
) {
    std::thread::Builder::new()
        .name("ingest-watchdog".into())
        .spawn(move || {
            let mut prev_live = *live_rx.borrow();
            loop {
                std::thread::sleep(check_interval);

                let live = *live_rx.borrow();
                // Resumed since the last check — reset the window so the
                // reconnecting stream isn't killed before its first tx lands.
                if live && !prev_live {
                    heartbeat.stamp();
                }
                prev_live = live;

                let idle = Duration::from_millis(heartbeat.idle_millis());
                if is_stalled(live, idle, timeout) {
                    error!(
                        "Ingest watchdog: no forward progress for {idle:?} (>= {timeout:?}) \
                         while live — forcing process exit so the supervisor restarts a wedged \
                         ingest"
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
    fn paused_is_never_a_stall() {
        // Even a huge idle gap can't trip the watchdog while live mode is off.
        assert!(!is_stalled(
            false,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_and_idle_past_timeout_stalls() {
        assert!(is_stalled(
            true,
            Duration::from_secs(121),
            Duration::from_secs(120)
        ));
        // Exactly at the threshold counts as stalled.
        assert!(is_stalled(
            true,
            Duration::from_secs(120),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_but_fresh_is_not_a_stall() {
        assert!(!is_stalled(
            true,
            Duration::from_secs(5),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn fresh_heartbeat_reports_small_idle() {
        let hb = IngestHeartbeat::new();
        assert!(hb.idle_millis() < 5_000);
        hb.stamp();
        assert!(hb.idle_millis() < 5_000);
    }
}
