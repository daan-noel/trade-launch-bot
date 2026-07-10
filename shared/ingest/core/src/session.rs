//! The venue-generic ingest session: [`Ingest<V>`] + [`IngestHandle<V>`].
//!
//! `Ingest<V>` owns the transport + decode task wiring; a venue-specific builder
//! (e.g. the pump.fun façade's `IngestBuilder`) assembles the `V` and constructs
//! the session via [`Ingest::new`]. `start()` spawns the transport task
//! (`transport::run<V>`) and the decode task (which calls `V::decode`), returning
//! the host event receiver + a control handle.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Notify};
use tracing::warn;

/// Count of events dropped after the back-pressure retry ALSO failed — the only
/// genuinely-lost events (a `Full` that the 100ms retry couldn't drain, or a
/// closed channel). Process-wide (ingest is a single session); surfaced on every
/// real drop so sustained loss is visible instead of silent.
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

use crate::config::{Auth, Commitment, IngestConfig};
use crate::event::IngestEvent;
use crate::proto::geyser::{CommitmentLevel, SubscribeUpdateTransaction};
use crate::transport::{self, TransportConfig};
use crate::venue::{DecodeOutput, IngestVenue, PoolIndex};

/// Validated, ready-to-start ingest session, generic over the venue.
pub struct Ingest<V: IngestVenue> {
    endpoint: String,
    auth: Auth,
    venue: Arc<V>,
    config: IngestConfig,
}

impl<V: IngestVenue> Ingest<V> {
    /// Assemble a session. Venue-specific builders own provider/endpoint/config
    /// validation and the construction of `venue`.
    pub fn new(endpoint: String, auth: Auth, venue: Arc<V>, config: IngestConfig) -> Self {
        Self {
            endpoint,
            auth,
            venue,
            config,
        }
    }

    /// Spawn the transport + decode tasks and return the event receiver + handle.
    ///
    /// `live` — initial live-mode state (pass `true` to start streaming
    ///   immediately; `false` pauses until `handle.set_live(true)` is called).
    pub fn start(self, live: bool) -> (mpsc::Receiver<IngestEvent>, IngestHandle<V>) {
        let cfg = &self.config;
        let venue = self.venue.clone();

        // Internal transport→decode channel.
        let (update_tx, mut update_rx) = mpsc::channel::<(
            Arc<SubscribeUpdateTransaction>,
            V::Relevance,
            chrono::DateTime<chrono::Utc>,
        )>(cfg.update_channel_cap);

        // Host-facing event channel.
        let (event_tx, event_rx) = mpsc::channel::<IngestEvent>(cfg.event_channel_cap);

        // Live-mode gate.
        let (live_tx, live_rx) = watch::channel(live);

        // Transport config (extracted from IngestConfig).
        let transport_cfg = Arc::new(TransportConfig {
            connect_timeout: cfg.connect_timeout,
            reconnect_base: cfg.reconnect_base,
            reconnect_max_backoff: cfg.reconnect_max_backoff,
            idle_reconnect_timeout: cfg.idle_reconnect_timeout,
            idle_check_interval: cfg.idle_check_interval,
            http2_keepalive: cfg.http2_keepalive,
            tcp_keepalive: cfg.tcp_keepalive,
            max_decoding_message_size: cfg.max_decoding_message_size,
            pipeline_send_timeout: cfg.pipeline_send_timeout,
            resubscribe_debounce: cfg.resubscribe_debounce,
            commitment: commitment_level(cfg.commitment),
        });

        // Gap-replay config channel: host sends (gap_replay_on_reconnect,
        // gap_replay_max_window_secs) whenever the operator changes the settings.
        // Default: off (false, 300 s). The sender is exposed via IngestHandle so
        // the host can push updates without restarting ingest.
        let (gap_replay_tx, gap_replay_rx) = watch::channel::<(bool, u64)>((false, 300));

        // Spawn transport task.
        tokio::spawn(transport::run(
            self.endpoint.clone(),
            self.auth.clone(),
            venue.clone(),
            update_tx,
            live_rx,
            transport_cfg,
            gap_replay_rx,
        ));

        // Spawn decode task.
        let venue_decode = venue.clone();
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            while let Some((update, relevance, received_at)) = update_rx.recv().await {
                let output = venue_decode.decode(&update, relevance, received_at);

                let events = match output {
                    DecodeOutput::Events(v) => v,
                    DecodeOutput::Ignored => continue,
                };

                // Optionally append the raw-tx event (under feature gate).
                #[cfg(feature = "raw-tx")]
                let events = {
                    let mut v = events;
                    if let Some(raw_ev) = crate::raw_tx::build_raw_tx_event(&update, received_at) {
                        v.push(raw_ev);
                    }
                    v
                };

                for ev in events {
                    match event_tx_clone.try_send(ev) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(ev)) => {
                            // Channel briefly full — retry with a short timeout
                            // rather than logging a drop prematurely (the retry
                            // usually succeeds under transient back-pressure).
                            if event_tx_clone
                                .send_timeout(ev, std::time::Duration::from_millis(100))
                                .await
                                .is_err()
                            {
                                // Retry ALSO failed — this is a genuine drop. Count
                                // it and log the running total so sustained loss is
                                // visible instead of silent.
                                let n = DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                                warn!(
                                    dropped_total = n,
                                    "ingest: event channel full after retry — dropped event"
                                );
                            }
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            return; // Host dropped the receiver — stop.
                        }
                    }
                }
            }
        });

        let handle = IngestHandle {
            live_tx,
            gap_replay_tx,
            pool_index: venue.pool_index(),
            pools_changed: venue.pools_changed(),
            venue,
        };

        (event_rx, handle)
    }
}

// ── IngestHandle ──────────────────────────────────────────────────────────────

/// Control handle returned by [`Ingest::start`]. All methods are lock-free
/// (atomic writes or channel sends).
pub struct IngestHandle<V: IngestVenue> {
    live_tx: watch::Sender<bool>,
    /// Gap-replay settings: (gap_replay_on_reconnect, gap_replay_max_window_secs).
    /// Push a new value whenever the operator changes the settings; the transport
    /// picks it up on the next reconnect.
    gap_replay_tx: watch::Sender<(bool, u64)>,
    pool_index: PoolIndex,
    pools_changed: Arc<Notify>,
    venue: Arc<V>,
}

impl<V: IngestVenue> IngestHandle<V> {
    /// Pause (`false`) or resume (`true`) the transport stream.
    pub fn set_live(&self, live: bool) {
        let _ = self.live_tx.send(live);
    }

    /// Current live-mode state (`true` = streaming, `false` = paused).
    pub fn is_live(&self) -> bool {
        *self.live_tx.borrow()
    }

    /// Push updated gap-replay settings to the transport. Takes effect on the
    /// next reconnect; no-op if the transport task has already stopped.
    pub fn set_gap_replay(&self, on: bool, max_window_secs: u64) {
        let _ = self.gap_replay_tx.send((on, max_window_secs));
    }

    /// Register pool PDAs for the given mints and signal the transport to
    /// resubscribe. Idempotent — already-registered mints are skipped.
    pub fn track_pools(&self, mints: &[String]) {
        let mut added = false;
        for mint in mints {
            if let Some(pool) = self.venue.derive_pool(mint) {
                if self.pool_index.insert(pool, mint.clone()).is_none() {
                    added = true;
                }
            }
        }
        if added {
            self.pools_changed.notify_one();
        }
    }

    /// Remove the given mint pool PDAs from the subscription set.
    pub fn untrack_pools(&self, mints: &[String]) {
        let mut removed = false;
        for mint in mints {
            if let Some(pool) = self.venue.derive_pool(mint) {
                if self.pool_index.remove(&pool).is_some() {
                    removed = true;
                }
            }
        }
        if removed {
            self.pools_changed.notify_one();
        }
    }

    /// Direct access to the shared pool→mint index (for host backfill paths
    /// that need to query the live tracked set without going through the handle).
    pub fn pool_index(&self) -> PoolIndex {
        self.pool_index.clone()
    }

    /// Notify handle — fires when a pool is added/removed (auto or manual).
    pub fn pools_changed(&self) -> Arc<Notify> {
        self.pools_changed.clone()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn commitment_level(c: Commitment) -> CommitmentLevel {
    match c {
        Commitment::Processed => CommitmentLevel::Processed,
        Commitment::Confirmed => CommitmentLevel::Confirmed,
        Commitment::Finalized => CommitmentLevel::Finalized,
    }
}
