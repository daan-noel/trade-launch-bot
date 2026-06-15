//! Live progress + cooperative cancellation for the grouped param-sweep.
//!
//! The sweep runs inside `spawn_blocking` on a bounded rayon pool and can fold
//! millions of (combo, token) outcomes, so — like the backtest's [`SimProgress`]
//! — it streams real `processed / total` token counts over an SSE event instead
//! of leaving the dashboard on a fake trickle bar. The engine layer
//! ([`engine`]/[`grouped_engine`]) stays strategy- and transport-agnostic by
//! talking only to the [`SweepObserver`] trait; the SSE-emitting [`SweepProgress`]
//! lives here and is wired in by the handler.
//!
//! Cancellation is cooperative: the handler flips an `AtomicBool` (via the cancel
//! endpoint) and the engine polls [`SweepObserver::cancelled`] between groups and
//! inside the per-token hot loop, bailing fast without an extra RPC/DB hit.
//!
//! [`engine`]: crate::sweep::engine
//! [`grouped_engine`]: crate::sweep::grouped_engine
//! [`SimProgress`]: crate::strategies::sim_progress::SimProgress

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::models::ingest::SseEvent;

/// Target number of progress frames over a full run; the per-tick throttle is
/// derived from this so a tiny corpus reports often and a huge one reports ~1%.
const STEPS: usize = 100;

/// What the engine needs from a progress sink — kept transport-agnostic so the
/// rayon hot loop never touches SSE/Tokio. `Sync` because the engine shares one
/// `&dyn SweepObserver` across rayon worker threads.
pub trait SweepObserver: Sync {
    /// Declare the run's total work unit (surviving tokens across all groups),
    /// known only after partitioning. Called once before sweeping begins.
    fn set_total(&self, total: usize);
    /// Mark one token folded (called once per token per group).
    fn token_done(&self);
    /// True once a cancel has been requested — polled in the hot loop.
    fn cancelled(&self) -> bool;
}

/// SSE-emitting observer for the grouped sweep. Broadcasts a throttled
/// [`SseEvent::SweepProgress`] frame as tokens fold so the dashboard renders a
/// real percentage, and surfaces the shared cancel flag to the engine.
pub struct SweepProgress {
    sse_tx: broadcast::Sender<SseEvent>,
    strategy_id: String,
    total: AtomicUsize,
    done: AtomicUsize,
    /// Cooperative cancel flag, shared with the cancel endpoint via app state.
    cancel: Arc<AtomicBool>,
}

impl SweepProgress {
    pub fn new(
        sse_tx: broadcast::Sender<SseEvent>,
        strategy_id: impl Into<String>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            sse_tx,
            strategy_id: strategy_id.into(),
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            cancel,
        }
    }

    fn send(&self, processed: usize) {
        let _ = self.sse_tx.send(SseEvent::SweepProgress {
            strategy_id: self.strategy_id.clone(),
            processed: processed as u64,
            total: self.total.load(Ordering::Relaxed) as u64,
        });
    }
}

impl SweepObserver for SweepProgress {
    fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        // Emit the initial `0 / total` frame so a subscriber can switch from
        // indeterminate to determinate before the first token folds.
        self.send(0);
    }

    fn token_done(&self) {
        let n = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        let total = self.total.load(Ordering::Relaxed);
        // Throttle to ~STEPS frames/run; always emit the final token.
        let step = (total / STEPS).max(1);
        if n == total || n.is_multiple_of(step) {
            self.send(n);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// No-op observer for tests / call sites that don't report progress.
#[cfg(test)]
pub struct NoopObserver;

#[cfg(test)]
impl SweepObserver for NoopObserver {
    fn set_total(&self, _total: usize) {}
    fn token_done(&self) {}
    fn cancelled(&self) -> bool {
        false
    }
}
