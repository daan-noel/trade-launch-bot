//! Process watchdog for the ingest pipeline.
//!
//! Watches the `DbWriter` heartbeat (stamped only when a flush actually persists
//! a row) and force-exits if no successful write lands within the configured
//! timeout while ingest is live — covering BOTH failure modes that stalled the
//! feed for 7h on 2026-07-22: a wedged downstream (DB pool exhausted, every write
//! timing out) AND an upstream stall (transport dead, nothing arriving to write).
//!
//! It deliberately does NOT gate on "DB queue has pending work". That proxy had a
//! blind spot: an upstream stall drains the queue empty, so the old
//! `work_pending && stale` condition never tripped and the process sat alive but
//! dead for hours. The pump.fun feed is never quiet — live + zero successful
//! writes for the timeout window is unambiguously a fault, queue depth or not.
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

    /// Stamp — called by the `DbWriter` only after a flush that persisted at
    /// least one row. Stamping on an all-failed flush is a false liveness signal
    /// that hides a wedged pipeline from the watchdog (the 2026-07-22 root cause).
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

fn is_stalled(enabled: bool, live: bool, idle: Duration, timeout: Duration) -> bool {
    enabled && live && idle >= timeout
}

/// Spawn the watchdog on a dedicated OS thread.
pub fn spawn_watchdog(
    heartbeat: DbHeartbeat,
    live_rx: watch::Receiver<bool>,
    settings_rx: watch::Receiver<AppSettings>,
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
                if is_stalled(enabled, live, idle, timeout) {
                    error!(
                        "Ingest watchdog: no successful DB write for {idle:?} (>= {timeout:?}) \
                         while live — ingest wedged (feed stalled or DB pool exhausted); \
                         forcing process exit"
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
        // live=false (operator toggled ingest off) — a quiet feed is expected.
        assert!(!is_stalled(true, false, Duration::from_secs(10_000), Duration::from_secs(120)));
    }
    #[test] fn disabled_never_stalls() {
        assert!(!is_stalled(false, true, Duration::from_secs(10_000), Duration::from_secs(120)));
    }
    #[test] fn fresh_write_never_stalls() {
        // A recent successful write keeps idle below the timeout — healthy.
        assert!(!is_stalled(true, true, Duration::from_secs(30), Duration::from_secs(120)));
    }
    #[test] fn live_and_stale_stalls() {
        // Live, enabled, and no successful write past the timeout — fault, restart.
        // No longer gated on queue depth: this now catches an upstream stall too.
        assert!(is_stalled(true, true, Duration::from_secs(121), Duration::from_secs(120)));
    }
}
