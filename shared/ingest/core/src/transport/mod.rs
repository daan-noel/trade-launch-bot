//! Venue-agnostic gRPC transport task for the LaserStream ingest path.
//!
//! Connects to Yellowstone, opens a transaction `Subscribe` stream, pre-filters
//! each update via the [`IngestVenue`] (log scan), and forwards the raw protobuf
//! to the decode task via a bounded `mpsc` channel. The decode task owns all
//! interpretation.
//!
//! Reconnects automatically with exponential backoff. When the host enables
//! gap replay, resumes from a retained [`ReplayAnchor`] that OUTLIVES failed
//! reconnect attempts — the whole point, since the attempt that has to carry the
//! resume point is often not the one that succeeds. Falls back to live on
//! pipeline-backpressure or credit exhaustion (self-reinforcing billing storms),
//! on a gap wider than the operator's window, and after
//! [`MAX_REPLAY_ATTEMPTS`] fruitless replays.
//!
//! Everything here is generic over `V: IngestVenue` (static dispatch): the venue
//! supplies the subscription accounts, the filter-map key, the pre-filter, and
//! the resubscribe signal. Nothing in this module knows about pump.fun.

use std::collections::HashMap;
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

use crate::config::{Auth, CurveSource, SubscriptionRole};
use crate::dedupe::SignatureDedupe;
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

/// Consecutive replay attempts that may make no progress before the anchor is
/// dropped and the transport falls back to live.
///
/// Retaining the anchor across a no-progress attempt is what closes the gap (see
/// [`ReplayAnchor`]) — but unbounded retention can wedge: a provider that refuses
/// the `from_slot` outright (LaserStream only serves a few minutes of history)
/// fails every attempt, and re-asking forever would keep the feed down instead of
/// losing one window. Three attempts is enough to ride out a transient connect
/// failure while still recovering to live in ~seconds.
const MAX_REPLAY_ATTEMPTS: u32 = 3;

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
    /// Called on every `blocks_meta` update with `(slot, blockhash,
    /// block_time_unix_secs)`. `Some` ⇒ a `blocks_meta` filter is added to the
    /// subscription.
    ///
    /// `block_time_unix_secs` is the chain's own clock for that slot and is the
    /// ONLY chain-time reference on the stream — a *transaction* frame carries a
    /// slot but no block time, so a venue decoder has nothing but its own receive
    /// clock to stamp. A host measuring feed lag (`now - block_time`) must read it
    /// here. Resolution is **whole seconds**, so a single sample bounds lag rather
    /// than timing it; the distribution over many slots is the usable signal.
    #[allow(clippy::type_complexity)]
    pub on_block_meta: Option<Box<dyn Fn(u64, &str, Option<i64>) + Send + Sync>>,
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

    /// Whether the push feeds alone justify holding the subscription open.
    ///
    /// Decides what happens when the venue has no accounts to watch (the relay
    /// carries the curve and no pool is tracked yet): with push feeds the stream
    /// stays up carrying only `blocks_meta` / `accounts`, without them it idles.
    fn wants_stream(&self) -> bool {
        self.wants_blocks_meta() || !self.account_filter().is_empty()
    }
}

/// Why a single stream attempt ended.
#[derive(Clone, Copy)]
enum DisconnectReason {
    Graceful,
    /// Decode task / host event consumer is gone — do **not** reconnect.
    DownstreamClosed,
    IdleTimeout,
    PipelineBackpressure,
    StreamError(tonic::Code),
    ConnectError,
}

impl DisconnectReason {
    fn label(self) -> &'static str {
        match self {
            DisconnectReason::Graceful => "graceful",
            DisconnectReason::DownstreamClosed => "downstream_closed",
            DisconnectReason::IdleTimeout => "idle_timeout",
            DisconnectReason::PipelineBackpressure => "pipeline_backpressure",
            DisconnectReason::StreamError(_) => "stream_error",
            DisconnectReason::ConnectError => "connect_error",
        }
    }

    /// Terminal for the transport task — reconnecting cannot help.
    fn is_terminal(self) -> bool {
        matches!(self, DisconnectReason::DownstreamClosed)
    }
}

#[derive(Default)]
struct ReconnectCounts {
    graceful: u64,
    downstream_closed: u64,
    idle_timeout: u64,
    backpressure: u64,
    stream_error: u64,
    connect_error: u64,
}

impl ReconnectCounts {
    fn record(&mut self, reason: DisconnectReason) {
        match reason {
            DisconnectReason::Graceful => self.graceful += 1,
            DisconnectReason::DownstreamClosed => self.downstream_closed += 1,
            DisconnectReason::IdleTimeout => self.idle_timeout += 1,
            DisconnectReason::PipelineBackpressure => self.backpressure += 1,
            DisconnectReason::StreamError(_) => self.stream_error += 1,
            DisconnectReason::ConnectError => self.connect_error += 1,
        }
    }
}

/// Where a reconnect would resume from, and **when** that slot was actually
/// observed.
///
/// Retained across reconnect attempts on purpose. An attempt that makes no
/// progress — a connect failure, a subscribe rejection, or a stream that opens
/// and then goes silent until the idle watchdog trips — must NOT discard the
/// anchor: it is the only record of where the gap starts, and dropping it makes
/// the next (successful) attempt subscribe live and lose the whole window
/// permanently. That was the cause of a 2 min blackout on 2026-07-27 with
/// `gap_replay_on_reconnect = true`.
///
/// `at` is when the transport last *saw* a slot advance, not when the attempt
/// ended — the two differ by the whole idle timeout on a silently-dead stream,
/// and it is `at` that makes `gap_replay_max_window_secs` mean anything.
#[derive(Clone, Copy)]
struct ReplayAnchor {
    slot: u64,
    at: Instant,
}

/// What a single stream attempt achieved.
struct Attempt {
    reason: DisconnectReason,
    /// Highest slot seen on this attempt, and when it arrived. `None` ⇒ the
    /// attempt made no progress at all (see [`ReplayAnchor`]).
    progress: Option<ReplayAnchor>,
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

    Ok(
        GeyserClient::with_interceptor(channel, XTokenInterceptor { token })
            .max_decoding_message_size(cfg.max_decoding_message_size),
    )
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
    // An empty `account_include` is not "no transactions" to Yellowstone — it is
    // a filter that matches EVERY transaction on chain. Omit the filter entirely
    // instead, which is what "watch no transactions" actually means. This is the
    // shape used when the relay carries the curve and no pool is tracked yet: the
    // subscription still exists to carry the push feeds below.
    let mut transactions = HashMap::new();
    if !account_include.is_empty() {
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
    }
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
/// DateTime<Utc>)` to the decode lanes: creates (`V::is_create_lane`) on
/// `create_tx`, everything else on `normal_tx`, so AMM/curve swap volume can
/// never stall a create in the transport→decode queue.
///
/// `gap_replay_rx` carries `(gap_replay_on_reconnect, gap_replay_max_window_secs)`.
/// When `gap_replay_on_reconnect` is false (default), reconnects always use live
/// mode (no `from_slot`). When true, a `from_slot` is sent only if the gap since
/// the last slot was *observed* is within `gap_replay_max_window_secs`; a larger
/// gap re-subscribes live (and drops the anchor) rather than replay a backlog
/// that would cost credits and re-backpressure the pipeline.
///
/// The replay anchor survives attempts that make no progress — see
/// [`ReplayAnchor`]. It is dropped only when replaying would be wrong or futile:
/// a billing-shaped disconnect (the backlog is what caused it), a gap wider than
/// the operator's window, or [`MAX_REPLAY_ATTEMPTS`] consecutive attempts that
/// got nowhere. Losing one window is always preferable to leaving the feed down.
#[allow(clippy::too_many_arguments)]
pub async fn run<V: IngestVenue>(
    endpoint: String,
    auth: Auth,
    venue: Arc<V>,
    create_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    normal_tx: mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    mut live_rx: watch::Receiver<bool>,
    cfg: Arc<TransportConfig>,
    mut gap_replay_rx: watch::Receiver<(bool, u64)>,
    push: Arc<PushHooks>,
    mut source_rx: watch::Receiver<CurveSource>,
    dedupe: Option<Arc<SignatureDedupe>>,
) {
    let mut anchor: Option<ReplayAnchor> = None;
    let mut backoff = cfg.reconnect_base;
    let mut counts = ReconnectCounts::default();
    // Consecutive replay attempts that made no progress (see MAX_REPLAY_ATTEMPTS).
    let mut replay_attempts: u32 = 0;
    let pools_changed = venue.pools_changed();

    loop {
        while !*live_rx.borrow() {
            info!("LaserStream: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        // When another transport carries the bonding curve, this subscription is
        // pool PDAs only — and that set is empty until the first migration is
        // tracked. An empty `account_include` matches EVERY transaction on chain,
        // so
        // idle rather than subscribe to the firehose — unless the push feeds still
        // need the stream: `blocks_meta` backs the host blockhash cache, and losing
        // it silently reverts to paid `getLatestBlockhash` polling.
        let mut role = role_for(*source_rx.borrow_and_update());
        while venue.subscription_accounts(role).is_empty() && !push.wants_stream() {
            info!(
                role = ?role,
                "LaserStream: no accounts to watch — idle until a pool is tracked \
                 or the curve source changes"
            );
            tokio::select! {
                _ = pools_changed.notified() => {}
                r = source_rx.changed() => if r.is_err() { return; },
                r = live_rx.changed() => if r.is_err() { return; },
            }
            if !*live_rx.borrow() {
                break;
            }
            role = role_for(*source_rx.borrow());
        }
        if !*live_rx.borrow() {
            continue;
        }

        // Resolve the resume point HERE, immediately before subscribing, so the
        // window check measures the real age of the gap. (It used to be computed
        // at the end of the previous iteration right after resetting the clock,
        // which made `gap_replay_max_window_secs` unreachable.)
        let (gap_replay_on, gap_max_secs) = *gap_replay_rx.borrow_and_update();
        let from_slot = resolve_from_slot(&mut anchor, gap_replay_on, gap_max_secs);

        let attempt = run_once(
            &endpoint,
            &auth,
            &venue,
            &create_tx,
            &normal_tx,
            &mut live_rx,
            from_slot,
            &cfg,
            &push,
            role,
            &mut source_rx,
            dedupe.as_deref(),
        )
        .await;
        let reason = attempt.reason;

        // Advance the anchor only on real progress; an attempt that saw nothing
        // KEEPS the previous anchor so the gap is still replayable next time —
        // bounded by MAX_REPLAY_ATTEMPTS so a `from_slot` the provider refuses
        // can never wedge the feed.
        match attempt.progress {
            Some(progress) => {
                anchor = Some(progress);
                replay_attempts = 0;
            }
            None if from_slot.is_some() => {
                replay_attempts += 1;
                if replay_attempts >= MAX_REPLAY_ATTEMPTS {
                    warn!(
                        replay_attempts,
                        resume_slot = from_slot.unwrap_or_default(),
                        reason = reason.label(),
                        "LaserStream: replay made no progress in {MAX_REPLAY_ATTEMPTS} attempts \
                         — falling back to live (that window is lost, feed stays up)"
                    );
                    anchor = None;
                    replay_attempts = 0;
                }
            }
            None => {}
        }

        // Billing-shaped disconnects must never replay: the backlog is what
        // caused them, so re-requesting it is self-reinforcing.
        if matches!(
            reason,
            DisconnectReason::PipelineBackpressure
                | DisconnectReason::StreamError(tonic::Code::ResourceExhausted)
        ) {
            if anchor.take().is_some() {
                warn!(
                    reason = reason.label(),
                    "LaserStream: dropping the replay anchor — reconnecting live to avoid a \
                     self-reinforcing backlog (slots since the last one are permanently missing)"
                );
            }
            replay_attempts = 0;
        }

        counts.record(reason);
        if reason.is_terminal() {
            info!(
                "LaserStream: transport stopping (reason={}) — totals: graceful={} \
                 downstream_closed={} idle_timeout={} pipeline_backpressure={} \
                 stream_error={} connect_error={}",
                reason.label(),
                counts.graceful,
                counts.downstream_closed,
                counts.idle_timeout,
                counts.backpressure,
                counts.stream_error,
                counts.connect_error,
            );
            return;
        }

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

        let delay = if attempt.progress.is_some() {
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

// ── Resume-point decision (the ONE place `from_slot` is decided) ──────────────

/// Resolve the `from_slot` for the subscribe that is about to happen, given the
/// retained `anchor` and the operator's live gap-replay settings.
///
/// Consumes the anchor (`take`) only when the gap is too wide to replay — that
/// window is gone for good, and keeping the anchor would just re-log the same
/// refusal on every later reconnect. A disabled toggle leaves the anchor intact
/// so flipping it back on mid-outage still replays, subject to the same window.
///
/// Resumes at `slot + 1`, never `slot`: re-requesting the anchor slot would
/// re-deliver its transactions, and nothing between here and the strategy fold
/// dedups by signature (only the PG insert does, via `ON CONFLICT DO NOTHING`),
/// so a replayed slot would double-count into the live volume/flow metrics. The
/// residual cost is the tail of a slot the stream died mid-way through — bounded
/// by one slot (~400 ms) against the minute-scale gap this exists to close.
fn resolve_from_slot(
    anchor: &mut Option<ReplayAnchor>,
    gap_replay_on: bool,
    gap_max_secs: u64,
) -> Option<u64> {
    let a = (*anchor)?;
    if !gap_replay_on {
        return None;
    }
    let gap_secs = a.at.elapsed().as_secs();
    if gap_secs > gap_max_secs {
        warn!(
            gap_secs,
            gap_max_secs,
            resume_slot = a.slot + 1,
            "LaserStream: gap exceeds replay window — reconnecting live; slots from resume_slot \
             onward are permanently missing"
        );
        *anchor = None;
        return None;
    }
    Some(a.slot + 1)
}

/// Which slice of the venue this transport covers, given who owns the curve feed.
///
/// The whole switch mechanism reduces to this: when NATS carries the curve, the
/// gRPC subscription drops the venue program id and keeps only tracked pools.
fn role_for(source: CurveSource) -> SubscriptionRole {
    match source {
        CurveSource::Grpc => SubscriptionRole::All,
        CurveSource::Nats => SubscriptionRole::AmmOnly,
    }
}

// ── Single connection attempt ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_once<V: IngestVenue>(
    endpoint: &str,
    auth: &Auth,
    venue: &Arc<V>,
    create_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    normal_tx: &mpsc::Sender<(Arc<SubscribeUpdateTransaction>, V::Relevance, DateTime<Utc>)>,
    live_rx: &mut watch::Receiver<bool>,
    from_slot: Option<u64>,
    cfg: &TransportConfig,
    push: &PushHooks,
    mut role: SubscriptionRole,
    source_rx: &mut watch::Receiver<CurveSource>,
    dedupe: Option<&SignatureDedupe>,
) -> Attempt {
    // Highest slot seen on THIS attempt + when it arrived. Local (the attempt is
    // single-tasked) and returned to the caller, which owns the durable anchor.
    let mut progress: Option<ReplayAnchor> = None;
    macro_rules! done {
        ($reason:expr) => {
            return Attempt {
                reason: $reason,
                progress,
            }
        };
    }

    match from_slot {
        Some(slot) => info!("LaserStream: connecting to {endpoint} (replay from slot {slot})"),
        None => info!("LaserStream: connecting to {endpoint} (live)"),
    }
    let mut client = match connect(endpoint, auth, cfg).await {
        Ok(c) => c,
        Err(e) => {
            error!("LaserStream: connect failed — {e}");
            done!(DisconnectReason::ConnectError);
        }
    };

    let filter_key = venue.filter_key();
    let pools_changed = venue.pools_changed();

    let (req_tx, req_rx) = mpsc::channel::<SubscribeRequest>(REQUEST_QUEUE_CAP);
    let initial = build_subscribe_request(
        filter_key,
        venue.subscription_accounts(role),
        from_slot,
        cfg.commitment,
        push.wants_blocks_meta(),
        push.account_filter(),
    );
    if req_tx.send(initial).await.is_err() {
        error!("LaserStream: request channel closed before initial subscribe");
        done!(DisconnectReason::ConnectError);
    }

    let response = match client.subscribe(ReceiverStream::new(req_rx)).await {
        Ok(r) => r,
        Err(status) => {
            error!("LaserStream: subscribe failed — {status}");
            done!(DisconnectReason::StreamError(status.code()));
        }
    };
    let mut stream = response.into_inner();
    info!(
        role = ?role,
        "LaserStream: subscribed ({} account(s))",
        venue.subscription_accounts(role).len()
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
                                hook(meta.slot, &meta.blockhash, meta.block_time.map(|t| t.timestamp));
                            }
                        }
                        Some(UpdateOneof::Account(acc)) => {
                            if let (Some(hook), Some(info)) = (&push.on_account, acc.account.as_ref()) {
                                let pubkey = bs58::encode(&info.pubkey).into_string();
                                hook(acc.slot, &pubkey, info.lamports, &info.data);
                            }
                        }
                        Some(UpdateOneof::Transaction(tx)) => {
                            if progress.is_none_or(|p| tx.slot > p.slot) {
                                let now = tokio::time::Instant::now();
                                // `at` is the observation time of the newest slot —
                                // the anchor timestamp the caller measures the gap
                                // from (see `ReplayAnchor`).
                                progress = Some(ReplayAnchor { slot: tx.slot, at: now });
                                last_update = now;
                            }
                            // A migration tx matches both the curve feed and the
                            // AMM pool filter, so with two transports running the
                            // same signature legitimately arrives twice. `None`
                            // (single transport) skips the check entirely.
                            let fresh = match dedupe {
                                Some(d) => tx
                                    .transaction
                                    .as_ref()
                                    .is_some_and(|t| d.insert_new(&t.signature)),
                                None => true,
                            };
                            if fresh {
                            if let Some(relevance) = venue.classify(&tx) {
                                let received_at = Utc::now();
                                let lane = if V::is_create_lane(relevance) {
                                    create_tx
                                } else {
                                    normal_tx
                                };
                                match lane
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
                                        done!(DisconnectReason::PipelineBackpressure);
                                    }
                                    Err(mpsc::error::SendTimeoutError::Closed(_)) => {
                                        info!(
                                            "LaserStream: pipeline receiver dropped — stopping \
                                             (no reconnect)"
                                        );
                                        done!(DisconnectReason::DownstreamClosed);
                                    }
                                }
                            }
                            }
                        }
                        _ => {}
                        }
                    }
                    Ok(None) => {
                        info!("LaserStream: server closed the stream");
                        done!(DisconnectReason::Graceful);
                    }
                    Err(status) => {
                        error!("LaserStream: stream error — {status}");
                        done!(DisconnectReason::StreamError(status.code()));
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
                    venue.subscription_accounts(role),
                    None,
                    cfg.commitment,
                    push.wants_blocks_meta(),
                    push.account_filter(),
                );
                if req_tx.send(req).await.is_err() {
                    done!(DisconnectReason::Graceful);
                }
            }

            result = live_rx.changed() => {
                if result.is_err() {
                    info!("LaserStream: live-mode sender dropped — stopping (no reconnect)");
                    done!(DisconnectReason::DownstreamClosed);
                }
                if !*live_rx.borrow() {
                    info!("LaserStream: live mode disabled — closing stream");
                    done!(DisconnectReason::Graceful);
                }
            }

            result = source_rx.changed() => {
                if result.is_err() {
                    info!("LaserStream: curve-source sender dropped — stopping (no reconnect)");
                    done!(DisconnectReason::DownstreamClosed);
                }
                let next = role_for(*source_rx.borrow());
                if next != role {
                    role = next;
                    let accounts = venue.subscription_accounts(role);
                    // Nothing left to watch under the new role — hand back to the
                    // outer loop, which idles until a pool is tracked.
                    if accounts.is_empty() && !push.wants_stream() {
                        info!(role = ?role, "LaserStream: curve source changed — nothing to watch, idling");
                        done!(DisconnectReason::Graceful);
                    }
                    info!(
                        role = ?role,
                        "LaserStream: curve source changed — resubscribing ({} account(s))",
                        accounts.len()
                    );
                    // Resubscribe in place: the stream stays open, so the switch
                    // costs no reconnect and leaves no gap on the AMM side.
                    let req = build_subscribe_request(
                        filter_key,
                        accounts,
                        None,
                        cfg.commitment,
                        push.wants_blocks_meta(),
                        push.account_filter(),
                    );
                    if req_tx.send(req).await.is_err() {
                        done!(DisconnectReason::Graceful);
                    }
                }
            }

            _ = idle_check.tick() => {
                let idle = last_update.elapsed();
                if idle >= cfg.idle_reconnect_timeout {
                    warn!("LaserStream: no transaction update for {idle:?} — forcing reconnect");
                    done!(DisconnectReason::IdleTimeout);
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
        assert_eq!(
            DisconnectReason::DownstreamClosed.label(),
            "downstream_closed"
        );
        assert!(DisconnectReason::DownstreamClosed.is_terminal());
        assert!(!DisconnectReason::Graceful.is_terminal());
        assert_eq!(DisconnectReason::IdleTimeout.label(), "idle_timeout");
        assert_eq!(
            DisconnectReason::PipelineBackpressure.label(),
            "pipeline_backpressure"
        );
        assert_eq!(DisconnectReason::ConnectError.label(), "connect_error");
        assert!(DisconnectReason::StreamError(tonic::Code::Unavailable)
            .label()
            .starts_with("stream_error"));
    }

    fn anchor(slot: u64, age_secs: u64) -> Option<ReplayAnchor> {
        Some(ReplayAnchor {
            slot,
            at: Instant::now() - Duration::from_secs(age_secs),
        })
    }

    /// Baseline: a fresh anchor with the toggle on resumes at `slot + 1` — never
    /// at `slot`, which would re-deliver that slot's trades into the live metric
    /// fold (nothing dedups by signature before the fold).
    #[test]
    fn a_fresh_anchor_resumes_one_slot_past_the_last_one_seen() {
        let mut a = anchor(500, 1);
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
        // Still armed — a resume does not consume the anchor.
        assert!(a.is_some());
    }

    /// **The 2026-07-27 blackout.** A reconnect attempt that makes no progress
    /// (connect failure, subscribe rejection, or a stream that opens then goes
    /// silent until the idle watchdog trips) must NOT discard the anchor. It used
    /// to: `from_slot` was recomputed from that attempt's own `last_slot`, so one
    /// empty attempt zeroed it and the next successful attempt subscribed live,
    /// losing the whole window permanently.
    #[test]
    fn a_no_progress_attempt_keeps_the_gap_replayable() {
        let mut a = anchor(500, 1);
        // Attempt 1 replays from 501 and dies having seen nothing → no progress,
        // so the caller leaves `anchor` untouched.
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
        // Attempt 2 must still ask for the SAME resume point, not go live.
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
    }

    /// `gap_replay_max_window_secs` is reachable. It used to be dead: the caller
    /// reset the progress clock immediately before measuring against it, so the
    /// measured gap was always ~0 and a multi-hour backlog would have been
    /// requested in full. A refused gap also drops the anchor — that window is
    /// gone, and keeping it would re-log the refusal on every later reconnect.
    #[test]
    fn a_gap_wider_than_the_window_reconnects_live_and_disarms() {
        let mut a = anchor(500, 301);
        assert_eq!(resolve_from_slot(&mut a, true, 300), None);
        assert!(a.is_none(), "a refused gap must not be retried forever");
        // Exactly at the window is still replayable (inclusive bound).
        let mut edge = anchor(500, 300);
        assert_eq!(resolve_from_slot(&mut edge, true, 300), Some(501));
    }

    /// Toggle off ⇒ live, but the anchor is RETAINED: flipping it back on
    /// mid-outage still replays, subject to the same window check.
    #[test]
    fn the_toggle_gates_replay_without_forgetting_where_we_were() {
        let mut a = anchor(500, 5);
        assert_eq!(resolve_from_slot(&mut a, false, 300), None);
        assert!(a.is_some());
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
    }

    /// Cold start (or after a billing-shaped disconnect dropped the anchor):
    /// nothing to resume from, so live regardless of the toggle.
    #[test]
    fn no_anchor_means_live() {
        let mut none = None;
        assert_eq!(resolve_from_slot(&mut none, true, 300), None);
        assert_eq!(resolve_from_slot(&mut none, false, 300), None);
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
        assert_eq!(
            req.transactions["myvenue"].account_include,
            vec!["acct".to_string()]
        );
        // No push hooks → no extra filters (a push-less host's subscription is
        // byte-identical to the pre-push one).
        assert!(req.blocks_meta.is_empty());
        assert!(req.accounts.is_empty());
    }

    /// Push hooks ride the SAME subscription: `blocks_meta` + `accounts` filters
    /// appear only when the corresponding hook is set.
    /// An empty `account_include` means "watch nothing", but Yellowstone reads it
    /// as "watch everything" — so the filter must be omitted, not sent empty.
    #[test]
    fn an_empty_account_set_omits_the_transactions_filter() {
        let req = build_subscribe_request(
            "pumpfun",
            Vec::new(),
            None,
            CommitmentLevel::Processed,
            true,
            Vec::new(),
        );
        assert!(
            req.transactions.is_empty(),
            "empty account_include must not send a tx filter"
        );
        // The push feed still rides the subscription.
        assert!(req.blocks_meta.contains_key("pumpfun"));

        let req = build_subscribe_request(
            "pumpfun",
            vec!["pool".into()],
            None,
            CommitmentLevel::Processed,
            false,
            Vec::new(),
        );
        assert_eq!(
            req.transactions["pumpfun"].account_include,
            vec!["pool".to_string()]
        );
    }

    #[test]
    fn push_hooks_decide_whether_an_empty_venue_holds_the_stream() {
        let none = PushHooks::default();
        assert!(!none.wants_stream());

        let metas = PushHooks {
            on_block_meta: Some(Box::new(|_, _, _| {})),
            ..Default::default()
        };
        assert!(metas.wants_stream());

        let accounts = PushHooks {
            watch_accounts: vec!["wallet".into()],
            on_account: Some(Box::new(|_, _, _, _| {})),
            ..Default::default()
        };
        assert!(accounts.wants_stream());
    }

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
            on_block_meta: Some(Box::new(|_, _, _| {})),
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
        let _helius = (
            String::from("https://mainnet.helius-rpc.com"),
            Auth::XToken("k".into()),
        );
        let _triton = (
            String::from("https://grpc.triton.one"),
            Auth::XToken("k2".into()),
        );
        let _selfhosted = (String::from("http://localhost:10000"), Auth::None);
    }
}
