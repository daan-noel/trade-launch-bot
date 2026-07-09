//! gRPC transport task for the LaserStream ingest path.
//!
//! Connects to Yellowstone, opens a transaction `Subscribe` stream, pre-filters
//! each update by log messages, and forwards the raw protobuf to the decode task
//! via a bounded `mpsc` channel. The decode task owns all interpretation.
//!
//! Reconnects automatically with exponential backoff. Replays from the last slot
//! on normal disconnects; falls back to live on pipeline-backpressure or credit
//! exhaustion to avoid self-reinforcing billing storms.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{mpsc, watch, Notify};
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};
use tracing::{error, info, warn};

use crate::decode::TxRelevance;
use crate::proto::geyser::geyser_client::GeyserClient;
use crate::proto::geyser::subscribe_update::UpdateOneof;
use crate::proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterTransactions,
    SubscribeUpdateTransaction,
};
use crate::protocol::Protocol;

// ── Constants (all backed by IngestConfig in the builder; kept here for
//   the inner run_once which receives them as references) ──────────────────────

const REQUEST_QUEUE_CAP: usize = 16;

/// Why a single stream attempt ended.
#[derive(Clone, Copy)]
enum DisconnectReason {
    Graceful,
    IdleTimeout,
    PipelineBackpressure,
    StreamError(tonic::Code),
    ConnectError,
}

impl DisconnectReason {
    fn label(self) -> &'static str {
        match self {
            DisconnectReason::Graceful => "graceful",
            DisconnectReason::IdleTimeout => "idle_timeout",
            DisconnectReason::PipelineBackpressure => "pipeline_backpressure",
            DisconnectReason::StreamError(_) => "stream_error",
            DisconnectReason::ConnectError => "connect_error",
        }
    }
}

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

fn with_jitter(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    let extra_ms = (base.as_millis() as u64).saturating_mul(nanos % 50) / 100;
    base + Duration::from_millis(extra_ms)
}

// ── Public client type ────────────────────────────────────────────────────────

pub type LaserStreamClient = GeyserClient<InterceptedService<Channel, XTokenInterceptor>>;

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

pub async fn connect(
    endpoint: &str,
    api_key: &str,
    cfg: &TransportConfig,
) -> crate::Result<LaserStreamClient> {
    let token: MetadataValue<Ascii> = api_key.parse().map_err(|_| {
        crate::error::IngestError::InvalidEndpoint("API key is not a valid gRPC metadata value".into())
    })?;

    let channel = Endpoint::from_shared(endpoint.to_string())
        .map_err(|e| crate::error::IngestError::InvalidEndpoint(e.to_string()))?
        .tls_config(ClientTlsConfig::new())
        .map_err(crate::error::IngestError::Transport)?
        .connect_timeout(cfg.connect_timeout)
        .http2_keep_alive_interval(cfg.http2_keepalive)
        .keep_alive_while_idle(true)
        .tcp_keepalive(Some(cfg.tcp_keepalive))
        .connect()
        .await
        .map_err(crate::error::IngestError::Transport)?;

    Ok(GeyserClient::with_interceptor(channel, XTokenInterceptor { token })
        .max_decoding_message_size(cfg.max_decoding_message_size))
}

pub fn build_subscribe_request(
    account_include: Vec<String>,
    from_slot: Option<u64>,
    commitment: CommitmentLevel,
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
        commitment: Some(commitment as i32),
        from_slot,
        ..Default::default()
    }
}

fn account_includes(pump_fun_id: &str, pool_index: &DashMap<String, String>) -> Vec<String> {
    let mut accounts = Vec::with_capacity(pool_index.len() + 1);
    accounts.push(pump_fun_id.to_string());
    accounts.extend(pool_index.iter().map(|e| e.key().clone()));
    accounts
}

fn classify_tx(
    update: &SubscribeUpdateTransaction,
    pump_fun_id: &str,
    pump_swap_id: &str,
) -> Option<TxRelevance> {
    let meta = update.transaction.as_ref()?.meta.as_ref()?;
    if meta.log_messages.iter().any(|l| l.contains(pump_fun_id)) {
        Some(TxRelevance::Curve)
    } else if meta.log_messages.iter().any(|l| l.contains(pump_swap_id)) {
        Some(TxRelevance::Amm)
    } else {
        None
    }
}

// ── Transport run parameters ──────────────────────────────────────────────────

/// Tunables extracted from `IngestConfig` at `start()` time.
pub struct TransportConfig {
    pub connect_timeout: Duration,
    pub reconnect_base: Duration,
    pub reconnect_max_backoff: Duration,
    pub idle_reconnect_timeout: Duration,
    pub idle_check_interval: Duration,
    pub http2_keepalive: Duration,
    pub tcp_keepalive: Duration,
    pub max_decoding_message_size: usize,
    pub pipeline_send_timeout: Duration,
    pub resubscribe_debounce: Duration,
    pub commitment: CommitmentLevel,
}

impl Default for TransportConfig {
    fn default() -> Self {
        use crate::config::IngestConfig;
        let c = IngestConfig::default();
        use crate::config::Commitment;
        let commitment = match c.commitment {
            Commitment::Processed => CommitmentLevel::Processed,
            Commitment::Confirmed => CommitmentLevel::Confirmed,
            Commitment::Finalized => CommitmentLevel::Finalized,
        };
        Self {
            connect_timeout: c.connect_timeout,
            reconnect_base: c.reconnect_base,
            reconnect_max_backoff: c.reconnect_max_backoff,
            idle_reconnect_timeout: c.idle_reconnect_timeout,
            idle_check_interval: c.idle_check_interval,
            http2_keepalive: c.http2_keepalive,
            tcp_keepalive: c.tcp_keepalive,
            max_decoding_message_size: c.max_decoding_message_size,
            pipeline_send_timeout: c.pipeline_send_timeout,
            resubscribe_debounce: c.resubscribe_debounce,
            commitment,
        }
    }
}

// ── Outer reconnect loop ──────────────────────────────────────────────────────

/// Transport task entry point. Loops: (wait for live) → connect → subscribe →
/// stream → reconnect. Forwards `(Arc<SubscribeUpdateTransaction>, TxRelevance,
/// DateTime<Utc>)` to `event_tx` for the decode task.
///
/// `gap_replay_rx` carries `(gap_replay_on_reconnect, gap_replay_max_window_secs)`.
/// When `gap_replay_on_reconnect` is false (default), reconnects always use live
/// mode (no `from_slot`). When true, a `from_slot` is sent only if the gap since
/// last progress is within `gap_replay_max_window_secs`; larger gaps re-subscribe
/// live to avoid replaying a huge backlog.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    endpoint: String,
    api_key: String,
    protocol: Arc<Protocol>,
    event_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, TxRelevance, chrono::DateTime<Utc>)>,
    mut live_rx: watch::Receiver<bool>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
    cfg: Arc<TransportConfig>,
    mut gap_replay_rx: watch::Receiver<(bool, u64)>,
) {
    let pump_fun_id = protocol.programs.pump_fun.base58.clone();
    let pump_swap_id = protocol.programs.pump_swap.base58.clone();

    let mut from_slot: Option<u64> = None;
    let mut backoff = cfg.reconnect_base;
    let mut counts = ReconnectCounts::default();
    let mut last_progress_at = Instant::now();

    loop {
        while !*live_rx.borrow() {
            info!("LaserStream: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        let last_slot = AtomicU64::new(0);
        let reason = run_once(
            &endpoint,
            &api_key,
            &pump_fun_id,
            &pump_swap_id,
            &event_tx,
            &mut live_rx,
            &pool_index,
            &pools_changed,
            from_slot,
            &last_slot,
            &cfg,
        )
        .await;

        let seen = last_slot.load(Ordering::Relaxed);
        if seen > 0 {
            last_progress_at = Instant::now();
        }

        let is_billing_reason = matches!(
            reason,
            DisconnectReason::PipelineBackpressure
                | DisconnectReason::StreamError(tonic::Code::ResourceExhausted)
        );

        let (gap_replay_on, gap_max_secs) = *gap_replay_rx.borrow_and_update();
        let gap_secs = last_progress_at.elapsed().as_secs();

        from_slot = if seen > 0 && !is_billing_reason && gap_replay_on && gap_secs <= gap_max_secs
        {
            Some(seen + 1)
        } else {
            if seen > 0 && !is_billing_reason && !gap_replay_on {
                // gap-replay disabled by operator — reconnecting live
            } else if gap_replay_on && gap_secs > gap_max_secs {
                warn!(
                    gap_secs,
                    gap_max_secs,
                    "LaserStream: gap exceeds replay window — reconnecting live (no from_slot)"
                );
            }
            None
        };

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

        let delay = if seen > 0 {
            backoff = cfg.reconnect_base;
            cfg.reconnect_base
        } else {
            let d = backoff;
            backoff = (backoff * 2).min(cfg.reconnect_max_backoff);
            d
        };
        let delay = with_jitter(delay);
        info!("LaserStream: reconnecting in {delay:?}");
        tokio::time::sleep(delay).await;
    }
}

// ── Single connection attempt ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_once(
    endpoint: &str,
    api_key: &str,
    pump_fun_id: &str,
    pump_swap_id: &str,
    event_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, TxRelevance, chrono::DateTime<Utc>)>,
    live_rx: &mut watch::Receiver<bool>,
    pool_index: &DashMap<String, String>,
    pools_changed: &Notify,
    from_slot: Option<u64>,
    last_slot: &AtomicU64,
    cfg: &TransportConfig,
) -> DisconnectReason {
    match from_slot {
        Some(slot) => info!("LaserStream: connecting to {endpoint} (replay from slot {slot})"),
        None => info!("LaserStream: connecting to {endpoint} (live)"),
    }
    let mut client = match connect(endpoint, api_key, cfg).await {
        Ok(c) => c,
        Err(e) => {
            error!("LaserStream: connect failed — {e}");
            return DisconnectReason::ConnectError;
        }
    };

    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(
        account_includes(pump_fun_id, pool_index),
        from_slot,
        cfg.commitment,
    );
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

    let mut last_update = tokio::time::Instant::now();
    let mut idle_check = tokio::time::interval(cfg.idle_check_interval);
    idle_check.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            msg = stream.message() => {
                match msg {
                    Ok(Some(update)) => {
                        if let Some(UpdateOneof::Transaction(tx)) = update.update_oneof {
                            if tx.slot > last_slot.fetch_max(tx.slot, Ordering::Relaxed) {
                                last_update = tokio::time::Instant::now();
                            }
                            if let Some(relevance) = classify_tx(&tx, pump_fun_id, pump_swap_id) {
                                let received_at = Utc::now();
                                match event_tx
                                    .send_timeout(
                                        (Arc::new(tx), relevance, received_at),
                                        cfg.pipeline_send_timeout,
                                    )
                                    .await
                                {
                                    Ok(()) => {}
                                    Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
                                        warn!(
                                            "LaserStream: pipeline backpressured for \
                                             {:?} — forcing reconnect (downstream stalled)",
                                            cfg.pipeline_send_timeout
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

            _ = pools_changed.notified() => {
                loop {
                    tokio::select! {
                        _ = pools_changed.notified() => continue,
                        _ = tokio::time::sleep(cfg.resubscribe_debounce) => break,
                    }
                }
                let req = build_subscribe_request(
                    account_includes(pump_fun_id, pool_index),
                    None,
                    cfg.commitment,
                );
                if req_tx.send(req).await.is_err() {
                    return DisconnectReason::Graceful;
                }
            }

            _ = live_rx.changed() => {
                if !*live_rx.borrow() {
                    info!("LaserStream: live mode disabled — closing stream");
                    return DisconnectReason::Graceful;
                }
            }

            _ = idle_check.tick() => {
                let idle = last_update.elapsed();
                if idle >= cfg.idle_reconnect_timeout {
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
    fn reason_labels_are_distinct() {
        assert_eq!(DisconnectReason::Graceful.label(), "graceful");
        assert_eq!(DisconnectReason::IdleTimeout.label(), "idle_timeout");
        assert_eq!(DisconnectReason::PipelineBackpressure.label(), "pipeline_backpressure");
        assert_eq!(DisconnectReason::ConnectError.label(), "connect_error");
        assert!(DisconnectReason::StreamError(tonic::Code::Unavailable).label().starts_with("stream_error"));
    }
}
