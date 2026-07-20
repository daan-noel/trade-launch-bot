//! Venue-agnostic gRPC transport task for the LaserStream ingest path.
//!
//! Connects to Yellowstone, opens a transaction `Subscribe` stream, pre-filters
//! each update via the [`IngestVenue`] (log scan), and forwards the raw protobuf
//! to the decode task via a bounded `mpsc` channel. The decode task owns all
//! interpretation.
//!
//! Reconnects automatically with exponential backoff. Replays from the last slot
//! on normal disconnects; falls back to live on pipeline-backpressure or credit
//! exhaustion to avoid self-reinforcing billing storms.
//!
//! Everything here is generic over `V: IngestVenue` (static dispatch): the venue
//! supplies the subscription accounts, the filter-map key, the pre-filter, and
//! the resubscribe signal. Nothing in this module knows about pump.fun.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};
use tracing::{error, info, warn};

use crate::config::Auth;
use crate::proto::geyser::geyser_client::GeyserClient;
use crate::proto::geyser::subscribe_update::UpdateOneof;
use crate::proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterTransactions,
    SubscribeUpdateTransaction,
};
use crate::venue::IngestVenue;

// ── Constants (all backed by IngestConfig in the builder; kept here for
//   the inner run_once which receives them as references) ──────────────────────

const REQUEST_QUEUE_CAP: usize = 16;

// ── Push-feed hooks ───────────────────────────────────────────────────────────

/// Optional push feeds carried on the SAME LaserStream subscription as the
/// transaction stream (gRPC messages are covered by the subscription, not
/// per-call RPC credits). Venue-neutral: the host decides what to do with each
/// update (e.g. bridge block metas into an executor's blockhash cache and nonce
/// account updates into its durable-nonce slots).
///
/// Both callbacks run ON THE TRANSPORT TASK — they must be cheap and
/// non-blocking (parse + store; no I/O, no `.await`).
#[derive(Default)]
pub struct PushHooks {
    /// Extra account pubkeys (base58) subscribed via an `accounts` filter.
    /// Updates arrive at `on_account`. Empty ⇒ no accounts filter.
    pub watch_accounts: Vec<String>,
    /// Called on every `blocks_meta` update with `(slot, blockhash)`. `Some` ⇒
    /// a `blocks_meta` filter is added to the subscription.
    #[allow(clippy::type_complexity)]
    pub on_block_meta: Option<Box<dyn Fn(u64, &str) + Send + Sync>>,
    /// Called on every watched-account update with `(slot, pubkey_base58,
    /// lamports, account_data)`. `lamports` is the account's balance from the
    /// Yellowstone update — the value carrier for System accounts (e.g. a watched
    /// wallet), whose SOL balance isn't in `data`.
    #[allow(clippy::type_complexity)]
    pub on_account: Option<Box<dyn Fn(u64, &str, u64, &[u8]) + Send + Sync>>,
}

impl PushHooks {
    fn wants_blocks_meta(&self) -> bool {
        self.on_block_meta.is_some()
    }

    fn account_filter(&self) -> Vec<String> {
        if self.on_account.is_some() {
            self.watch_accounts.clone()
        } else {
            Vec::new()
        }
    }
}

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

/// Yellowstone auth interceptor. Inserts the `x-token` header when the provider
/// [`Auth`] carries one; a no-auth provider (self-hosted geyser) inserts nothing.
#[derive(Clone)]
pub struct XTokenInterceptor {
    token: Option<MetadataValue<Ascii>>,
}

impl Interceptor for XTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            req.metadata_mut().insert("x-token", token.clone());
        }
        Ok(req)
    }
}

pub async fn connect(
    endpoint: &str,
    auth: &Auth,
    cfg: &TransportConfig,
) -> crate::Result<LaserStreamClient> {
    let token: Option<MetadataValue<Ascii>> = match auth {
        Auth::XToken(key) => Some(key.parse().map_err(|_| {
            crate::error::IngestError::InvalidEndpoint(
                "API key is not a valid gRPC metadata value".into(),
            )
        })?),
        Auth::None => None,
    };

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

/// Build a `Subscribe` request. `filter_key` is the transaction filter-map key
/// (a label the venue owns; was the hardcoded `"pumpfun"`). `blocks_meta` adds
/// a block-meta filter; `watch_accounts` (non-empty) adds an `accounts` filter
/// — both feed the optional [`PushHooks`].
pub fn build_subscribe_request(
    filter_key: &str,
    account_include: Vec<String>,
    from_slot: Option<u64>,
    commitment: CommitmentLevel,
    blocks_meta: bool,
    watch_accounts: Vec<String>,
) -> SubscribeRequest {
    let mut transactions = HashMap::new();
    transactions.insert(
        filter_key.to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            signature: None,
            account_include,
            account_exclude: Vec::new(),
            account_required: Vec::new(),
        },
    );
    let mut req = SubscribeRequest {
        transactions,
        commitment: Some(commitment as i32),
        from_slot,
        ..Default::default()
    };
    if blocks_meta {
        req.blocks_meta
            .insert(filter_key.to_string(), SubscribeRequestFilterBlocksMeta {});
    }
    if !watch_accounts.is_empty() {
        req.accounts.insert(
            filter_key.to_string(),
            SubscribeRequestFilterAccounts {
                account: watch_accounts,
                owner: Vec::new(),
                filters: Vec::new(),
            },
        );
    }
    req
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
        use crate::config::{Commitment, IngestConfig};
        let c = IngestConfig::default();
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
/// stream → reconnect. Forwards `(Arc<SubscribeUpdateTransaction>, V::Relevance,
/// DateTime<Utc>)` to `event_tx` for the decode task.
///
/// `gap_replay_rx` carries `(gap_replay_on_reconnect, gap_replay_max_window_secs)`.
/// When `gap_replay_on_reconnect` is false (default), reconnects always use live
/// mode (no `from_slot`). When true, a `from_slot` is sent only if the gap since
/// last progress is within `gap_replay_max_window_secs`; larger gaps re-subscribe
/// live to avoid replaying a huge backlog.
#[allow(clippy::too_many_arguments)]
pub async fn run<V: IngestVenue>(
    endpoint: String,
    auth: Auth,
    venue: Arc<V>,
    event_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    mut live_rx: watch::Receiver<bool>,
    cfg: Arc<TransportConfig>,
    mut gap_replay_rx: watch::Receiver<(bool, u64)>,
    push: Arc<PushHooks>,
) {
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
            &auth,
            &venue,
            &event_tx,
            &mut live_rx,
            from_slot,
            &last_slot,
            &cfg,
            &push,
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
async fn run_once<V: IngestVenue>(
    endpoint: &str,
    auth: &Auth,
    venue: &Arc<V>,
    event_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    live_rx: &mut watch::Receiver<bool>,
    from_slot: Option<u64>,
    last_slot: &AtomicU64,
    cfg: &TransportConfig,
    push: &PushHooks,
) -> DisconnectReason {
    match from_slot {
        Some(slot) => info!("LaserStream: connecting to {endpoint} (replay from slot {slot})"),
        None => info!("LaserStream: connecting to {endpoint} (live)"),
    }
    let mut client = match connect(endpoint, auth, cfg).await {
        Ok(c) => c,
        Err(e) => {
            error!("LaserStream: connect failed — {e}");
            return DisconnectReason::ConnectError;
        }
    };

    let filter_key = venue.filter_key();
    let pools_changed = venue.pools_changed();

    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(
        filter_key,
        venue.subscription_accounts(),
        from_slot,
        cfg.commitment,
        push.wants_blocks_meta(),
        push.account_filter(),
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
        "LaserStream: subscribed ({} account(s))",
        venue.subscription_accounts().len()
    );

    let mut last_update = tokio::time::Instant::now();
    let mut idle_check = tokio::time::interval(cfg.idle_check_interval);
    idle_check.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            msg = stream.message() => {
                match msg {
                    Ok(Some(update)) => {
                        match update.update_oneof {
                        // Push feeds (cheap host callbacks; see `PushHooks`).
                        // Deliberately do NOT touch `last_update`: the idle
                        // watchdog guards the TRANSACTION stream, and block
                        // metas keep flowing even when it silently dies.
                        Some(UpdateOneof::BlockMeta(meta)) => {
                            if let Some(hook) = &push.on_block_meta {
                                hook(meta.slot, &meta.blockhash);
                            }
                        }
                        Some(UpdateOneof::Account(acc)) => {
                            if let (Some(hook), Some(info)) = (&push.on_account, acc.account.as_ref()) {
                                let pubkey = bs58::encode(&info.pubkey).into_string();
                                hook(acc.slot, &pubkey, info.lamports, &info.data);
                            }
                        }
                        Some(UpdateOneof::Transaction(tx)) => {
                            if tx.slot > last_slot.fetch_max(tx.slot, Ordering::Relaxed) {
                                last_update = tokio::time::Instant::now();
                            }
                            if let Some(relevance) = venue.classify(&tx) {
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
                        _ => {}
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
                    filter_key,
                    venue.subscription_accounts(),
                    None,
                    cfg.commitment,
                    push.wants_blocks_meta(),
                    push.account_filter(),
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

    /// The filter-map key is venue-supplied (was a hardcoded `"pumpfun"`); a
    /// venue can subscribe under any key without a transport change.
    #[test]
    fn build_subscribe_request_honors_venue_filter_key() {
        let req = build_subscribe_request(
            "myvenue",
            vec!["acct".to_string()],
            Some(42),
            CommitmentLevel::Processed,
            false,
            Vec::new(),
        );
        assert!(req.transactions.contains_key("myvenue"));
        assert_eq!(req.from_slot, Some(42));
        assert_eq!(req.transactions["myvenue"].account_include, vec!["acct".to_string()]);
        // No push hooks → no extra filters (a push-less host's subscription is
        // byte-identical to the pre-push one).
        assert!(req.blocks_meta.is_empty());
        assert!(req.accounts.is_empty());
    }

    /// Push hooks ride the SAME subscription: `blocks_meta` + `accounts` filters
    /// appear only when the corresponding hook is set.
    #[test]
    fn build_subscribe_request_adds_push_filters() {
        let req = build_subscribe_request(
            "myvenue",
            Vec::new(),
            None,
            CommitmentLevel::Processed,
            true,
            vec!["nonce1".to_string(), "nonce2".to_string()],
        );
        assert!(req.blocks_meta.contains_key("myvenue"));
        assert_eq!(
            req.accounts["myvenue"].account,
            vec!["nonce1".to_string(), "nonce2".to_string()]
        );
    }

    /// `PushHooks` gating: an account list without an `on_account` hook (or a
    /// hook without accounts) must not create an accounts filter.
    #[test]
    fn push_hooks_gate_their_filters() {
        let none = PushHooks::default();
        assert!(!none.wants_blocks_meta());
        assert!(none.account_filter().is_empty());

        let accounts_no_hook = PushHooks {
            watch_accounts: vec!["a".into()],
            ..Default::default()
        };
        assert!(accounts_no_hook.account_filter().is_empty());

        let wired = PushHooks {
            watch_accounts: vec!["a".into()],
            on_block_meta: Some(Box::new(|_, _| {})),
            on_account: Some(Box::new(|_, _, _, _| {})),
        };
        assert!(wired.wants_blocks_meta());
        assert_eq!(wired.account_filter(), vec!["a".to_string()]);
    }

    /// Provider-as-config: the `x-token` header is inserted only when the
    /// provider [`Auth`] carries one, so a no-auth (self-hosted) provider is a
    /// pure config swap — no crate change.
    #[test]
    fn interceptor_inserts_x_token_only_when_present() {
        let mut with = XTokenInterceptor {
            token: Some("secret".parse().unwrap()),
        };
        let req = with.call(Request::new(())).unwrap();
        assert!(req.metadata().get("x-token").is_some());

        let mut without = XTokenInterceptor { token: None };
        let req = without.call(Request::new(())).unwrap();
        assert!(req.metadata().get("x-token").is_none());
    }

    /// Swapping Helius → Triton/Shyft → self-hosted is only different data
    /// (endpoint + `Auth`); it type-checks with no change to this crate.
    #[test]
    fn provider_swap_is_config() {
        let _helius = (String::from("https://mainnet.helius-rpc.com"), Auth::XToken("k".into()));
        let _triton = (String::from("https://grpc.triton.one"), Auth::XToken("k2".into()));
        let _selfhosted = (String::from("http://localhost:10000"), Auth::None);
    }
}
