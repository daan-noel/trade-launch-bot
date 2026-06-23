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

use crate::storage::repositories::settings_repo::AppSettings;

/// Floor for the watchdog stall window. Must stay above the worst-case upstream
/// recovery window so the watchdog never pre-empts an in-progress reconnect:
/// `client::STREAM_RECONNECT_IDLE_TIMEOUT` (10 s) + check cadence (~2 s) + max
/// backoff (~30 s) + connect (~10 s) = 52 s exactly. A hand-edited or stale DB
/// row below this is retroactively neutralized; the API clamps writes here and
/// the watchdog re-applies it defensively on every tick. Update this constant
/// whenever the reconnect timing constants in `client.rs` change.
pub const WATCHDOG_STALL_TIMEOUT_FLOOR_SECS: u64 = 52;
/// Floor for the watchdog check cadence — a `0`/tiny interval would busy-spin the
/// OS thread for no detection benefit.
pub const WATCHDOG_CHECK_INTERVAL_FLOOR_SECS: u64 = 5;

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
/// while the watchdog is enabled and live (a paused operator expects silence), and
/// once the idle gap reaches the timeout.
fn is_stalled(enabled: bool, live: bool, idle: Duration, timeout: Duration) -> bool {
    enabled && live && idle >= timeout
}

/// Spawn the liveness watchdog on a dedicated OS thread. Wakes every
/// `watchdog_check_interval_secs`; if the heartbeat has been stale for
/// `watchdog_stall_timeout_secs` while live mode is on (and the watchdog is
/// enabled), it logs and force-exits the process (exit code 1) so the supervisor
/// restarts a wedged ingest. Off the tokio runtime on purpose: a runtime starved by
/// the wedge we're detecting must not be able to freeze the watchdog too.
///
/// Reads its enable/timeout/cadence from `settings_rx` on every tick — a sync
/// `borrow()`, fine off the runtime — so the settings page can tune it live with no
/// restart. Floors are re-applied defensively so a hand-edited DB row can't make it
/// trigger-happy.
///
/// Gated on `live_rx`: while live mode is off the operator paused ingest on purpose,
/// so silence is expected and the watchdog holds fire. On the off→on edge it resets
/// the window so a freshly-resumed stream gets a full timeout to reconnect before it
/// can be judged stalled.
pub fn spawn_watchdog(
    heartbeat: IngestHeartbeat,
    live_rx: watch::Receiver<bool>,
    settings_rx: watch::Receiver<AppSettings>,
) {
    std::thread::Builder::new()
        .name("ingest-watchdog".into())
        .spawn(move || {
            let mut prev_live = *live_rx.borrow();
            loop {
                // Re-read the live config each tick so UI changes take effect
                // without a restart. Floors mirror the API-side clamp.
                let (enabled, timeout, check_interval) = {
                    let s = settings_rx.borrow();
                    (
                        s.watchdog_enabled,
                        Duration::from_secs(
                            s.watchdog_stall_timeout_secs
                                .max(WATCHDOG_STALL_TIMEOUT_FLOOR_SECS),
                        ),
                        Duration::from_secs(
                            s.watchdog_check_interval_secs
                                .max(WATCHDOG_CHECK_INTERVAL_FLOOR_SECS),
                        ),
                    )
                };
                std::thread::sleep(check_interval);

                let live = *live_rx.borrow();
                // Resumed since the last check — reset the window so the
                // reconnecting stream isn't killed before its first tx lands.
                if live && !prev_live {
                    heartbeat.stamp();
                }
                prev_live = live;

                let idle = Duration::from_millis(heartbeat.idle_millis());
                if is_stalled(enabled, live, idle, timeout) {
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
            true,
            false,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn disabled_is_never_a_stall() {
        // With the watchdog disabled, even a live + long-idle stall holds fire.
        assert!(!is_stalled(
            false,
            true,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_and_idle_past_timeout_stalls() {
        assert!(is_stalled(
            true,
            true,
            Duration::from_secs(121),
            Duration::from_secs(120)
        ));
        // Exactly at the threshold counts as stalled.
        assert!(is_stalled(
            true,
            true,
            Duration::from_secs(120),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_but_fresh_is_not_a_stall() {
        assert!(!is_stalled(
            true,
            true,
            Duration::from_secs(5),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn stall_floor_exceeds_upstream_recovery_window() {
        // Floor = idle timeout (10s) + check cadence (2s) + max backoff (30s) +
        // connect (10s) = 52s. Must be updated when client.rs timing constants change.
        assert!(WATCHDOG_STALL_TIMEOUT_FLOOR_SECS >= 52);
    }

    #[test]
    fn fresh_heartbeat_reports_small_idle() {
        let hb = IngestHeartbeat::new();
        assert!(hb.idle_millis() < 5_000);
        hb.stamp();
        assert!(hb.idle_millis() < 5_000);
    }
}
