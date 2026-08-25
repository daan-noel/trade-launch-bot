//! NATS relay transport — a bonding-curve feed that costs no provider credits.
//!
//! A third party subscribes to Helius `transactionSubscribe` once and rebroadcasts
//! the raw `transactionNotification` frames on a NATS subject. This task consumes
//! that subject and produces exactly what the gRPC transport produces: a
//! classified [`SubscribeUpdateTransaction`] on the create or normal decode lane.
//! Everything downstream — dedupe, decode, events, host sinks — is unchanged.
//!
//! # What this transport cannot do
//!
//! - **No filter control.** The publisher chooses what it broadcasts; we take the
//!   whole subject and classify locally. AMM pool traffic therefore stays on the
//!   gRPC transport, whose filter is keyed on the pool PDAs this bot tracks.
//! - **No replay.** Core NATS is at-most-once with no history, so there is no
//!   `from_slot` equivalent. A reconnect resumes live and the gap is lost.
//! - **No failure filter.** gRPC screens failed transactions server-side
//!   (`failed: Some(false)`); this task applies the same screen locally via
//!   [`crate::convert::json_tx_failed`] so both sources behave identically.
//!
//! # Slow-consumer defence
//!
//! Core NATS *disconnects* a consumer that falls behind rather than buffering it.
//! So the socket reader does nothing but move frames into a bounded queue, and
//! sheds when that queue is full; JSON parsing and decode dispatch happen on a
//! separate task. Shedding a frame keeps the read loop fast, which is what keeps
//! the connection alive — the opposite of the gRPC transport, which deliberately
//! drops its connection under backpressure so the provider can replay the gap.
//! Here a reconnect would lose the same frames and cost a resubscribe, so a
//! stalled pipeline sheds rather than reconnects.

mod client;

pub use client::{NatsConn, NatsError, ServerInfo};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::{mpsc, watch};
use tokio::time::{timeout, Duration, Instant};
use tracing::{debug, info, warn};

use crate::config::{CurveSource, NatsConfig};
use crate::convert;
use crate::dedupe::SignatureDedupe;
use crate::proto::geyser::SubscribeUpdateTransaction;
use crate::transport::TransportConfig;
use crate::venue::IngestVenue;

/// Frames dropped because the parser could not keep up. Process-wide; a nonzero
/// value means the decode pipeline — not the network — is the bottleneck.
static SHED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// How often the running counters are logged.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// A connection that survives at least this long is treated as healthy, so the
/// reconnect ramp restarts from the base delay instead of climbing forever.
const HEALTHY_UPTIME: Duration = Duration::from_secs(30);

/// Subscription id. Only one subscription per connection here.
const SID: u64 = 1;

/// Total frames shed since process start (see [`SHED_FRAMES`]).
pub fn shed_frames() -> u64 {
    SHED_FRAMES.load(Ordering::Relaxed)
}

/// Snapshot of everything worth reporting about one connection.
#[derive(Default)]
struct Stats {
    frames: u64,
    decoded: u64,
    duplicates: u64,
    failed_tx: u64,
    unparseable: u64,
    irrelevant: u64,
}

impl Stats {
    fn log(&self, subject: &str) {
        info!(
            subject,
            frames = self.frames,
            decoded = self.decoded,
            duplicates = self.duplicates,
            failed_tx = self.failed_tx,
            unparseable = self.unparseable,
            irrelevant = self.irrelevant,
            shed = SHED_FRAMES.load(Ordering::Relaxed),
            "NATS: feed stats"
        );
    }
}

/// Run the NATS curve feed until the host drops the decode lanes.
///
/// Idles (fully disconnected, so the subject costs no bandwidth) whenever live
/// mode is off or `source_rx` is not [`CurveSource::Nats`], and reconnects when
/// it is switched back on.
#[allow(clippy::too_many_arguments)]
pub async fn run<V: IngestVenue>(
    nats_cfg: NatsConfig,
    venue: Arc<V>,
    create_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    normal_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    mut live_rx: watch::Receiver<bool>,
    mut source_rx: watch::Receiver<CurveSource>,
    cfg: Arc<TransportConfig>,
    dedupe: Arc<SignatureDedupe>,
) {
    if nats_cfg.url.is_empty() {
        warn!("NATS: no url configured - curve feed cannot start");
        return;
    }
    if nats_cfg.subject.contains('*') || nats_cfg.subject.contains('>') {
        // Relays commonly publish the same frames on a specific subject AND a
        // catch-all mirror; a wildcard then delivers every message twice.
        warn!(
            subject = %nats_cfg.subject,
            "NATS: subject is a wildcard - if the relay mirrors subjects this \
             doubles bandwidth for no extra data (dedupe drops the copies)"
        );
    }

    let mut backoff = cfg.reconnect_base;

    loop {
        // Idle until this transport is the selected curve source.
        while !(*live_rx.borrow() && *source_rx.borrow() == CurveSource::Nats) {
            info!("NATS: idle (not the selected curve source)");
            let stop = tokio::select! {
                r = live_rx.changed() => r.is_err(),
                r = source_rx.changed() => r.is_err(),
            };
            if stop {
                return;
            }
        }

        let started = Instant::now();
        let outcome = run_once(
            &nats_cfg,
            &venue,
            &create_tx,
            &normal_tx,
            &mut live_rx,
            &mut source_rx,
            &cfg,
            &dedupe,
        )
        .await;

        // A connection that ran a while was healthy; only a fast-failing one
        // should climb the ramp.
        if started.elapsed() >= HEALTHY_UPTIME {
            backoff = cfg.reconnect_base;
        }

        match outcome {
            Outcome::Disabled => {
                info!("NATS: curve source deselected - disconnecting");
                backoff = cfg.reconnect_base;
            }
            Outcome::DownstreamClosed => {
                info!("NATS: pipeline receiver dropped - stopping");
                return;
            }
            Outcome::Reconnect(why) => {
                warn!("NATS: {why} - reconnecting in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
            }
        }
    }
}

enum Outcome {
    /// Live mode off or curve source switched away.
    Disabled,
    /// Host dropped the decode lanes — nothing to do.
    DownstreamClosed,
    /// Recoverable failure; caller backs off.
    Reconnect(String),
}

/// What ended the reader task, if anything worth reconnecting over.
type ReaderStop = Option<String>;

#[allow(clippy::too_many_arguments)]
async fn run_once<V: IngestVenue>(
    nats_cfg: &NatsConfig,
    venue: &Arc<V>,
    create_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    normal_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    live_rx: &mut watch::Receiver<bool>,
    source_rx: &mut watch::Receiver<CurveSource>,
    cfg: &TransportConfig,
    dedupe: &SignatureDedupe,
) -> Outcome {
    let mut conn = match NatsConn::connect(&nats_cfg.url, "hunter-ingest", cfg.connect_timeout)
        .await
    {
        Ok(c) => c,
        Err(e) => return Outcome::Reconnect(format!("connect to {} failed: {e}", nats_cfg.url)),
    };

    if let Err(e) = conn
        .subscribe(&nats_cfg.subject, nats_cfg.queue_group.as_deref(), SID)
        .await
    {
        return Outcome::Reconnect(format!("subscribe {} failed: {e}", nats_cfg.subject));
    }

    info!(
        url = %nats_cfg.url,
        subject = %nats_cfg.subject,
        "NATS: connected - curve feed live"
    );

    // Reader -> parser hand-off. Bounded and shed-on-full: see the module docs.
    let (frame_tx, mut frame_rx) = mpsc::channel::<Vec<u8>>(nats_cfg.frame_channel_cap);

    // Reader task: drain the socket as fast as the OS delivers, nothing else.
    let idle_timeout = nats_cfg.idle_reconnect_timeout;
    let subject = nats_cfg.subject.clone();
    let reader = tokio::spawn(async move {
        loop {
            let payload = match timeout(idle_timeout, conn.next_message()).await {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => return Some(format!("read failed: {e}")) as ReaderStop,
                Err(_) => {
                    return Some(format!("no frame on {subject} for {idle_timeout:?}"));
                }
            };

            if frame_tx.try_send(payload).is_err() {
                if frame_tx.is_closed() {
                    return None;
                }
                // Full: the parser is behind. Shedding is what keeps this loop
                // fast enough to avoid a server-side slow-consumer disconnect.
                let n = SHED_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 1_000 == 1 {
                    warn!(
                        shed_total = n,
                        "NATS: parser behind - shedding frames (decode pipeline is the bottleneck)"
                    );
                }
            }
        }
    });

    let mut stats = Stats::default();
    let mut next_stats = Instant::now() + STATS_INTERVAL;
    let mut feed_ended = false;

    let outcome = loop {
        tokio::select! {
            // Bias the frame branch so a busy feed is not starved by the watches.
            biased;

            frame = frame_rx.recv() => {
                let Some(payload) = frame else {
                    feed_ended = true;
                    break Outcome::Reconnect(String::new());
                };
                stats.frames += 1;

                if let Some((update, relevance)) =
                    handle_frame::<V>(&payload, venue, dedupe, &mut stats)
                {
                    let received_at = Utc::now();
                    let lane = if V::is_create_lane(relevance) { create_tx } else { normal_tx };
                    match lane
                        .send_timeout(
                            (Arc::new(update), relevance, received_at),
                            cfg.pipeline_send_timeout,
                        )
                        .await
                    {
                        Ok(()) => stats.decoded += 1,
                        Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                            // Unlike gRPC, do NOT reconnect: there is no replay, so
                            // a reconnect loses these frames anyway and adds a
                            // resubscribe. Shed and keep the feed up.
                            warn!(
                                "NATS: decode lane blocked for {:?} - dropping tx",
                                cfg.pipeline_send_timeout
                            );
                        }
                        Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                            break Outcome::DownstreamClosed;
                        }
                    }
                }

                if Instant::now() >= next_stats {
                    stats.log(&nats_cfg.subject);
                    next_stats = Instant::now() + STATS_INTERVAL;
                }
            }

            r = live_rx.changed() => {
                if r.is_err() { break Outcome::DownstreamClosed; }
                if !*live_rx.borrow() { break Outcome::Disabled; }
            }

            r = source_rx.changed() => {
                if r.is_err() { break Outcome::DownstreamClosed; }
                if *source_rx.borrow() != CurveSource::Nats { break Outcome::Disabled; }
            }
        }
    };

    stats.log(&nats_cfg.subject);

    if feed_ended {
        // The reader stopped; ask it why so the backoff is driven by the real
        // cause rather than an immediate blind retry.
        let why = match reader.await {
            Ok(Some(why)) => why,
            Ok(None) => return Outcome::DownstreamClosed,
            Err(_) => "reader task ended unexpectedly".to_string(),
        };
        return Outcome::Reconnect(why);
    }

    // Aborting drops the connection, which closes the socket and the subscription.
    reader.abort();
    outcome
}

/// Parse one relay frame into a classified update, or `None` if it should not
/// reach the decoder. Counts every rejection reason so a silent feed is
/// diagnosable from the stats line alone.
fn handle_frame<V: IngestVenue>(
    payload: &[u8],
    venue: &Arc<V>,
    dedupe: &SignatureDedupe,
    stats: &mut Stats,
) -> Option<(SubscribeUpdateTransaction, V::Relevance)> {
    let envelope: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => {
            stats.unparseable += 1;
            debug!("NATS: frame is not JSON - {e}");
            return None;
        }
    };

    let result = extract_result(&envelope)?;

    // Match the gRPC subscription's server-side `failed: false` screen.
    if convert::json_tx_failed(result) {
        stats.failed_tx += 1;
        return None;
    }

    let update = match convert::json_tx_to_protobuf(result) {
        Some(u) => u,
        None => {
            stats.unparseable += 1;
            debug!("NATS: frame did not convert to a transaction update");
            return None;
        }
    };

    // Dedupe BEFORE classify: the cheap check should gate the key scan, and a
    // duplicate is a duplicate regardless of how it classifies.
    let signature = update
        .transaction
        .as_ref()
        .map(|t| t.signature.as_slice())?;
    if !dedupe.insert_new(signature) {
        stats.duplicates += 1;
        return None;
    }

    match venue.classify(&update) {
        Some(relevance) => Some((update, relevance)),
        None => {
            stats.irrelevant += 1;
            None
        }
    }
}

/// Unwrap the transaction result from whatever envelope the relay uses.
///
/// Handles the Helius WS notification (`params.result`), a bare JSON-RPC response
/// (`result`), and an already-unwrapped result — so a relay that decides to strip
/// its envelope does not break the feed.
fn extract_result(envelope: &Value) -> Option<&Value> {
    if let Some(r) = envelope.get("params").and_then(|p| p.get("result")) {
        return Some(r);
    }
    if let Some(r) = envelope.get("result") {
        return Some(r);
    }
    envelope.get("transaction").map(|_| envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn result_is_found_in_every_envelope_shape() {
        let inner = json!({"slot": 1, "transaction": {"meta": {}}});
        let wrapped = json!({
            "jsonrpc": "2.0",
            "method": "transactionNotification",
            "params": {"subscription": 7, "result": inner.clone()}
        });
        assert_eq!(extract_result(&wrapped), Some(&inner));

        let rpc = json!({"jsonrpc": "2.0", "id": 1, "result": inner.clone()});
        assert_eq!(extract_result(&rpc), Some(&inner));

        assert_eq!(extract_result(&inner), Some(&inner));

        assert_eq!(extract_result(&json!({"nope": 1})), None);
    }
}
