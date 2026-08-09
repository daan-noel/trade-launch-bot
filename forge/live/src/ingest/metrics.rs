//! Read-only health metrics for the ingest pipeline.
//!
//! The [`watchdog`](super::watchdog) already force-exits on a stall, but that's
//! invisible from outside until it fires. These counters expose the same signals
//! it watches — commit-heartbeat age, write-queue depth — plus a committed-event
//! total, so `GET /api/ingest` can show liveness/lag before the watchdog trips.
//!
//! Cheap and lock-free: an [`AtomicU64`] the `DbWriter` bumps per flush, the shared
//! [`DbHeartbeat`] atomic, and a `WeakSender` peek at channel occupancy (weak so
//! the metrics handle never keeps the channel alive past the consumer).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc::WeakSender;

use super::db_writer::{DbWriteOp, CHANNEL_CAPACITY};
use super::watchdog::DbHeartbeat;

/// Shared, cloneable handle to the ingest pipeline's live counters. Built by
/// `spawn_ingest`, cloned into the `DbWriter` (which bumps `events`), and exposed
/// to the HTTP layer via `app_data`.
#[derive(Clone)]
pub struct IngestMetrics {
    heartbeat: DbHeartbeat,
    /// Durable rows committed since boot (trades + tokens + raw_txs).
    events: Arc<AtomicU64>,
    /// Weak view of the consumer→writer channel, for the current buffer depth.
    queue: WeakSender<DbWriteOp>,
}

impl IngestMetrics {
    pub fn new(heartbeat: DbHeartbeat, queue: WeakSender<DbWriteOp>) -> Self {
        Self { heartbeat, events: Arc::new(AtomicU64::new(0)), queue }
    }

    /// The `AtomicU64` the `DbWriter` increments per flush.
    pub fn events_counter(&self) -> Arc<AtomicU64> {
        self.events.clone()
    }

    /// Millis since the last committed batch — the watchdog's stall signal. A
    /// healthy live stream keeps this small; a paused stream lets it grow (no
    /// writes is not a stall — the badge already shows `live: false`).
    pub fn commit_idle_millis(&self) -> u64 {
        self.heartbeat.idle_millis()
    }

    /// Total durable rows committed since boot.
    pub fn events_total(&self) -> u64 {
        self.events.load(Ordering::Relaxed)
    }

    /// Ops queued in the consumer→writer channel right now (`None` once the
    /// channel is gone, i.e. the consumer exited). A rising depth is the earliest
    /// sign the writer is falling behind the feed.
    pub fn buffer_depth(&self) -> Option<usize> {
        self.queue.upgrade().map(|s| s.max_capacity() - s.capacity())
    }

    /// The channel's fixed capacity (the depth's denominator).
    pub fn buffer_capacity(&self) -> usize {
        CHANNEL_CAPACITY
    }
}
