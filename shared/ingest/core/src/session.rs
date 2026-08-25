//! The venue-generic ingest session: [`Ingest<V>`] + [`IngestHandle<V>`].
//!
//! `Ingest<V>` owns the transport + decode task wiring; a venue-specific builder
//! (e.g. the pump.fun façade's `IngestBuilder`) assembles the `V` and constructs
//! the session via [`Ingest::new`]. `start()` spawns the transport task
//! (`transport::run<V>`) and **two** decode tasks (create lane + normal lane),
//! returning the host event receiver + a control handle.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Notify};
use tracing::warn;

/// Count of events dropped after the back-pressure retry ALSO failed — the only
/// genuinely-lost events (a `Full` that the 100ms retry couldn't drain, or a
/// closed channel). Process-wide (ingest is a single session); surfaced on every
/// real drop so sustained loss is visible instead of silent.
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

use crate::config::{Auth, Commitment, CurveSource, IngestConfig};
use crate::dedupe::SignatureDedupe;
use crate::event::IngestEvent;
use crate::proto::geyser::{CommitmentLevel, SubscribeUpdateTransaction};
use crate::transport::{self, PushHooks, TransportConfig};
use crate::venue::{DecodeOutput, IngestVenue, PoolIndex};

/// Validated, ready-to-start ingest session, generic over the venue.
pub struct Ingest<V: IngestVenue> {
    endpoint: String,
    auth: Auth,
    venue: Arc<V>,
    config: IngestConfig,
    push: Arc<PushHooks>,
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
            push: Arc::new(PushHooks::default()),
        }
    }

    /// Attach optional push-feed hooks (`blocks_meta` / watched-account updates
    /// on the same subscription). See [`PushHooks`]. Default: none — the
    /// subscription is unchanged for hosts that don't opt in.
    pub fn with_push_hooks(mut self, push: PushHooks) -> Self {
        self.push = Arc::new(push);
        self
    }

    /// Spawn the transport + decode tasks and return the event receiver + handle.
    ///
    /// `live` — initial live-mode state (pass `true` to start streaming
    ///   immediately; `false` pauses until `handle.set_live(true)` is called).
    ///
    /// Two transport→decode lanes share one host `event_tx`: creates
    /// (`V::is_create_lane`) never queue behind AMM/curve swap decode work.
    pub fn start(self, live: bool) -> (mpsc::Receiver<IngestEvent>, IngestHandle<V>) {
        let cfg = &self.config;
        let venue = self.venue.clone();

        type UpdateMsg<V> = (
            Arc<SubscribeUpdateTransaction>,
            <V as IngestVenue>::Relevance,
            chrono::DateTime<chrono::Utc>,
        );

        // Create lane stays shallow: creates are rare vs swaps, and a deep
        // create backlog would only mean the host is already wedged.
        let create_cap = (cfg.update_channel_cap / 8).max(64);
        let (create_tx, create_rx) = mpsc::channel::<UpdateMsg<V>>(create_cap);
        let (normal_tx, normal_rx) = mpsc::channel::<UpdateMsg<V>>(cfg.update_channel_cap);

        // Host-facing event channel (both decode lanes merge here).
        let (event_tx, event_rx) = mpsc::channel::<IngestEvent>(cfg.event_channel_cap);

        // Live-mode gate.
        let (live_tx, live_rx) = watch::channel(live);

        // Which transport carries bonding-curve traffic. Switchable at runtime via
        // `IngestHandle::set_curve_source`; the gRPC transport re-scopes its
        // subscription and the NATS task connects/disconnects off this one signal.
        let curve_source = resolve_curve_source(cfg);
        let (source_tx, source_rx) = watch::channel(curve_source);

        // Only meaningful when two transports run at once — a single transport
        // cannot deliver the same signature twice, so it skips the check entirely.
        let dedupe = cfg
            .nats
            .as_ref()
            .map(|_| Arc::new(SignatureDedupe::for_window(cfg.dedupe_window)));

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

        // The NATS curve feed. Spawned whenever it is *configured*, not only when
        // it is selected: it idles (fully disconnected) until `source_rx` says
        // Nats, which is what makes the switch instant and restart-free.
        #[cfg(feature = "nats")]
        if let Some(nats_cfg) = cfg.nats.clone() {
            tokio::spawn(crate::nats::run(
                nats_cfg,
                venue.clone(),
                create_tx.clone(),
                normal_tx.clone(),
                live_tx.subscribe(),
                source_rx.clone(),
                transport_cfg.clone(),
                dedupe
                    .clone()
                    .expect("dedupe is built whenever nats is configured"),
            ));
        }

        tokio::spawn(transport::run(
            self.endpoint.clone(),
            self.auth.clone(),
            venue.clone(),
            create_tx,
            normal_tx,
            live_rx,
            transport_cfg,
            gap_replay_rx,
            self.push.clone(),
            source_rx,
            dedupe,
        ));

        spawn_decode_lane(venue.clone(), create_rx, event_tx.clone());
        spawn_decode_lane(venue.clone(), normal_rx, event_tx);

        let handle = IngestHandle {
            live_tx,
            gap_replay_tx,
            source_tx,
            pool_index: venue.pool_index(),
            pools_changed: venue.pools_changed(),
            venue,
        };

        (event_rx, handle)
    }
}

/// One decode task for one transport→decode lane. Both lanes merge onto the
/// same host `event_tx` — the split only isolates decode/queue latency.
fn spawn_decode_lane<V: IngestVenue>(
    venue: Arc<V>,
    mut update_rx: mpsc::Receiver<(
        Arc<SubscribeUpdateTransaction>,
        V::Relevance,
        chrono::DateTime<chrono::Utc>,
    )>,
    event_tx: mpsc::Sender<IngestEvent>,
) {
    tokio::spawn(async move {
        while let Some((update, relevance, received_at)) = update_rx.recv().await {
            let output = venue.decode(&update, relevance, received_at);

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
                match event_tx.try_send(ev) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(ev)) => {
                        // Channel briefly full — retry with a short timeout
                        // rather than logging a drop prematurely (the retry
                        // usually succeeds under transient back-pressure).
                        if event_tx
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
    /// Which transport carries bonding-curve traffic. Both transport tasks watch
    /// this, so one send re-points the feed with no restart.
    source_tx: watch::Sender<CurveSource>,
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

    /// Switch which transport carries bonding-curve traffic.
    ///
    /// Takes effect immediately and without a restart: the newly-selected feed
    /// connects while the old one is still draining, the dedupe ring absorbs the
    /// overlap, and the gRPC subscription re-scopes in place (keeping AMM pools
    /// subscribed throughout, so open positions never lose their feed).
    ///
    /// Switching to [`CurveSource::Nats`] is a no-op if no `NatsConfig` was
    /// supplied at build time — there would be nothing to connect to.
    pub fn set_curve_source(&self, source: CurveSource) {
        let _ = self.source_tx.send(source);
    }

    /// Which transport is currently carrying bonding-curve traffic.
    pub fn curve_source(&self) -> CurveSource {
        *self.source_tx.borrow()
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

/// Validate the requested curve source against what was actually built.
///
/// Selecting NATS without a relay to talk to would leave the curve feed silently
/// dead — the worst possible failure for an ingest path — so fall back to gRPC
/// loudly instead.
fn resolve_curve_source(cfg: &IngestConfig) -> CurveSource {
    if cfg.curve_source != CurveSource::Nats {
        return cfg.curve_source;
    }
    if cfg.nats.is_none() {
        warn!("ingest: curve_source=Nats but no NatsConfig was supplied — using gRPC");
        return CurveSource::Grpc;
    }
    #[cfg(not(feature = "nats"))]
    {
        warn!("ingest: curve_source=Nats but the `nats` feature is not compiled in — using gRPC");
        CurveSource::Grpc
    }
    #[cfg(feature = "nats")]
    {
        tracing::info!("ingest: bonding curve on the NATS relay; AMM pools stay on gRPC");
        CurveSource::Nats
    }
}

fn commitment_level(c: Commitment) -> CommitmentLevel {
    match c {
        Commitment::Processed => CommitmentLevel::Processed,
        Commitment::Confirmed => CommitmentLevel::Confirmed,
        Commitment::Finalized => CommitmentLevel::Finalized,
    }
}
