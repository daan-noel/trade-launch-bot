//! The venue-generic ingest session: [`Ingest<V>`] + [`IngestHandle<V>`].
//!
//! `Ingest<V>` owns everything below the wire — the two decode lanes, the host
//! event channel, the live gate, the gap-replay setting, and the cross-feed
//! dedupe ring. It spawns **no** transport: [`Ingest::start`] hands back a
//! [`FeedLanes`] carrying exactly what a feed supervisor needs, and the venue's
//! assembly root spawns one [`crate::supervisor::run`] per configured feed.
//!
//! That is the seam that makes feeds peers. Every feed writes to the same lanes,
//! so nothing from the decoders down can tell which wire delivered a transaction
//! — or notice when the assembly root moves traffic between them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Notify};
use tracing::warn;

/// Count of events dropped after the back-pressure retry ALSO failed — the only
/// genuinely-lost events (a `Full` that the 100ms retry couldn't drain, or a
/// closed channel). Process-wide (ingest is a single session); surfaced on every
/// real drop so sustained loss is visible instead of silent.
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

use crate::config::IngestConfig;
use crate::dedupe::SignatureDedupe;
use crate::event::IngestEvent;
use crate::proto::geyser::SubscribeUpdateTransaction;
use crate::push::PushHooks;
use crate::venue::{DecodeOutput, IngestVenue, PoolIndex};

/// One message on a feed→decode lane.
pub type UpdateMsg<V> = (
    Arc<SubscribeUpdateTransaction>,
    <V as IngestVenue>::Relevance,
    chrono::DateTime<chrono::Utc>,
);

/// Everything a feed supervisor needs from the session, and nothing about a wire.
///
/// Cloned once per feed. The lane senders, the live gate, the gap-replay setting
/// and the dedupe ring are shared; that sharing is what makes two feeds
/// interchangeable upstream of one pipeline.
pub struct FeedLanes<V: IngestVenue> {
    /// Create fast lane — `V::is_create_lane` traffic only.
    pub create_tx: mpsc::Sender<UpdateMsg<V>>,
    /// Everything else.
    pub normal_tx: mpsc::Sender<UpdateMsg<V>>,
    /// Live gate: `false` pauses every feed.
    pub live_rx: watch::Receiver<bool>,
    /// `(gap_replay_on_reconnect, gap_replay_max_window_secs)`. A feed that
    /// cannot replay warns once and ignores it.
    pub gap_replay_rx: watch::Receiver<(bool, u64)>,
    /// Cross-feed signature window. `None` when a single feed is running — one
    /// wire cannot deliver the same signature twice, so it skips the check.
    pub dedupe: Option<Arc<SignatureDedupe>>,
    /// Optional push feeds carried on the same subscription.
    pub push: Arc<PushHooks>,
}

impl<V: IngestVenue> Clone for FeedLanes<V> {
    fn clone(&self) -> Self {
        Self {
            create_tx: self.create_tx.clone(),
            normal_tx: self.normal_tx.clone(),
            live_rx: self.live_rx.clone(),
            gap_replay_rx: self.gap_replay_rx.clone(),
            dedupe: self.dedupe.clone(),
            push: self.push.clone(),
        }
    }
}

/// The pipeline below the wire, ready to start.
pub struct Ingest<V: IngestVenue> {
    venue: Arc<V>,
    config: IngestConfig,
    push: Arc<PushHooks>,
}

impl<V: IngestVenue> Ingest<V> {
    pub fn new(venue: Arc<V>, config: IngestConfig) -> Self {
        Self {
            venue,
            config,
            push: Arc::new(PushHooks::default()),
        }
    }

    /// Attach optional push-feed hooks. See [`PushHooks`]. Default: none — a
    /// host that doesn't opt in gets a subscription with no extra filters.
    pub fn with_push_hooks(mut self, push: PushHooks) -> Self {
        self.push = Arc::new(push);
        self
    }

    /// Spawn the decode lanes and return the host event receiver, the control
    /// handle, and the wiring for `feeds` feed supervisors.
    ///
    /// `live` — initial live-mode state (`true` streams immediately; `false`
    ///   pauses every feed until `handle.set_live(true)`).
    /// `feeds` — how many feeds the caller is about to spawn. More than one arms
    ///   the cross-feed dedupe ring; exactly one skips it entirely, because a
    ///   single wire cannot deliver the same signature twice.
    ///
    /// Two lanes share one host `event_tx`: creates (`V::is_create_lane`) never
    /// queue behind AMM/curve swap decode work.
    pub fn start(
        self,
        live: bool,
        feeds: usize,
    ) -> (
        mpsc::Receiver<IngestEvent>,
        IngestHandle<V>,
        FeedLanes<V>,
    ) {
        let cfg = &self.config;
        let venue = self.venue.clone();

        // Create lane stays shallow: creates are rare vs swaps, and a deep
        // create backlog would only mean the host is already wedged.
        let create_cap = (cfg.update_channel_cap / 8).max(64);
        let (create_tx, create_rx) = mpsc::channel::<UpdateMsg<V>>(create_cap);
        let (normal_tx, normal_rx) = mpsc::channel::<UpdateMsg<V>>(cfg.update_channel_cap);

        // Host-facing event channel (both decode lanes merge here).
        let (event_tx, event_rx) = mpsc::channel::<IngestEvent>(cfg.event_channel_cap);

        // Live-mode gate.
        let (live_tx, live_rx) = watch::channel(live);

        // Gap-replay config channel: host sends (gap_replay_on_reconnect,
        // gap_replay_max_window_secs) whenever the operator changes the settings.
        // Default: off (false, 300 s).
        let (gap_replay_tx, gap_replay_rx) = watch::channel::<(bool, u64)>((false, 300));

        let dedupe =
            (feeds > 1).then(|| Arc::new(SignatureDedupe::for_window(cfg.dedupe_window)));

        let lanes = FeedLanes {
            create_tx,
            normal_tx,
            live_rx,
            gap_replay_rx,
            dedupe,
            push: self.push.clone(),
        };

        spawn_decode_lane(venue.clone(), create_rx, event_tx.clone());
        spawn_decode_lane(venue.clone(), normal_rx, event_tx);

        let handle = IngestHandle {
            live_tx,
            gap_replay_tx,
            pool_index: venue.pool_index(),
            pools_changed: venue.pools_changed(),
            venue,
        };

        (event_rx, handle, lanes)
    }
}

/// One decode task for one feed→decode lane. Both lanes merge onto the same host
/// `event_tx` — the split only isolates decode/queue latency.
fn spawn_decode_lane<V: IngestVenue>(
    venue: Arc<V>,
    mut update_rx: mpsc::Receiver<UpdateMsg<V>>,
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

/// Control handle for everything below the wire. All methods are lock-free
/// (atomic writes or channel sends).
///
/// Which feed carries what is **not** here: that is the assembly root's, because
/// naming a wire is the one thing this crate refuses to do. A venue façade wraps
/// this handle and adds its own feed-selection method.
pub struct IngestHandle<V: IngestVenue> {
    live_tx: watch::Sender<bool>,
    gap_replay_tx: watch::Sender<(bool, u64)>,
    pool_index: PoolIndex,
    pools_changed: Arc<Notify>,
    venue: Arc<V>,
}

impl<V: IngestVenue> IngestHandle<V> {
    /// Pause (`false`) or resume (`true`) every feed.
    pub fn set_live(&self, live: bool) {
        let _ = self.live_tx.send(live);
    }

    /// Current live-mode state (`true` = streaming, `false` = paused).
    pub fn is_live(&self) -> bool {
        *self.live_tx.borrow()
    }

    /// Push updated gap-replay settings. Takes effect on the next reconnect of
    /// any feed that can replay; a feed that cannot warns once and ignores it.
    pub fn set_gap_replay(&self, on: bool, max_window_secs: u64) {
        let _ = self.gap_replay_tx.send((on, max_window_secs));
    }

    /// Register pool PDAs for the given mints and signal the feeds to
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
