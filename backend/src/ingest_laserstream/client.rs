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
use serde_json::Value;
use tokio::sync::{mpsc, watch, Notify};
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};
use tracing::{error, info};

use super::adapter;
use super::profile;
use super::proto::geyser::geyser_client::GeyserClient;
use super::proto::geyser::subscribe_update::UpdateOneof;
use super::proto::geyser::{CommitmentLevel, SubscribeRequest, SubscribeRequestFilterTransactions};

/// Outbound `SubscribeRequest` queue depth (initial subscribe + pool updates).
const REQUEST_QUEUE_CAP: usize = 16;
/// Cap on a decoded gRPC message; bundled txs can be large.
const MAX_DECODING_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
/// Upper bound on the exponential reconnect backoff for a hard-down endpoint.
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);
/// Quiet window for coalescing a burst of pool-set changes into one resubscribe.
const POOL_RESUBSCRIBE_DEBOUNCE: Duration = Duration::from_millis(250);

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

/// Combined `account_include`: the pump.fun curve program plus every currently
/// tracked PumpSwap pool account (the `pool_index` keys).
fn account_includes(pump_program_id: &str, pool_index: &DashMap<String, String>) -> Vec<String> {
    let mut accounts = Vec::with_capacity(pool_index.len() + 1);
    accounts.push(pump_program_id.to_string());
    accounts.extend(pool_index.iter().map(|e| e.key().clone()));
    accounts
}

/// Spawn-ready entry point. Loops forever: (wait for live) → connect → subscribe
/// → stream → reconnect. Adapted transaction `Value`s are forwarded to `value_tx`
/// for the pipeline to decode.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    laserstream_url: String,
    api_key: String,
    pump_program_id: String,
    value_tx: mpsc::Sender<Value>,
    mut live_rx: watch::Receiver<bool>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
    reconnect_interval: Duration,
) {
    // Slot to replay from on the next (re)connect; `None` = live subscription.
    let mut from_slot: Option<u64> = None;
    // Exponential backoff, reset whenever an attempt made progress.
    let mut backoff = reconnect_interval;

    loop {
        while !*live_rx.borrow() {
            info!("LaserStream: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        let last_slot = AtomicU64::new(0);
        let result = run_once(
            &laserstream_url,
            &api_key,
            &pump_program_id,
            &value_tx,
            &mut live_rx,
            &pool_index,
            &pools_changed,
            from_slot,
            &last_slot,
        )
        .await;

        // Replay the gap next attempt only if we made progress this attempt;
        // otherwise fall back to live so we never get stuck replaying a slot the
        // server can no longer serve (the existing token_sync backfill covers it).
        let seen = last_slot.load(Ordering::Relaxed);
        from_slot = if seen > 0 { Some(seen + 1) } else { None };

        match &result {
            Ok(()) => info!("LaserStream: stream closed gracefully"),
            Err(e) => error!("LaserStream: stream error — {e}"),
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
    value_tx: &mpsc::Sender<Value>,
    live_rx: &mut watch::Receiver<bool>,
    pool_index: &DashMap<String, String>,
    pools_changed: &Notify,
    from_slot: Option<u64>,
    last_slot: &AtomicU64,
) -> anyhow::Result<()> {
    match from_slot {
        Some(slot) => info!("LaserStream: connecting to {laserstream_url} (replay from slot {slot})"),
        None => info!("LaserStream: connecting to {laserstream_url} (live)"),
    }
    let mut client = connect(laserstream_url, api_key).await?;

    // Outbound request stream: the initial subscription plus later filter updates
    // (pool set changes) are sent through this channel on the one live stream.
    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(account_includes(pump_program_id, pool_index), from_slot);
    req_tx
        .send(initial)
        .await
        .map_err(|_| anyhow::anyhow!("request channel closed before initial subscribe"))?;

    let response = client.subscribe(ReceiverStream::new(req_rx)).await?;
    let mut stream = response.into_inner();
    info!(
        "LaserStream: subscribed (curve program + {} pool(s))",
        pool_index.len()
    );

    loop {
        tokio::select! {
            msg = stream.message() => {
                match msg {
                    Ok(Some(update)) => {
                        if let Some(UpdateOneof::Transaction(tx)) = update.update_oneof {
                            last_slot.fetch_max(tx.slot, Ordering::Relaxed);
                            let _span = profile::start();
                            let built = adapter::update_tx_to_value(&tx, pump_program_id);
                            profile::record_adapter(_span, built.is_some());
                            if let Some(value) = built {
                                if value_tx.send(value).await.is_err() {
                                    info!("LaserStream: pipeline receiver dropped — stopping");
                                    return Ok(());
                                }
                            }
                        }
                        // Ping/Pong/other updates: ignored (HTTP/2 keepalive
                        // keeps the connection alive; we only filter transactions).
                    }
                    Ok(None) => {
                        info!("LaserStream: server closed the stream");
                        return Ok(());
                    }
                    Err(status) => {
                        return Err(anyhow::anyhow!("stream error: {status}"));
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
                    return Ok(());
                }
            }

            // Live mode disabled — close this stream; the outer loop pauses.
            _ = live_rx.changed() => {
                if !*live_rx.borrow() {
                    info!("LaserStream: live mode disabled — closing stream");
                    return Ok(());
                }
            }
        }
    }
}
