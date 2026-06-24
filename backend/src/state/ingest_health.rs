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
//! This module closes that gap. The **DbWriter** stamps [`IngestHeartbeat`] every
//! time it commits a batch to Postgres — *real* end-to-end progress, measured at
//! the sink where work actually lands, not where the client hands it off. That
//! distinction is the whole point: a downstream that's merely *slow* (overloaded
//! Postgres, recovering) keeps committing batches and so keeps stamping, while a
//! downstream that's truly *wedged* (a hung DB socket parked on an `.await`)
//! commits nothing and lets the heartbeat age. A dedicated **OS thread** — not a
//! tokio task, so a runtime starved by the very wedge we're detecting can't also
//! freeze the watchdog — checks the heartbeat age and force-exits the process when
//! it goes stale *while work is pending* (the DB queue is non-empty). `main`
//! already exits non-zero for the supervisor to restart, so a wedge self-heals in
//! ~one timeout window instead of hours-until-noticed.
//!
//! The "work pending" gate is what makes this precise: an idle stretch (quiet
//! upstream, or a reconnect in flight) drains the queue empty, so the absence of
//! commits is expected and never counted as a stall — only a *backed-up* queue
//! that isn't draining is a wedge. This replaces the older, fragile scheme where
//! the client stamped on every forward and on healthy reconnects (which couldn't
//! tell transient overload from a deadlock and restarted on both).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::error;

use crate::storage::repositories::settings_repo::AppSettings;

/// Floor for the watchdog stall window. Kept generous (180 s) because the
/// watchdog now only ever fires on a *genuine* downstream wedge — the stall
/// predicate is gated on "work is pending" (the DB queue is non-empty), so a
/// quiet upstream or an in-progress reconnect can never trip it regardless of the
/// window. With false positives designed out, a wide window costs nothing and a
/// hand-edited / stale DB row below it is retroactively neutralized (the API
/// clamps writes here and the watchdog re-applies it defensively every tick).
pub const WATCHDOG_STALL_TIMEOUT_FLOOR_SECS: u64 = 90;
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
/// while the watchdog is enabled and live (a paused operator expects silence),
/// while work is actually pending (a non-empty DB queue — an empty one means the
/// worker has nothing to drain, so no-commit silence is legitimate, not a wedge),
/// and once the idle gap reaches the timeout.
fn is_stalled(
    enabled: bool,
    live: bool,
    work_pending: bool,
    idle: Duration,
    timeout: Duration,
) -> bool {
    enabled && live && work_pending && idle >= timeout
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
///
/// `work_pending` reports whether the downstream has anything to drain (the DB
/// queue is non-empty). It's a plain closure — kept here rather than importing the
/// queue type — so this module stays decoupled from `ingest_laserstream`; `main`
/// builds it from a `WeakSender` to the DbWriter queue. A stall is only ever
/// declared while it returns `true`: no pending work means no-commit silence is
/// legitimate (quiet upstream / reconnect draining), not a wedge.
pub fn spawn_watchdog(
    heartbeat: IngestHeartbeat,
    live_rx: watch::Receiver<bool>,
    settings_rx: watch::Receiver<AppSettings>,
    work_pending: impl Fn() -> bool + Send + 'static,
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
                if is_stalled(enabled, live, work_pending(), idle, timeout) {
                    error!(
                        "Ingest watchdog: DB queue backed up with no commit for {idle:?} \
                         (>= {timeout:?}) while live — downstream wedged; forcing process exit \
                         so the supervisor restarts ingest"
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
        // Even a huge idle gap with work pending can't trip the watchdog while
        // live mode is off.
        assert!(!is_stalled(
            true,
            false,
            true,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn disabled_is_never_a_stall() {
        // With the watchdog disabled, even a live + work-pending + long-idle stall
        // holds fire.
        assert!(!is_stalled(
            false,
            true,
            true,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn idle_queue_is_never_a_stall() {
        // The crux of the design: no pending work means no-commit silence is
        // legitimate (quiet upstream / reconnect draining), never a wedge — even
        // live, enabled, and idle far past the timeout.
        assert!(!is_stalled(
            true,
            true,
            false,
            Duration::from_secs(10_000),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_with_work_pending_idle_past_timeout_stalls() {
        // The one case that restarts: live, enabled, work pending (queue backed
        // up), and no commit for >= the timeout = a genuine downstream wedge.
        assert!(is_stalled(
            true,
            true,
            true,
            Duration::from_secs(121),
            Duration::from_secs(120)
        ));
        // Exactly at the threshold counts as stalled.
        assert!(is_stalled(
            true,
            true,
            true,
            Duration::from_secs(120),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn live_but_fresh_is_not_a_stall() {
        // Work pending but a recent commit (fresh heartbeat) = a slow-but-draining
        // worker, not a wedge.
        assert!(!is_stalled(
            true,
            true,
            true,
            Duration::from_secs(5),
            Duration::from_secs(120)
        ));
    }

    #[test]
    fn stall_floor_is_generous() {
        // The watchdog is gated on work-pending, so only a complete freeze triggers
        // it. 90s floor gives enough room for slow-but-not-hung DB periods while
        // recovering faster than the old 180s floor when the DbWriter truly wedges.
        assert!(WATCHDOG_STALL_TIMEOUT_FLOOR_SECS >= 60);
    }

    #[test]
    fn fresh_heartbeat_reports_small_idle() {
        let hb = IngestHeartbeat::new();
        assert!(hb.idle_millis() < 5_000);
        hb.stamp();
        assert!(hb.idle_millis() < 5_000);
    }
}
