//! gRPC transport for the LaserStream (Yellowstone) ingest path.
//!
//! Thin wrapper over the generated `GeyserClient`: connect with `x-token` auth
//! over TLS, open a transaction `Subscribe` stream, adapt each update to the
//! decoder's `Value` shape, and feed it to the pipeline. Drives the producer
//! side via the `live_rx` / `pool_index` / `pools_changed` inputs.
//!
//! On reconnect we replay from just after the last slot seen (`from_slot`), so a
//! brief disconnect doesn't lose data; if an attempt makes no progress (e.g. the
//! slot is too old to replay) we fall back to a live subscription.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{mpsc, watch, Notify};
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};
use tracing::{error, info, warn};

use crate::config::constants::PUMP_SWAP_PROGRAM_ID;
use crate::state::ingest_health::IngestHeartbeat;

use super::decoder::TxRelevance;
use super::proto::geyser::geyser_client::GeyserClient;
use super::proto::geyser::subscribe_update::UpdateOneof;
use super::proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterTransactions,
    SubscribeUpdateTransaction,
};

/// Outbound `SubscribeRequest` queue depth (initial subscribe + pool updates).
const REQUEST_QUEUE_CAP: usize = 16;
/// Cap on a decoded gRPC message; bundled txs can be large.
const MAX_DECODING_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
/// Upper bound on the exponential reconnect backoff for a hard-down endpoint.
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);
/// Quiet window for coalescing a burst of pool-set changes into one resubscribe.
const POOL_RESUBSCRIBE_DEBOUNCE: Duration = Duration::from_millis(250);
/// Force a reconnect if no transaction update arrives within this window. The
/// gRPC stream can go silent — server-side stall, a silently-dropped
/// subscription, or a throttle that pauses delivery — WITHOUT sending an error or
/// closing, in which case `stream.message()` blocks forever and ingest freezes
/// with no log (only a manual restart recovers it). Pump.fun's firehose never
/// pauses this long, so a gap this size means a dead stream: tear it down so the
/// outer loop reconnects (replaying from `last_slot`, so no data is lost).
const STREAM_RECONNECT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often the idle-reconnect timer wakes to check the silence gap. A fraction
/// of `STREAM_RECONNECT_IDLE_TIMEOUT` so detection latency is bounded to roughly
/// one tick.
const STREAM_RECONNECT_IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(15);
/// Hard cap on how long the client will block handing a tx to the pipeline. A
/// healthy pipeline drains in microseconds; a stall this long means a downstream
/// consumer (DbWriter / StrategyRunner) wedged on an `.await`, so we tear the stream
/// down rather than park forever on `send().await` — a park here sits OUTSIDE the
/// `select!`, so the idle-reconnect timer could never fire and ingest would freeze
/// with no log. Returning unblocks the task; the process liveness watchdog handles a
/// wedge that survives the reconnect.
const PIPELINE_SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a single stream attempt ended — drives the per-reason reconnect counters
/// and the reconnect log line, so a consumer-lag eviction (`PipelineBackpressure`
/// / a `ResourceExhausted` / `Unavailable` `StreamError`) is distinguishable from
/// an idle stall, a network blip, or a graceful close in the deployed logs.
#[derive(Clone, Copy)]
enum DisconnectReason {
    /// Server FIN, pipeline receiver dropped, or live mode turned off — expected.
    Graceful,
    /// In-stream idle timeout tripped (no slot advance within the window).
    IdleTimeout,
    /// `update_tx.send_timeout` exceeded `PIPELINE_SEND_TIMEOUT` — downstream
    /// (DbWriter / StrategyRunner / Postgres) lagged; the socket was being starved.
    PipelineBackpressure,
    /// gRPC stream or subscribe error, carrying the `tonic` status code so
    /// `Unavailable` / `Internal` / `ResourceExhausted` can be told apart.
    StreamError(tonic::Code),
    /// TCP/TLS connect (or the initial subscribe send) failed before streaming.
    ConnectError,
}

impl DisconnectReason {
    /// Whether this disconnect implies a downstream consumer wedged on an `.await`
    /// (the exact failure the process watchdog exists to restart). Only
    /// `PipelineBackpressure` does — every other reason is an upstream/network event
    /// or a normal close, where the client itself is healthy and reconnecting. The
    /// reconnect path stamps the liveness heartbeat for the healthy reasons so a
    /// routine reconnect (idle-timeout silence + backoff + connect) can't be mistaken
    /// for a stall; it deliberately does NOT stamp on a wedge, so consecutive
    /// backpressure reconnects keep aging the heartbeat until the watchdog fires.
    fn is_downstream_wedge(self) -> bool {
        matches!(self, DisconnectReason::PipelineBackpressure)
    }

    fn label(self) -> String {
        match self {
            DisconnectReason::Graceful => "graceful".to_string(),
            DisconnectReason::IdleTimeout => "idle_timeout".to_string(),
            DisconnectReason::PipelineBackpressure => "pipeline_backpressure".to_string(),
            DisconnectReason::StreamError(code) => format!("stream_error({code:?})"),
            DisconnectReason::ConnectError => "connect_error".to_string(),
        }
    }
}

/// Cumulative per-reason reconnect tally, logged on every reconnect so a
/// dominant arm (e.g. `pipeline_backpressure` climbing = consumer-lag eviction)
/// is visible at a glance without external metrics.
#[derive(Default)]
struct ReconnectCounts {
    graceful: u64,
    idle_timeout: u64,
    backpressure: u64,
    stream_error: u64,
    connect_error: u64,
}

impl ReconnectCounts {
    fn record(&mut self, reason: DisconnectReason) {
        match reason {
            DisconnectReason::Graceful => self.graceful += 1,
            DisconnectReason::IdleTimeout => self.idle_timeout += 1,
            DisconnectReason::PipelineBackpressure => self.backpressure += 1,
            DisconnectReason::StreamError(_) => self.stream_error += 1,
            DisconnectReason::ConnectError => self.connect_error += 1,
        }
    }
}

/// Add 0..50% jitter to a reconnect delay to decorrelate reconnect storms.
/// Derives the jitter from wall-clock subsecond nanos (no rng dependency).
fn with_jitter(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    let extra_ms = (base.as_millis() as u64).saturating_mul(nanos % 50) / 100;
    base + Duration::from_millis(extra_ms)
}

/// Concrete client type once the `x-token` auth interceptor is attached.
pub type LaserStreamClient = GeyserClient<InterceptedService<Channel, XTokenInterceptor>>;

/// Attaches the Helius API key to every gRPC request as `x-token` metadata.
#[derive(Clone)]
pub struct XTokenInterceptor {
    token: MetadataValue<Ascii>,
}

impl Interceptor for XTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        req.metadata_mut().insert("x-token", self.token.clone());
        Ok(req)
    }
}

/// Connect to the LaserStream gRPC endpoint over TLS with `x-token` auth.
pub async fn connect(endpoint: &str, api_key: &str) -> anyhow::Result<LaserStreamClient> {
    let token: MetadataValue<Ascii> = api_key
        .parse()
        .map_err(|_| anyhow::anyhow!("HELIUS_API_KEY is not a valid gRPC metadata value"))?;

    let channel = Endpoint::from_shared(endpoint.to_string())?
        .tls_config(ClientTlsConfig::new())?
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_while_idle(true)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .connect()
        .await?;

    let client = GeyserClient::with_interceptor(channel, XTokenInterceptor { token })
        .max_decoding_message_size(MAX_DECODING_MESSAGE_SIZE);
    Ok(client)
}

/// Build a `SubscribeRequest` for the given `account_include` set at `processed`
/// commitment. Resending a full request replaces the live filter, so callers pass
/// the complete account set (pump.fun program + currently-tracked pools).
///
/// `from_slot` is set only on the initial (re)subscribe to replay a reconnect
/// gap; live filter updates (pool changes) pass `None`.
pub fn build_subscribe_request(
    account_include: Vec<String>,
    from_slot: Option<u64>,
) -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        "pumpfun".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            signature: None,
            account_include,
            account_exclude: Vec::new(),
            account_required: Vec::new(),
        },
    );

    SubscribeRequest {
        transactions,
        commitment: Some(CommitmentLevel::Processed as i32),
        from_slot,
        ..Default::default()
    }
}

/// Cheap pre-filter mirroring the decoder's log gate: classify each tx by its
/// logs (curve takes priority over AMM, exactly as the decoder does) and drop the
/// rest of the subscription firehose before it reaches the pipeline. The verdict
/// is forwarded alongside the tx so the decoder reuses it instead of re-scanning
/// the logs ([`HeliusDecoder::decode_relevant_pb`]). This is the only per-tx work
/// the client task does now; the `Value` blob build it used to run moved
/// off-thread into the DbWriter (Tier B).
fn classify_tx(update: &SubscribeUpdateTransaction, pump_program_id: &str) -> Option<TxRelevance> {
    let meta = update.transaction.as_ref()?.meta.as_ref()?;
    if meta
        .log_messages
        .iter()
        .any(|l| l.contains(pump_program_id))
    {
        Some(TxRelevance::Curve)
    } else if meta
        .log_messages
        .iter()
        .any(|l| l.contains(PUMP_SWAP_PROGRAM_ID))
    {
        Some(TxRelevance::Amm)
    } else {
        None
    }
}

/// Combined `account_include`: the pump.fun curve program plus every currently
/// tracked PumpSwap pool account (the `pool_index` keys).
fn account_includes(pump_program_id: &str, pool_index: &DashMap<String, String>) -> Vec<String> {
    let mut accounts = Vec::with_capacity(pool_index.len() + 1);
    accounts.push(pump_program_id.to_string());
    accounts.extend(pool_index.iter().map(|e| e.key().clone()));
    accounts
}

/// Spawn-ready entry point. Loops forever: (wait for live) → connect → subscribe
/// → stream → reconnect. Pump/AMM-relevant transaction updates (typed protobuf,
/// shared via `Arc`) are forwarded to `update_tx` for the pipeline to decode; the
/// heap-heavy Helius `Value` blob is no longer built here (Tier B — it's
/// synthesised off-thread in the DbWriter for persisted txs only).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    laserstream_url: String,
    api_key: String,
    pump_program_id: String,
    update_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, TxRelevance)>,
    mut live_rx: watch::Receiver<bool>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
    reconnect_interval: Duration,
    heartbeat: IngestHeartbeat,
) {
    // Slot to replay from on the next (re)connect; `None` = live subscription.
    let mut from_slot: Option<u64> = None;
    // Exponential backoff, reset whenever an attempt made progress.
    let mut backoff = reconnect_interval;
    // Per-reason reconnect tally (Step 0 diagnostics).
    let mut counts = ReconnectCounts::default();

    loop {
        while !*live_rx.borrow() {
            info!("LaserStream: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        let last_slot = AtomicU64::new(0);
        let reason = run_once(
            &laserstream_url,
            &api_key,
            &pump_program_id,
            &update_tx,
            &mut live_rx,
            &pool_index,
            &pools_changed,
            from_slot,
            &last_slot,
            &heartbeat,
        )
        .await;

        // Replay the gap next attempt only if we made progress this attempt;
        // otherwise fall back to live so we never get stuck replaying a slot the
        // server can no longer serve (the existing token_sync backfill covers it).
        let seen = last_slot.load(Ordering::Relaxed);
        from_slot = if seen > 0 { Some(seen + 1) } else { None };

        // Per-reason reconnect tally: the detailed error/close was already logged
        // inside `run_once`; here we surface *which* arm fired plus the running
        // totals, so a climbing `pipeline_backpressure` (consumer-lag eviction) is
        // obvious in the deployed logs.
        counts.record(reason);
        info!(
            "LaserStream: reconnect (reason={}) — totals: graceful={} idle_timeout={} \
             pipeline_backpressure={} stream_error={} connect_error={}",
            reason.label(),
            counts.graceful,
            counts.idle_timeout,
            counts.backpressure,
            counts.stream_error,
            counts.connect_error,
        );

        // Liveness heartbeat: a healthy reconnect (idle-timeout silence, a network
        // blip, a graceful close) is NOT a stall, but it forwards no tx — so without
        // this stamp the in-stream idle window (up to STREAM_RECONNECT_IDLE_TIMEOUT)
        // plus the backoff + connect time would age the heartbeat past the process
        // watchdog's stall timeout and restart a perfectly-recovering process. Stamp
        // here so each fresh attempt gets a full timeout window to land its first tx.
        // The one exception is `PipelineBackpressure`: that means a downstream
        // consumer wedged on an `.await` — exactly what the watchdog must restart — so
        // we leave the heartbeat aging and let consecutive backpressure reconnects
        // trip it.
        if !reason.is_downstream_wedge() {
            heartbeat.stamp();
        }

        // A stream that delivered data and then dropped resets the backoff (a
        // long-lived connection shouldn't inherit a long delay); an attempt that
        // made no progress grows the backoff exponentially, capped, so a
        // hard-down endpoint isn't hammered. Jitter decorrelates reconnects.
        let delay = if seen > 0 {
            backoff = reconnect_interval;
            reconnect_interval
        } else {
            let d = backoff;
            backoff = (backoff * 2).min(MAX_RECONNECT_BACKOFF);
            d
        };
        let delay = with_jitter(delay);
        info!("LaserStream: reconnecting in {delay:?}");
        tokio::time::sleep(delay).await;
    }
}

/// Single connection attempt: connect, subscribe, read until error/close, or
/// until live mode is disabled. Re-subscribes (replacing the filter) whenever
/// `pools_changed` fires so newly-migrated tokens' pools are added live. Records
/// the highest slot seen into `last_slot` so the caller can replay the gap.
#[allow(clippy::too_many_arguments)]
async fn run_once(
    laserstream_url: &str,
    api_key: &str,
    pump_program_id: &str,
    update_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, TxRelevance)>,
    live_rx: &mut watch::Receiver<bool>,
    pool_index: &DashMap<String, String>,
    pools_changed: &Notify,
    from_slot: Option<u64>,
    last_slot: &AtomicU64,
    heartbeat: &IngestHeartbeat,
) -> DisconnectReason {
    match from_slot {
        Some(slot) => info!("LaserStream: connecting to {laserstream_url} (replay from slot {slot})"),
        None => info!("LaserStream: connecting to {laserstream_url} (live)"),
    }
    let mut client = match connect(laserstream_url, api_key).await {
        Ok(c) => c,
        Err(e) => {
            error!("LaserStream: connect failed — {e}");
            return DisconnectReason::ConnectError;
        }
    };

    // Outbound request stream: the initial subscription plus later filter updates
    // (pool set changes) are sent through this channel on the one live stream.
    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(account_includes(pump_program_id, pool_index), from_slot);
    if req_tx.send(initial).await.is_err() {
        error!("LaserStream: request channel closed before initial subscribe");
        return DisconnectReason::ConnectError;
    }

    let response = match client.subscribe(ReceiverStream::new(req_rx)).await {
        Ok(r) => r,
        Err(status) => {
            error!("LaserStream: subscribe failed — {status}");
            return DisconnectReason::StreamError(status.code());
        }
    };
    let mut stream = response.into_inner();
    info!(
        "LaserStream: subscribed (curve program + {} pool(s))",
        pool_index.len()
    );

    // Idle-reconnect timer: stamp the last time a transaction update arrived, and a
    // timer that periodically checks the silence gap. If it grows past
    // `STREAM_RECONNECT_IDLE_TIMEOUT` the stream has stalled silently — return an
    // error so the caller reconnects. Resets on transaction updates only (not
    // keepalive pings), so a stream that pings but stops delivering txs is still caught.
    let mut last_update = tokio::time::Instant::now();
    let mut idle_check = tokio::time::interval(STREAM_RECONNECT_IDLE_CHECK_INTERVAL);
    idle_check.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            msg = stream.message() => {
                match msg {
                    Ok(Some(update)) => {
                        if let Some(UpdateOneof::Transaction(tx)) = update.update_oneof {
                            // Reset the idle-reconnect timer only when the chain actually
                            // ADVANCES: `fetch_max` returns the prior high-water slot,
                            // so a stream stuck replaying or repeating a slot makes no
                            // progress and is still allowed to trip the timeout.
                            if tx.slot > last_slot.fetch_max(tx.slot, Ordering::Relaxed) {
                                last_update = tokio::time::Instant::now();
                            }
                            // Cheap protobuf-log pre-filter: forward only txs the
                            // decoder would keep (curve or AMM program in the logs),
                            // tagged with the verdict so the decoder skips re-scanning.
                            // No Value build here anymore (Tier B).
                            if let Some(relevance) = classify_tx(&tx, pump_program_id) {
                                // Bounded send: a downstream stall must surface as a
                                // reconnect, never an invisible infinite park. Parking
                                // on `send().await` sits OUTSIDE this `select!`, so the
                                // idle-reconnect timer above could never fire — the exact
                                // silent-freeze failure mode. A successful send is real
                                // end-to-end progress, so stamp the liveness heartbeat
                                // the process watchdog reads.
                                match update_tx
                                    .send_timeout((Arc::new(tx), relevance), PIPELINE_SEND_TIMEOUT)
                                    .await
                                {
                                    Ok(()) => heartbeat.stamp(),
                                    Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                                        warn!(
                                            "LaserStream: pipeline backpressured for \
                                             {PIPELINE_SEND_TIMEOUT:?} — forcing reconnect \
                                             (downstream stalled)"
                                        );
                                        return DisconnectReason::PipelineBackpressure;
                                    }
                                    Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                                        info!("LaserStream: pipeline receiver dropped — stopping");
                                        return DisconnectReason::Graceful;
                                    }
                                }
                            }
                        }
                        // Ping/Pong/other updates: ignored (HTTP/2 keepalive
                        // keeps the connection alive; we only filter transactions).
                    }
                    Ok(None) => {
                        info!("LaserStream: server closed the stream");
                        return DisconnectReason::Graceful;
                    }
                    Err(status) => {
                        error!("LaserStream: stream error — {status}");
                        return DisconnectReason::StreamError(status.code());
                    }
                }
            }

            // A pool was added/removed — resend the full filter to update the
            // live subscription without dropping the connection. No `from_slot`:
            // this is a live filter update, not a replay.
            _ = pools_changed.notified() => {
                // Debounce: a migration wave fires `pools_changed` repeatedly in
                // quick succession. Coalesce the burst into one resubscribe by
                // waiting for a brief quiet window (resetting on each new change)
                // before rebuilding the filter once.
                loop {
                    tokio::select! {
                        _ = pools_changed.notified() => continue,
                        _ = tokio::time::sleep(POOL_RESUBSCRIBE_DEBOUNCE) => break,
                    }
                }
                let req = build_subscribe_request(
                    account_includes(pump_program_id, pool_index),
                    None,
                );
                if req_tx.send(req).await.is_err() {
                    return DisconnectReason::Graceful;
                }
            }

            // Live mode disabled — close this stream; the outer loop pauses.
            _ = live_rx.changed() => {
                if !*live_rx.borrow() {
                    info!("LaserStream: live mode disabled — closing stream");
                    return DisconnectReason::Graceful;
                }
            }

            // Idle-reconnect tick — if no transaction has arrived within
            // `STREAM_RECONNECT_IDLE_TIMEOUT`, the stream stalled silently (no
            // error/close). Return an error so `run` reconnects and replays from `last_slot`.
            _ = idle_check.tick() => {
                let idle = last_update.elapsed();
                if idle >= STREAM_RECONNECT_IDLE_TIMEOUT {
                    warn!("LaserStream: no transaction update for {idle:?} — forcing reconnect");
                    return DisconnectReason::IdleTimeout;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_backpressure_counts_as_a_downstream_wedge() {
        // Backpressure means a downstream consumer wedged on an `.await` — the one
        // reason the reconnect path must NOT stamp the heartbeat, so the process
        // watchdog can still restart the wedge.
        assert!(DisconnectReason::PipelineBackpressure.is_downstream_wedge());

        // Every other reason is a healthy client reconnecting; stamping keeps a
        // routine reconnect from being mistaken for a stall.
        assert!(!DisconnectReason::Graceful.is_downstream_wedge());
        assert!(!DisconnectReason::IdleTimeout.is_downstream_wedge());
        assert!(!DisconnectReason::ConnectError.is_downstream_wedge());
        assert!(!DisconnectReason::StreamError(tonic::Code::Unavailable).is_downstream_wedge());
    }
}
