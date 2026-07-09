//! Sweep memory/timing observability (Phase 0.3).
//!
//! The memory plan judges **every** later phase against the Phase 0 baseline on
//! *two* axes — peak resident MB **and** total seconds — so a memory win that
//! silently regresses wall-clock is caught. This module is the shared, best-effort
//! instrument: process RSS (via the cross-platform `memory-stats` crate) and a
//! monotonic wall-clock, logged at each sweep milestone (admitted, corpus loaded,
//! partitioned, done). It never fails the sweep — an unreadable RSS just logs
//! `None`.

use std::time::Instant;

/// Resident-set size of this process in bytes, or `None` if the platform read
/// failed. Best-effort: callers log it, never branch on it for correctness.
pub fn process_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

/// Process RSS in whole MB (the unit the milestone logs report), or `None` when
/// unavailable. Used so a memory regression on the live box shows up directly in
/// the sweep logs without a profiler attached.
pub fn process_rss_mb() -> Option<u64> {
    process_rss_bytes().map(|b| b / (1024 * 1024))
}

/// A monotonic clock started at sweep admission. `elapsed_secs` is the wall-clock
/// since `start` — the second axis (with RSS) every milestone reports. `Copy` so
/// it threads cheaply through the handler without an `Arc`.
#[derive(Clone, Copy)]
pub struct SweepClock {
    start: Instant,
}

impl SweepClock {
    /// Start the clock — call once at admission so every milestone's `elapsed_s`
    /// is measured from the same origin.
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    /// Seconds elapsed since [`SweepClock::start`].
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

/// Log one sweep milestone with process RSS and elapsed-since-admission. Keeping
/// the field set identical across milestones makes the per-run RSS/seconds trace
/// trivially greppable (`milestone=corpus_loaded`).
pub fn log_milestone(clock: &SweepClock, milestone: &str) {
    tracing::info!(
        milestone,
        rss_mb = process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        "sweep: milestone"
    );
}
