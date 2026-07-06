//! LaserStream (Yellowstone gRPC) ingest crate — standalone drop-in.
//!
//! ```text
//! Ingest::builder()
//!     .endpoint(url)
//!     .api_key(key)
//!     .protocol(Protocol::pump_fun())
//!     .config(IngestConfig::default())
//!     .build()?
//!     .start()
//! -> (Receiver<IngestEvent>, IngestHandle)
//! ```
//!
//! The host owns all sinks (DB, cache, SSE, strategy, watchdog). The crate emits
//! decoded [`event::IngestEvent`]s out a bounded mpsc channel and never reads env.
#[allow(clippy::all, dead_code)]
#[rustfmt::skip]
pub mod proto {
    include!("generated/mod.rs");
}

#[cfg(feature = "rpc-backfill")]
pub mod backfill;
pub mod config;
pub mod decode;
pub mod error;
pub mod event;
pub mod pool;
pub mod protocol;

pub mod slot_anchor;
pub mod transport;

#[cfg(feature = "raw-tx")]
pub mod raw_tx;

pub use config::{Commitment, IngestConfig};
pub use error::{IngestError, Result};
pub use event::IngestEvent;
pub use pool::PoolIndex;
pub use protocol::Protocol;

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{mpsc, watch, Notify};
use tracing::warn;

use decode::{DecodeOutput, Decoder, TxRelevance};
use proto::geyser::{CommitmentLevel, SubscribeUpdateTransaction};
use transport::TransportConfig;

// ── Builder ───────────────────────────────────────────────────────────────────

/// Validated, ready-to-start ingest session.
pub struct Ingest {
    endpoint: String,
    api_key: String,
    protocol: Arc<Protocol>,
    config: IngestConfig,
}

/// Fluent builder for [`Ingest`].
#[derive(Default)]
pub struct IngestBuilder {
    endpoint: Option<String>,
    api_key: Option<String>,
    protocol: Option<Protocol>,
    config: Option<IngestConfig>,
}

impl IngestBuilder {
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn protocol(mut self, p: Protocol) -> Self {
        self.protocol = Some(p);
        self
    }

    pub fn config(mut self, c: IngestConfig) -> Self {
        self.config = Some(c);
        self
    }

    pub fn build(self) -> Result<Ingest> {
        let endpoint = self.endpoint.ok_or_else(|| {
            IngestError::InvalidEndpoint("endpoint() not set".into())
        })?;
        let api_key = self.api_key.ok_or_else(|| {
            IngestError::InvalidEndpoint("api_key() not set".into())
        })?;
        Ok(Ingest {
            endpoint,
            api_key,
            protocol: Arc::new(self.protocol.unwrap_or_else(Protocol::pump_fun)),
            config: self.config.unwrap_or_default(),
        })
    }
}

impl Ingest {
    pub fn builder() -> IngestBuilder {
        IngestBuilder::default()
    }

    /// Spawn the transport + decode tasks and return the event receiver + handle.
    ///
    /// `live` — initial live-mode state (pass `true` to start streaming
    ///   immediately; `false` pauses until `handle.set_live(true)` is called).
    pub fn start(self, live: bool) -> (mpsc::Receiver<IngestEvent>, IngestHandle) {
        let cfg = &self.config;
        let protocol = self.protocol.clone();

        // Internal transport→decode channel.
        let (update_tx, mut update_rx) = mpsc::channel::<(
            Arc<SubscribeUpdateTransaction>,
            TxRelevance,
            chrono::DateTime<chrono::Utc>,
        )>(cfg.update_channel_cap);

        // Host-facing event channel.
        let (event_tx, event_rx) = mpsc::channel::<IngestEvent>(cfg.event_channel_cap);

        // Shared pool→mint index (transport resubscribes when it changes).
        let pool_index: PoolIndex = Arc::new(DashMap::new());
        let pools_changed = Arc::new(Notify::new());

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

        // Decoder: gets the pool index + pools_changed Notify.
        let decoder = {
            let mut d = Decoder::new(protocol.clone()).with_pools_changed(pools_changed.clone());
            if cfg.track_amm {
                d = d.with_pool_index(pool_index.clone());
            }
            Arc::new(d)
        };

        // Gap-replay config channel: host sends (gap_replay_on_reconnect,
        // gap_replay_max_window_secs) whenever the operator changes the settings.
        // Default: off (false, 300 s). The sender is exposed via IngestHandle so
        // the host can push updates without restarting ingest.
        let (gap_replay_tx, gap_replay_rx) = watch::channel::<(bool, u64)>((false, 300));

        // Spawn transport task.
        tokio::spawn(transport::run(
            self.endpoint.clone(),
            self.api_key.clone(),
            protocol,
            update_tx,
            live_rx,
            pool_index.clone(),
            pools_changed.clone(),
            transport_cfg,
            gap_replay_rx,
        ));

        // Spawn decode task.
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            while let Some((update, relevance, received_at)) = update_rx.recv().await {
                let output = decoder.decode_relevant_pb(&update, relevance, received_at);

                let events = match output {
                    DecodeOutput::Events(v) => v,
                    DecodeOutput::Ignored => continue,
                };

                // Optionally append the raw-tx event (under feature gate).
                #[cfg(feature = "raw-tx")]
                let events = {
                    let mut v = events;
                    if let Some(raw_ev) = raw_tx::build_raw_tx_event(&update, received_at) {
                        v.push(raw_ev);
                    }
                    v
                };

                for ev in events {
                    match event_tx_clone.try_send(ev) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(ev)) => {
                            warn!("ingest: event channel full — dropping event");
                            // Retry with timeout to avoid silent loss on brief back-pressure.
                            let _ = event_tx_clone.send_timeout(ev, std::time::Duration::from_millis(100)).await;
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
            pool_index,
            pools_changed,
            protocol: self.protocol,
        };

        (event_rx, handle)
    }
}

// ── IngestHandle ──────────────────────────────────────────────────────────────

/// Control handle returned by [`Ingest::start`]. Clone-safe; all methods are
/// lock-free (atomic writes or channel sends).
pub struct IngestHandle {
    live_tx: watch::Sender<bool>,
    /// Gap-replay settings: (gap_replay_on_reconnect, gap_replay_max_window_secs).
    /// Push a new value whenever the operator changes the settings; the transport
    /// picks it up on the next reconnect.
    gap_replay_tx: watch::Sender<(bool, u64)>,
    pool_index: PoolIndex,
    pools_changed: Arc<Notify>,
    protocol: Arc<Protocol>,
}

impl IngestHandle {
    /// Pause (`false`) or resume (`true`) the transport stream.
    pub fn set_live(&self, live: bool) {
        let _ = self.live_tx.send(live);
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
            if pool::register_pool(&self.pool_index, mint, &self.protocol) {
                added = true;
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
            if let Some(pool) = pool::pool_for_mint(mint, &self.protocol) {
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
