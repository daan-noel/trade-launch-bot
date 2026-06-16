//! Shared progress snapshot for long-running background jobs (grouped sweep,
//! rule simulation).
//!
//! SSE only delivers *future* frames, so a dashboard that mounts mid-run — or
//! reconnects after a page refresh — has no way to learn the current
//! `processed / total` of an in-flight job. This cell is the readable snapshot
//! the `/api/jobs/status` endpoint serves for that recovery: the progress
//! reporter ([`SweepProgress`]/[`SimProgress`]) writes the same counts here that
//! it broadcasts over SSE, and the status handler reads them back.
//!
//! [`SweepProgress`]: crate::sweep::progress::SweepProgress
//! [`SimProgress`]: crate::strategies::sim_progress::SimProgress

use std::sync::atomic::{AtomicU64, Ordering};

/// A single job's live `processed / total` token counts. `total == 0` means the
/// total isn't known yet (the run is still partitioning / selecting candidates),
/// so the client shows an indeterminate bar.
#[derive(Default)]
pub struct ProgressCell {
    processed: AtomicU64,
    total: AtomicU64,
}

impl ProgressCell {
    /// Declare the run's total work unit (known once after candidate selection).
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// Record the latest processed count (overwrite, not increment — the reporter
    /// already holds the authoritative running counter).
    pub fn set_processed(&self, processed: u64) {
        self.processed.store(processed, Ordering::Relaxed);
    }

    /// Reset to `0 / 0` — called when a job ends so a stale snapshot can't make a
    /// finished job look in-flight.
    pub fn reset(&self) {
        self.processed.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    /// `(processed, total)` snapshot for the status endpoint.
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.processed.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }
}
