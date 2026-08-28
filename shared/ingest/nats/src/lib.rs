//! The NATS relay ingest feed — a bonding-curve wire that costs no provider
//! credits.
//!
//! A third party subscribes to Helius `transactionSubscribe` once and
//! rebroadcasts the raw `transactionNotification` frames on a NATS subject. This
//! crate consumes that subject and produces exactly what the gRPC feed produces:
//! neutral [`FeedUpdate`]s on `ingest-core`'s one supervisor. Everything
//! downstream — dedupe, classify, lanes, decode, events, host sinks — is
//! identical and cannot tell the two apart.
//!
//! # What this wire cannot do, and says so through [`CAPS`]
//!
//! - **No filter control.** The publisher chooses what it broadcasts; we take
//!   the whole subject and classify locally. AMM pool traffic therefore stays on
//!   the gRPC feed, whose filter is keyed on the pool PDAs this bot tracks.
//! - **No replay.** Core NATS is at-most-once with no history, so there is no
//!   `from_slot` equivalent. A reconnect resumes live and the gap is lost — which
//!   is also why a stalled decode pipeline sheds here instead of reconnecting.
//! - **No server-side failure filter.** gRPC screens failed transactions
//!   (`failed: Some(false)`); [`frame::parse`] applies the same screen locally so
//!   both sources deliver the same corpus.
//!
//! # Slow-consumer defence
//!
//! Core NATS *disconnects* a consumer that falls behind rather than buffering
//! it. So the socket reader does nothing but move frames into a bounded queue,
//! and sheds when that queue is full; JSON parsing happens on the consuming side.
//! Shedding a frame keeps the read loop fast, which is what keeps the connection
//! alive.

pub mod client;
pub mod frame;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ingest_core::feed::{Feed, FeedCaps, FeedConn, FeedError, FeedUpdate, Subscription};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};
use tracing::{info, warn};

pub use client::{NatsConn, NatsError, ServerInfo};
use frame::Reject;

/// Frames dropped because the consumer could not keep up. Process-wide; a
/// nonzero value means the decode pipeline — not the network — is the bottleneck.
static SHED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// Total frames shed since process start (see [`SHED_FRAMES`]).
pub fn shed_frames() -> u64 {
    SHED_FRAMES.load(Ordering::Relaxed)
}

/// How often the wire-level counters are logged.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// Subscription id. Only one subscription per connection here.
const SID: u64 = 1;

/// Client name announced in the NATS `CONNECT` line.
const CLIENT_NAME: &str = "hunter-ingest";

/// What this wire can do. Read by the supervisor instead of naming the transport.
pub const CAPS: FeedCaps = FeedCaps {
    replay: false,
    server_filter: false,
    in_place_resubscribe: false,
};

// ── Config ────────────────────────────────────────────────────────────────────

/// Everything this wire needs and no other wire has.
#[derive(Debug, Clone)]
pub struct NatsConfig {
    /// `nats://host:port`. Comma-separated entries seed a cluster.
    pub url: String,
    /// Subject to subscribe to, e.g. `helius.raw.bondingcurve`.
    ///
    /// Prefer an exact subject over a wildcard: a relay that also publishes a
    /// mirror subject (`helius.raw.all`) would otherwise deliver every frame
    /// twice, doubling bandwidth for nothing.
    pub subject: String,
    /// Optional queue group — only for running several consumers that should
    /// *share* the stream. A single bot leaves this `None`.
    pub queue_group: Option<String>,
    /// Depth of the reader → parser hand-off. Frames beyond this are shed (and
    /// counted) rather than blocking the socket read.
    pub frame_channel_cap: usize,
    /// TCP connect timeout.
    pub connect_timeout: Duration,
    /// How long the subject may go silent before the supervisor forces a
    /// reconnect. Longer than the gRPC allowance: a relay's publish cadence is
    /// not ours to control.
    pub idle_reconnect_timeout: Duration,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            subject: "helius.raw.bondingcurve".to_string(),
            queue_group: None,
            frame_channel_cap: 8192,
            connect_timeout: Duration::from_secs(10),
            idle_reconnect_timeout: Duration::from_secs(30),
        }
    }
}

// ── The feed ──────────────────────────────────────────────────────────────────

/// The NATS relay curve feed.
pub struct NatsFeed {
    cfg: NatsConfig,
}

impl NatsFeed {
    pub fn new(cfg: NatsConfig) -> Self {
        if cfg.subject.contains('*') || cfg.subject.contains('>') {
            // Relays commonly publish the same frames on a specific subject AND
            // a catch-all mirror; a wildcard then delivers every message twice.
            warn!(
                subject = %cfg.subject,
                "NATS: subject is a wildcard - if the relay mirrors subjects this doubles \
                 bandwidth for no extra data (the cross-feed dedupe drops the copies)"
            );
        }
        Self { cfg }
    }

    /// This feed's idle allowance, for the supervisor's `FeedPolicy`.
    pub fn idle_reconnect_timeout(&self) -> Duration {
        self.cfg.idle_reconnect_timeout
    }
}

impl Feed for NatsFeed {
    type Conn = RelayConn;

    fn name(&self) -> &'static str {
        "nats"
    }

    fn caps(&self) -> FeedCaps {
        CAPS
    }

    /// Connect and subscribe. The neutral [`Subscription`]'s account filter and
    /// `from_slot` are absent by construction — the supervisor never builds them
    /// for a feed whose [`CAPS`] deny them.
    async fn connect(&self, sub: Subscription) -> Result<Self::Conn, FeedError> {
        if self.cfg.url.is_empty() {
            return Err(FeedError::Connect("no url configured".into()));
        }
        let _ = sub;

        let mut conn = NatsConn::connect(&self.cfg.url, CLIENT_NAME, self.cfg.connect_timeout)
            .await
            .map_err(|e| FeedError::Connect(format!("{} — {e}", self.cfg.url)))?;

        conn.subscribe(&self.cfg.subject, self.cfg.queue_group.as_deref(), SID)
            .await
            .map_err(|e| FeedError::Subscribe(format!("{} — {e}", self.cfg.subject)))?;

        info!(
            url = %self.cfg.url,
            subject = %self.cfg.subject,
            "NATS: connected - curve feed live"
        );

        // Reader → parser hand-off. Bounded and shed-on-full: see the crate docs.
        let (frame_tx, frame_rx) = mpsc::channel::<Vec<u8>>(self.cfg.frame_channel_cap);
        let idle = self.cfg.idle_reconnect_timeout;
        let subject = self.cfg.subject.clone();

        // Reader task: drain the socket as fast as the OS delivers, nothing else.
        let reader = tokio::spawn(async move {
            loop {
                let payload = match timeout(idle, conn.next_message()).await {
                    Ok(Ok(p)) => p,
                    Ok(Err(e)) => return format!("read failed: {e}"),
                    Err(_) => return format!("no frame on {subject} for {idle:?}"),
                };

                if frame_tx.try_send(payload).is_err() {
                    if frame_tx.is_closed() {
                        return "consumer dropped the frame queue".to_string();
                    }
                    // Full: the consumer is behind. Shedding is what keeps this
                    // loop fast enough to avoid a server-side slow-consumer
                    // disconnect.
                    let n = SHED_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 1_000 == 1 {
                        warn!(
                            shed_total = n,
                            "NATS: consumer behind - shedding frames (the decode pipeline is \
                             the bottleneck)"
                        );
                    }
                }
            }
        });

        Ok(RelayConn {
            frame_rx,
            reader: Some(reader),
            subject: self.cfg.subject.clone(),
            stats: WireStats::default(),
            next_stats: Instant::now() + STATS_INTERVAL,
        })
    }
}

/// Wire-level counters — what this transport alone can see. The relevance and
/// duplicate counts belong to the supervisor, which measures them for every feed.
#[derive(Default)]
struct WireStats {
    frames: u64,
    transactions: u64,
    failed_tx: u64,
    unparseable: u64,
}

impl WireStats {
    fn log(&self, subject: &str) {
        info!(
            subject,
            frames = self.frames,
            transactions = self.transactions,
            failed_tx = self.failed_tx,
            unparseable = self.unparseable,
            shed = shed_frames(),
            "NATS: wire stats"
        );
    }
}

/// One live relay subscription: the bounded frame queue plus the reader task
/// draining the socket into it.
pub struct RelayConn {
    frame_rx: mpsc::Receiver<Vec<u8>>,
    /// Taken when the queue closes, to recover why the reader stopped.
    reader: Option<JoinHandle<String>>,
    subject: String,
    stats: WireStats,
    next_stats: Instant,
}

impl RelayConn {
    /// The reader stopped, so ask it why: the supervisor's backoff should be
    /// driven by the real cause rather than an immediate blind retry.
    async fn reader_stop(&mut self) -> FeedError {
        match self.reader.take() {
            Some(h) => match h.await {
                Ok(why) => FeedError::Stream(why),
                Err(_) => FeedError::Stream("reader task ended unexpectedly".into()),
            },
            None => FeedError::Closed,
        }
    }
}

impl Drop for RelayConn {
    fn drop(&mut self) {
        self.stats.log(&self.subject);
        // Aborting drops the socket, which closes the subscription.
        if let Some(h) = &self.reader {
            h.abort();
        }
    }
}

impl FeedConn for RelayConn {
    /// Cancel-safe: the only `await` is the queue receive, and everything after
    /// it runs to a `return` without yielding, so a frame that has been taken off
    /// the queue is always accounted for.
    async fn next(&mut self) -> Result<FeedUpdate, FeedError> {
        let Some(payload) = self.frame_rx.recv().await else {
            return Err(self.reader_stop().await);
        };
        self.stats.frames += 1;

        let update = match frame::parse(&payload) {
            Ok(u) => {
                self.stats.transactions += 1;
                u
            }
            // A frame that carries nothing is still liveness evidence — this
            // subject's frame rate is the only thing whose absence means "dead".
            Err(Reject::Failed) => {
                self.stats.failed_tx += 1;
                FeedUpdate::Tick
            }
            Err(Reject::Unparseable) => {
                self.stats.unparseable += 1;
                FeedUpdate::Tick
            }
        };

        if Instant::now() >= self.next_stats {
            self.stats.log(&self.subject);
            self.next_stats = Instant::now() + STATS_INTERVAL;
        }
        Ok(update)
    }

    /// A broadcast subject has no filter to update. The supervisor never calls
    /// this — [`CAPS`] says so — and would reconnect instead.
    async fn resubscribe(&mut self, _sub: Subscription) -> Result<(), FeedError> {
        Err(FeedError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capability set is this crate's contract with the supervisor: no
    /// replay is what makes a stalled pipeline shed instead of reconnect, and no
    /// server filter is what keeps AMM pools on the gRPC feed.
    #[test]
    fn this_wire_neither_replays_nor_filters() {
        assert!(!CAPS.replay);
        assert!(!CAPS.server_filter);
        assert!(!CAPS.in_place_resubscribe);
        assert!(
            !CAPS.reconnect_on_backpressure(),
            "a reconnect here loses the same frames a shed would AND costs a resubscribe"
        );
    }

    /// The defaults are the proven relay settings; the idle allowance is
    /// deliberately longer than gRPC's because a relay's publish cadence is not
    /// ours to control.
    #[test]
    fn defaults_are_the_proven_relay_settings() {
        let c = NatsConfig::default();
        assert_eq!(c.subject, "helius.raw.bondingcurve");
        assert_eq!(c.frame_channel_cap, 8192);
        assert_eq!(c.idle_reconnect_timeout, Duration::from_secs(30));
        assert!(c.queue_group.is_none());
    }
}
