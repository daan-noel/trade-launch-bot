//! Shared backtest progress reporter for the tpsl snipers.
//!
//! A simulation fetches + resolves every candidate token concurrently; the slow
//! part is the per-token `trades` round-trip. This emits an [`SseEvent::SimulationProgress`]
//! frame as each candidate completes so the dashboard can show real percentages
//! instead of a fake trickle bar. Frames are throttled to ~`STEPS` updates per run
//! (plus a guaranteed final `processed == total`) to keep the shared SSE channel
//! quiet — broadcasting one tiny frame per token would flood it for large candidate
//! sets. `tpsl_sniper_1` and `tpsl_sniper_2` are clones, so they share this.

use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::broadcast;
use uuid::Uuid;

use std::sync::Arc;

use crate::models::ingest::SseEvent;
use crate::state::job_progress::ProgressCell;

/// Target number of progress frames over a full run; the per-tick throttle is
/// derived from this so a 5-token run reports every token and a 50k-token run
/// reports ~every 1%.
const STEPS: usize = 100;

/// Per-run progress counter. Clone the `&` into each concurrent token future and
/// call [`SimProgress::tick`] exactly once when that future completes (success,
/// skip, or no-entry alike) so the count always reaches `total`.
pub struct SimProgress {
    sse_tx: broadcast::Sender<SseEvent>,
    rule_id: Uuid,
    total: usize,
    done: AtomicUsize,
    /// Emit only when `processed` is a multiple of this (or equals `total`).
    step: usize,
    /// Shared readable snapshot for `/api/jobs/status` (refresh recovery), written
    /// on every tick even when the SSE frame is throttled.
    cell: Arc<ProgressCell>,
}

impl SimProgress {
    pub fn new(
        sse_tx: broadcast::Sender<SseEvent>,
        rule_id: Uuid,
        total: usize,
        cell: Arc<ProgressCell>,
    ) -> Self {
        cell.set_total(total as u64);
        Self {
            sse_tx,
            rule_id,
            total,
            done: AtomicUsize::new(0),
            step: (total / STEPS).max(1),
            cell,
        }
    }

    /// Broadcast the initial `0 / total` frame so a subscriber learns the total
    /// (and can switch from indeterminate to determinate) before any token resolves.
    pub fn start(&self) {
        self.send(0);
    }

    /// Mark one candidate done; broadcast a throttled progress frame.
    pub fn tick(&self) {
        let n = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        self.cell.set_processed(n as u64);
        if n == self.total || n.is_multiple_of(self.step) {
            self.send(n);
        }
    }

    fn send(&self, processed: usize) {
        // The render bridge is always subscribed, so this never blocks on a full
        // channel for lack of readers; throttling keeps the frame count low.
        let _ = self.sse_tx.send(SseEvent::SimulationProgress {
            rule_id: self.rule_id,
            processed: processed as u64,
            total: self.total as u64,
        });
    }
}
