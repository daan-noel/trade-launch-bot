//! Ingest health snapshot — published via atomics + a watch channel.
//!
//! The crate never calls `process::exit`. The host reads health via
//! `IngestHandle::health()` (cheap atomic snapshot) or
//! `IngestHandle::health_watch()` (watch receiver for a host watchdog).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::watch;

/// A point-in-time snapshot of ingest liveness counters.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    /// True while the gRPC stream is open and delivering updates.
    pub connected: bool,
    /// Unix-milliseconds of the last event emitted to the host.
    pub last_event_at_ms: u64,
    /// Last slot seen on the stream.
    pub last_slot: u64,
    /// Cumulative number of reconnect attempts.
    pub reconnects: u64,
    /// Total events emitted to the host.
    pub events_total: u64,
    /// Events dropped because the host's event channel was full.
    pub dropped_on_full: u64,
}

/// Shared atomic health state — written by the ingest tasks, read by the host.
pub(crate) struct HealthState {
    connected: AtomicBool,
    last_event_at_ms: AtomicU64,
    last_slot: AtomicU64,
    reconnects: AtomicU64,
    events_total: AtomicU64,
    dropped_on_full: AtomicU64,
    watch_tx: watch::Sender<HealthSnapshot>,
}

impl HealthState {
    pub(crate) fn new() -> (Arc<Self>, watch::Receiver<HealthSnapshot>) {
        let initial = HealthSnapshot {
            connected: false,
            last_event_at_ms: 0,
            last_slot: 0,
            reconnects: 0,
            events_total: 0,
            dropped_on_full: 0,
        };
        let (watch_tx, watch_rx) = watch::channel(initial);
        let state = Arc::new(Self {
            connected: AtomicBool::new(false),
            last_event_at_ms: AtomicU64::new(0),
            last_slot: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            events_total: AtomicU64::new(0),
            dropped_on_full: AtomicU64::new(0),
            watch_tx,
        });
        (state, watch_rx)
    }

    pub(crate) fn snapshot(&self) -> HealthSnapshot {
        HealthSnapshot {
            connected: self.connected.load(Ordering::Relaxed),
            last_event_at_ms: self.last_event_at_ms.load(Ordering::Relaxed),
            last_slot: self.last_slot.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
            events_total: self.events_total.load(Ordering::Relaxed),
            dropped_on_full: self.dropped_on_full.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn set_slot(&self, slot: u64) {
        self.last_slot.store(slot, Ordering::Relaxed);
    }

    pub(crate) fn on_event_emitted(&self) {
        self.events_total.fetch_add(1, Ordering::Relaxed);
        self.last_event_at_ms
            .store(now_millis(), Ordering::Relaxed);
        self.publish();
    }

    pub(crate) fn on_dropped(&self) {
        self.dropped_on_full.fetch_add(1, Ordering::Relaxed);
        self.publish();
    }

    fn publish(&self) {
        let _ = self.watch_tx.send(self.snapshot());
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
