//! The one reconnect/route loop, generic over the wire and the venue.
//!
//! Drives a [`Feed`]: (wait for live) → (wait for a non-empty scope) → connect →
//! subscribe → pull updates → route → reconnect. Every policy that used to be
//! written twice — the backoff ramp, the replay anchor, the idle guard, the
//! create/normal lane split, the cross-feed dedupe, the backpressure verdict —
//! lives here once and reads the feed's [`FeedCaps`] where the transports
//! genuinely differ.
//!
//! Nothing in this module knows about gRPC, NATS, JSON, or pump.fun.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tracing::{error, info, warn};

use crate::feed::{Feed, FeedCaps, FeedConn, FeedError, FeedUpdate, StreamScope, Subscription};
use crate::session::FeedLanes;
use crate::venue::IngestVenue;

/// Consecutive replay attempts that may make no progress before the anchor is
/// dropped and the feed falls back to live.
///
/// Retaining the anchor across a no-progress attempt is what closes the gap (see
/// [`ReplayAnchor`]) — but unbounded retention can wedge: a provider that refuses
/// the resume point outright (LaserStream only serves a few minutes of history)
/// fails every attempt, and re-asking forever would keep the feed down instead of
/// losing one window. Three attempts is enough to ride out a transient connect
/// failure while still recovering to live in ~seconds.
const MAX_REPLAY_ATTEMPTS: u32 = 3;

/// How often the running feed counters are logged.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// A connection that survives at least this long is treated as healthy, so the
/// reconnect ramp restarts from the base delay instead of climbing forever.
const HEALTHY_UPTIME: Duration = Duration::from_secs(30);

// ── Policy the supervisor reads ───────────────────────────────────────────────

/// Wire-neutral reconnect and routing policy. One per feed: the assembly root
/// builds it from [`crate::IngestConfig`] plus that feed's own idle timeout,
/// because how long silence is allowed to last is a property of the wire.
#[derive(Debug, Clone)]
pub struct FeedPolicy {
    /// Base reconnect delay (reset by any attempt that made progress).
    pub reconnect_base: Duration,
    /// Upper bound on the exponential reconnect backoff.
    pub reconnect_max_backoff: Duration,
    /// Force a reconnect after this much silence on whatever this feed's
    /// subscription actually guarantees — see [`idle_basis`].
    pub idle_reconnect_timeout: Duration,
    /// How often the idle check fires.
    pub idle_check_interval: Duration,
    /// Hard cap on how long to wait handing an update to a decode lane before
    /// the backpressure verdict applies (shed, or reconnect).
    pub pipeline_send_timeout: Duration,
    /// Quiet window for coalescing a burst of pool-set changes into one
    /// resubscribe.
    pub resubscribe_debounce: Duration,
    pub commitment: crate::config::Commitment,
}

impl Default for FeedPolicy {
    fn default() -> Self {
        let c = crate::config::IngestConfig::default();
        Self {
            reconnect_base: c.reconnect_base,
            reconnect_max_backoff: c.reconnect_max_backoff,
            idle_reconnect_timeout: Duration::from_secs(10),
            idle_check_interval: c.idle_check_interval,
            pipeline_send_timeout: c.pipeline_send_timeout,
            resubscribe_debounce: c.resubscribe_debounce,
            commitment: c.commitment,
        }
    }
}

// ── Internal state ────────────────────────────────────────────────────────────

/// Why a single connection attempt ended.
#[derive(Clone, Copy)]
enum DisconnectReason {
    Graceful,
    /// Decode task / host event consumer is gone — do **not** reconnect.
    DownstreamClosed,
    IdleTimeout,
    PipelineBackpressure,
    /// Provider capacity/billing refusal — must never replay.
    Exhausted,
    StreamError,
    ConnectError,
}

impl DisconnectReason {
    fn label(self) -> &'static str {
        match self {
            DisconnectReason::Graceful => "graceful",
            DisconnectReason::DownstreamClosed => "downstream_closed",
            DisconnectReason::IdleTimeout => "idle_timeout",
            DisconnectReason::PipelineBackpressure => "pipeline_backpressure",
            DisconnectReason::Exhausted => "exhausted",
            DisconnectReason::StreamError => "stream_error",
            DisconnectReason::ConnectError => "connect_error",
        }
    }

    /// Terminal for this task — reconnecting cannot help.
    fn is_terminal(self) -> bool {
        matches!(self, DisconnectReason::DownstreamClosed)
    }

    /// Replaying into this would re-cause it (the backlog is what triggered it).
    fn forbids_replay(self) -> bool {
        matches!(
            self,
            DisconnectReason::PipelineBackpressure | DisconnectReason::Exhausted
        )
    }

    fn of(err: &FeedError) -> Self {
        match err {
            FeedError::Connect(_) => DisconnectReason::ConnectError,
            FeedError::Subscribe(_) | FeedError::Stream(_) => DisconnectReason::StreamError,
            FeedError::Exhausted(_) => DisconnectReason::Exhausted,
            FeedError::Closed => DisconnectReason::Graceful,
        }
    }
}

#[derive(Default)]
struct ReconnectCounts {
    graceful: u64,
    downstream_closed: u64,
    idle_timeout: u64,
    backpressure: u64,
    exhausted: u64,
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
            DisconnectReason::Exhausted => self.exhausted += 1,
            DisconnectReason::StreamError => self.stream_error += 1,
            DisconnectReason::ConnectError => self.connect_error += 1,
        }
    }

    fn log(&self, feed: &str, reason: DisconnectReason, stopping: bool) {
        let verb = if stopping { "stopping" } else { "reconnect" };
        info!(
            feed,
            reason = reason.label(),
            graceful = self.graceful,
            downstream_closed = self.downstream_closed,
            idle_timeout = self.idle_timeout,
            pipeline_backpressure = self.backpressure,
            exhausted = self.exhausted,
            stream_error = self.stream_error,
            connect_error = self.connect_error,
            "ingest: {verb}"
        );
    }
}

/// Running counters for one feed, logged every [`STATS_INTERVAL`].
#[derive(Default)]
struct FeedStats {
    updates: u64,
    routed: u64,
    duplicates: u64,
    irrelevant: u64,
    shed: u64,
}

impl FeedStats {
    /// `accounts` is what the live subscription names right now — the tracked
    /// pool set, which is what the provider bills against. Without it a rising
    /// `updates` count has no denominator: you cannot tell a busy pool from a
    /// leaking pool set.
    fn log(&self, feed: &str, scope: StreamScope, accounts: usize) {
        info!(
            feed,
            program = scope.program,
            pools = scope.pools,
            accounts,
            updates = self.updates,
            routed = self.routed,
            duplicates = self.duplicates,
            irrelevant = self.irrelevant,
            shed = self.shed,
            "ingest: feed stats"
        );
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
/// `at` is when the feed last *saw* a slot advance, not when the attempt ended —
/// the two differ by the whole idle timeout on a silently-dead stream, and it is
/// `at` that makes `gap_replay_max_window_secs` mean anything.
#[derive(Clone, Copy)]
struct ReplayAnchor {
    slot: u64,
    at: Instant,
}

/// What a single connection attempt achieved.
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

// ── Entry point ───────────────────────────────────────────────────────────────

/// Drive one feed until the host drops the decode lanes.
///
/// `scope_rx` is what the assembly root moves to re-point traffic between feeds:
/// an empty scope idles this feed (fully disconnected, costing nothing), and a
/// change on a live connection resubscribes in place where the wire allows it.
/// Both feeds write to the same lanes, so nothing downstream observes a switch.
pub async fn run<V: IngestVenue, F: Feed>(
    feed: F,
    venue: Arc<V>,
    mut lanes: FeedLanes<V>,
    mut scope_rx: watch::Receiver<StreamScope>,
    policy: Arc<FeedPolicy>,
) {
    let name = feed.name();
    let caps = feed.caps();
    let mut anchor: Option<ReplayAnchor> = None;
    let mut backoff = policy.reconnect_base;
    let mut counts = ReconnectCounts::default();
    let mut replay_attempts: u32 = 0;
    let mut warned_no_replay = false;
    let pools_changed = venue.pools_changed();

    loop {
        while !*lanes.live_rx.borrow() {
            info!(feed = name, "ingest: live mode disabled, paused");
            if lanes.live_rx.changed().await.is_err() {
                return;
            }
        }

        // Idle while there is nothing for this feed to carry. Two ways that
        // happens: the assembly root gave it an empty scope (another feed has
        // the traffic), or its scope is pools-only and none is tracked yet —
        // and on a server-filtered wire an empty `account_include` is not
        // "watch nothing", it is "watch the whole chain".
        let mut scope = *scope_rx.borrow_and_update();
        while let Some(why) = idle_reason(caps, &venue, scope, &lanes.push) {
            info!(feed = name, ?scope, "ingest: idle — {why}");
            tokio::select! {
                _ = pools_changed.notified() => {}
                r = scope_rx.changed() => if r.is_err() { return; },
                r = lanes.live_rx.changed() => if r.is_err() { return; },
            }
            if !*lanes.live_rx.borrow() {
                break;
            }
            scope = *scope_rx.borrow();
        }
        if !*lanes.live_rx.borrow() {
            continue;
        }

        // Resolve the resume point HERE, immediately before subscribing, so the
        // window check measures the real age of the gap.
        let (gap_replay_on, gap_max_secs) = *lanes.gap_replay_rx.borrow_and_update();
        let from_slot = if caps.replay {
            resolve_from_slot(&mut anchor, gap_replay_on, gap_max_secs)
        } else {
            if gap_replay_on && !warned_no_replay {
                warned_no_replay = true;
                warn!(
                    feed = name,
                    "ingest: gap replay is on but this feed cannot resume from a slot — \
                     reconnects always come back live and the gap is lost"
                );
            }
            None
        };

        let started = Instant::now();
        let attempt = run_once(
            &feed,
            &venue,
            &mut lanes,
            from_slot,
            &policy,
            scope,
            &mut scope_rx,
        )
        .await;
        let reason = attempt.reason;

        // Advance the anchor only on real progress; an attempt that saw nothing
        // KEEPS the previous anchor so the gap is still replayable next time —
        // bounded by MAX_REPLAY_ATTEMPTS so a resume point the provider refuses
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
                        feed = name,
                        replay_attempts,
                        resume_slot = from_slot.unwrap_or_default(),
                        reason = reason.label(),
                        "ingest: replay made no progress in {MAX_REPLAY_ATTEMPTS} attempts — \
                         falling back to live (that window is lost, feed stays up)"
                    );
                    anchor = None;
                    replay_attempts = 0;
                }
            }
            None => {}
        }

        if reason.forbids_replay() {
            if anchor.take().is_some() {
                warn!(
                    feed = name,
                    reason = reason.label(),
                    "ingest: dropping the replay anchor — reconnecting live to avoid a \
                     self-reinforcing backlog (slots since the last one are permanently missing)"
                );
            }
            replay_attempts = 0;
        }

        counts.record(reason);
        if reason.is_terminal() {
            counts.log(name, reason, true);
            return;
        }
        counts.log(name, reason, false);

        // A connection that ran a while was healthy; only a fast-failing one
        // should climb the ramp.
        let delay = if attempt.progress.is_some() || started.elapsed() >= HEALTHY_UPTIME {
            backoff = policy.reconnect_base;
            policy.reconnect_base
        } else {
            let d = backoff;
            backoff = (backoff * 2).min(policy.reconnect_max_backoff);
            d
        };
        // A deselected feed reconnects the moment it is selected again, so there
        // is nothing to back off from.
        if matches!(reason, DisconnectReason::Graceful) && scope.is_empty() {
            continue;
        }
        let delay = with_jitter(delay);
        info!(feed = name, "ingest: reconnecting in {delay:?}");
        tokio::time::sleep(delay).await;
    }
}

// ── Idle / scope decisions ────────────────────────────────────────────────────

/// Why this feed has nothing to do right now, or `None` to connect.
fn idle_reason<V: IngestVenue>(
    caps: FeedCaps,
    venue: &Arc<V>,
    scope: StreamScope,
    push: &crate::push::PushHooks,
) -> Option<&'static str> {
    if scope.is_empty() {
        return Some("another feed carries this traffic");
    }
    // Only a server-filtered wire can be starved by an empty account set. On a
    // broadcast subject the filter is applied locally, so an empty pool set just
    // means nothing classifies — not a firehose.
    if caps.server_filter
        && venue.subscription_accounts(scope).is_empty()
        && !push.wants_stream()
    {
        return Some("no accounts to watch — waiting for a tracked pool");
    }
    None
}

/// What silence this feed is judged by, and what to call it in the log.
///
/// The guard's premise is "this subscription is never legitimately quiet", so it
/// must measure **what the subscription actually guarantees**:
///
/// - A broadcast subject guarantees *frames*, not their content — the publisher
///   chooses what it sends. Frames are the liveness signal.
/// - A server-filtered subscription that includes the venue program is a
///   firehose: we asked for those transactions, so a transaction gap means the
///   stream died even while other frames keep arriving. That is exactly the
///   silent death this exists to catch.
/// - A server-filtered subscription over tracked pools alone carries 0-14
///   accounts that go minutes without a trade, and **zero** accounts right after
///   a boot. Judging that by transactions force-reconnects a healthy stream every
///   `idle_reconnect_timeout` forever, which churns the provider connection,
///   drops the block-meta stream, and burns the replay anchor on attempts that
///   can never make progress. Block metas arrive ~2.5/s on any live connection,
///   so their absence still catches a dead stream while quiet pools no longer
///   look like one.
/// - With no block-meta subscription there is no such signal, and silence on a
///   narrow filter proves nothing — the guard stands down and the socket is left
///   to the wire's own keepalive.
fn idle_basis(
    caps: FeedCaps,
    scope: StreamScope,
    blocks_meta: bool,
) -> Option<(IdleBasis, &'static str)> {
    if !caps.server_filter {
        return Some((IdleBasis::AnyFrame, "frame"));
    }
    if scope.program {
        return Some((IdleBasis::Transactions, "transaction update"));
    }
    if blocks_meta {
        return Some((IdleBasis::AnyFrame, "stream frame"));
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IdleBasis {
    Transactions,
    AnyFrame,
}

/// Resolve the resume point for the subscribe that is about to happen, given the
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
            "ingest: gap exceeds replay window — reconnecting live; slots from resume_slot \
             onward are permanently missing"
        );
        *anchor = None;
        return None;
    }
    Some(a.slot + 1)
}

/// The wire-neutral request for one connection attempt.
///
/// **Block metas ride a transaction subscription; they never justify one.** They
/// are one metered frame per slot — ~2.5/s, every slot, forever — and the host
/// bridges them into a recent-blockhash cache that only the AMM buy path reads.
/// A server-filtered feed with no account to watch has no AMM pool tracked, so
/// that path has nothing to buy and the frames buy nothing: under
/// `CURVE_SOURCE=nats` with no tracked pool they are the *entire* provider bill.
/// So they are asked for only when the subscription carries transactions, which
/// is exactly when the cache they fill is readable.
///
/// The `accounts` filter is not gated the same way: those pubkeys are the host's
/// own (nonce accounts, its wallet), they update only when the bot itself
/// transacts, and they keep the durable-nonce path armed at feed speed — see
/// [`crate::push::PushHooks::wants_stream`], which is what holds this
/// subscription open once the block metas are gone.
fn build_subscription<V: IngestVenue>(
    venue: &Arc<V>,
    caps: FeedCaps,
    scope: StreamScope,
    from_slot: Option<u64>,
    policy: &FeedPolicy,
    push: &crate::push::PushHooks,
) -> Subscription {
    // A wire with no server-side filter gets no account list to ignore.
    let account_include = if caps.server_filter {
        venue.subscription_accounts(scope)
    } else {
        Vec::new()
    };
    Subscription {
        filter_key: venue.filter_key(),
        blocks_meta: push.wants_blocks_meta() && carries_transactions(caps, &account_include),
        account_include,
        from_slot,
        commitment: policy.commitment,
        watch_accounts: push.account_filter(),
    }
}

/// Whether this subscription asks for any transaction at all.
///
/// A broadcast subject always does — it carries whatever the publisher sends and
/// the filter is applied locally. A server-filtered one does only when it names
/// at least one account: an empty `account_include` is not sent as a filter (it
/// would mean "the whole chain"), so the subscription carries no transactions.
fn carries_transactions(caps: FeedCaps, account_include: &[String]) -> bool {
    !caps.server_filter || !account_include.is_empty()
}

// ── Single connection attempt ─────────────────────────────────────────────────

async fn run_once<V: IngestVenue, F: Feed>(
    feed: &F,
    venue: &Arc<V>,
    lanes: &mut FeedLanes<V>,
    from_slot: Option<u64>,
    policy: &FeedPolicy,
    mut scope: StreamScope,
    scope_rx: &mut watch::Receiver<StreamScope>,
) -> Attempt {
    let name = feed.name();
    let caps = feed.caps();
    // Highest slot seen on THIS attempt + when it arrived. Local (the attempt is
    // single-tasked) and returned to the caller, which owns the durable anchor.
    let mut progress: Option<ReplayAnchor> = None;
    let mut stats = FeedStats::default();

    macro_rules! done {
        ($reason:expr) => {{
            stats.log(name, scope, venue.subscription_accounts(scope).len());
            return Attempt {
                reason: $reason,
                progress,
            };
        }};
    }

    let sub = build_subscription(venue, caps, scope, from_slot, policy, &lanes.push);
    let n_accounts = sub.account_include.len();
    // What THIS subscription asked for, not what the host has a hook for. The
    // idle guard's only liveness signal on a pools-only scope is the block-meta
    // stream, and `build_subscription` declines to ask for it when there are no
    // transactions to carry — judging by a stream that was never requested reads
    // a healthy connection as dead every `idle_reconnect_timeout`, forever.
    // Re-read on every resubscribe below: the scope decides it, and the scope
    // moves.
    let mut sub_blocks_meta = sub.blocks_meta;
    match from_slot {
        Some(slot) => info!(feed = name, ?scope, "ingest: connecting (replay from slot {slot})"),
        None => info!(feed = name, ?scope, "ingest: connecting (live)"),
    }

    let mut conn = match feed.connect(sub).await {
        Ok(c) => c,
        Err(e) => {
            error!(feed = name, "ingest: {e}");
            done!(DisconnectReason::of(&e));
        }
    };
    info!(feed = name, ?scope, "ingest: subscribed ({n_accounts} account(s))");

    // Two clocks, because what counts as evidence of a fault depends on the feed
    // and its scope — see `idle_basis`. `last_update` is transactions only;
    // `last_frame` is any update at all.
    let mut last_update = Instant::now();
    let mut last_frame = last_update;
    let mut next_stats = last_update + STATS_INTERVAL;
    let mut idle_check = tokio::time::interval(policy.idle_check_interval);
    idle_check.tick().await; // consume the immediate first tick

    let pools_changed = venue.pools_changed();

    loop {
        tokio::select! {
            update = conn.next() => {
                let update = match update {
                    Ok(u) => u,
                    Err(e) => {
                        match &e {
                            FeedError::Closed => info!(feed = name, "ingest: {e}"),
                            _ => error!(feed = name, "ingest: {e}"),
                        }
                        done!(DisconnectReason::of(&e));
                    }
                };
                last_frame = Instant::now();
                stats.updates += 1;

                match update {
                    // Push feeds (cheap host callbacks; see `PushHooks`).
                    // Deliberately do NOT touch `last_update`: on a firehose the
                    // idle guard watches the TRANSACTION stream, and block metas
                    // keep flowing even when it silently dies.
                    FeedUpdate::BlockMeta { slot, blockhash, block_time } => {
                        if let Some(hook) = &lanes.push.on_block_meta {
                            hook(slot, &blockhash, block_time);
                        }
                    }
                    FeedUpdate::Account { slot, pubkey, lamports, data } => {
                        if let Some(hook) = &lanes.push.on_account {
                            hook(slot, &pubkey, lamports, &data);
                        }
                    }
                    FeedUpdate::Tick => {}
                    FeedUpdate::Transaction(tx) => {
                        if progress.is_none_or(|p| tx.slot > p.slot) {
                            // `at` is the observation time of the newest slot —
                            // the anchor timestamp the caller measures the gap
                            // from (see `ReplayAnchor`).
                            progress = Some(ReplayAnchor { slot: tx.slot, at: last_frame });
                            last_update = last_frame;
                        }
                        match route(venue, lanes, policy, caps, tx, &mut stats).await {
                            Routed::Ok | Routed::Dropped => {}
                            Routed::Backpressure => done!(DisconnectReason::PipelineBackpressure),
                            Routed::Closed => {
                                info!(feed = name, "ingest: pipeline receiver dropped — stopping");
                                done!(DisconnectReason::DownstreamClosed);
                            }
                        }
                    }
                }

                if Instant::now() >= next_stats {
                    stats.log(name, scope, venue.subscription_accounts(scope).len());
                    next_stats = Instant::now() + STATS_INTERVAL;
                }
            }

            // Pool set changed. Only a server-filtered wire has a filter to
            // update; a broadcast subject already carries everything.
            _ = pools_changed.notified(), if caps.server_filter => {
                loop {
                    tokio::select! {
                        _ = pools_changed.notified() => continue,
                        _ = tokio::time::sleep(policy.resubscribe_debounce) => break,
                    }
                }
                if !caps.in_place_resubscribe {
                    info!(feed = name, "ingest: pool set changed — reconnecting to apply it");
                    done!(DisconnectReason::Graceful);
                }
                let sub = build_subscription(venue, caps, scope, None, policy, &lanes.push);
                sub_blocks_meta = sub.blocks_meta;
                // An in-place resubscribe used to be the one subscription change
                // that logged NOTHING, so `subscribed (N account(s))` stayed the
                // last word in the log while the account list silently grew or
                // shrank underneath it. That is the whole tracked-pool set, and
                // it decides what the provider bills for — say it out loud.
                let n = sub.account_include.len();
                if let Err(e) = conn.resubscribe(sub).await {
                    error!(feed = name, "ingest: resubscribe failed — {e}");
                    done!(DisconnectReason::of(&e));
                }
                info!(
                    feed = name,
                    accounts = n,
                    blocks_meta = sub_blocks_meta,
                    "ingest: pool set changed — resubscribed"
                );
            }

            result = lanes.live_rx.changed() => {
                if result.is_err() {
                    info!(feed = name, "ingest: live-mode sender dropped — stopping");
                    done!(DisconnectReason::DownstreamClosed);
                }
                if !*lanes.live_rx.borrow() {
                    info!(feed = name, "ingest: live mode disabled — closing");
                    done!(DisconnectReason::Graceful);
                }
            }

            result = scope_rx.changed() => {
                if result.is_err() {
                    info!(feed = name, "ingest: scope sender dropped — stopping");
                    done!(DisconnectReason::DownstreamClosed);
                }
                let next = *scope_rx.borrow();
                if next != scope {
                    scope = next;
                    // Nothing left to carry — hand back to the outer loop, which
                    // idles until this feed is given traffic again.
                    if idle_reason(caps, venue, scope, &lanes.push).is_some() {
                        info!(feed = name, ?scope, "ingest: scope changed — nothing to carry, idling");
                        done!(DisconnectReason::Graceful);
                    }
                    if caps.server_filter {
                        if !caps.in_place_resubscribe {
                            info!(feed = name, ?scope, "ingest: scope changed — reconnecting to apply it");
                            done!(DisconnectReason::Graceful);
                        }
                        let sub = build_subscription(venue, caps, scope, None, policy, &lanes.push);
                        sub_blocks_meta = sub.blocks_meta;
                        info!(
                            feed = name, ?scope,
                            "ingest: scope changed — resubscribing ({} account(s))",
                            sub.account_include.len()
                        );
                        // Resubscribe in place: the connection stays open, so the
                        // switch costs no reconnect and leaves no gap.
                        if let Err(e) = conn.resubscribe(sub).await {
                            error!(feed = name, "ingest: resubscribe failed — {e}");
                            done!(DisconnectReason::of(&e));
                        }
                    }
                }
            }

            _ = idle_check.tick() => {
                if let Some((basis, what)) =
                    idle_basis(caps, scope, sub_blocks_meta)
                {
                    let idle = match basis {
                        IdleBasis::Transactions => last_update.elapsed(),
                        IdleBasis::AnyFrame => last_frame.elapsed(),
                    };
                    if idle >= policy.idle_reconnect_timeout {
                        warn!(
                            feed = name, ?scope,
                            "ingest: no {what} for {idle:?} — forcing reconnect"
                        );
                        done!(DisconnectReason::IdleTimeout);
                    }
                }
            }
        }
    }
}

// ── Lane routing (the ONE place an update becomes a decode-lane send) ─────────

enum Routed {
    Ok,
    /// Filtered out (duplicate, or the venue does not want it), or shed under
    /// back-pressure on a feed that cannot replay.
    Dropped,
    Backpressure,
    Closed,
}

/// Dedupe → classify → pick the lane → hand off. Written once so both feeds
/// filter and route identically; the only thing that differs is what a stalled
/// pipeline means, which [`FeedCaps::reconnect_on_backpressure`] answers.
async fn route<V: IngestVenue>(
    venue: &Arc<V>,
    lanes: &FeedLanes<V>,
    policy: &FeedPolicy,
    caps: FeedCaps,
    tx: crate::proto::geyser::SubscribeUpdateTransaction,
    stats: &mut FeedStats,
) -> Routed {
    // Two feeds can legitimately deliver the same signature: during a scope
    // switch (both briefly overlap), and in steady state for a migration tx that
    // touches both the venue program and a tracked pool. `None` (single feed)
    // skips the check entirely.
    if let Some(d) = lanes.dedupe.as_deref() {
        let fresh = tx
            .transaction
            .as_ref()
            .is_some_and(|t| d.insert_new(&t.signature));
        if !fresh {
            stats.duplicates += 1;
            return Routed::Dropped;
        }
    }

    let Some(relevance) = venue.classify(&tx) else {
        stats.irrelevant += 1;
        return Routed::Dropped;
    };

    let lane = if V::is_create_lane(relevance) {
        &lanes.create_tx
    } else {
        &lanes.normal_tx
    };

    match lane
        .send_timeout((Arc::new(tx), relevance, Utc::now()), policy.pipeline_send_timeout)
        .await
    {
        Ok(()) => {
            stats.routed += 1;
            Routed::Ok
        }
        Err(mpsc::error::SendTimeoutError::Timeout(_)) => {
            stats.shed += 1;
            if caps.reconnect_on_backpressure() {
                warn!(
                    "ingest: pipeline backpressured for {:?} — forcing reconnect \
                     (downstream stalled)",
                    policy.pipeline_send_timeout
                );
                Routed::Backpressure
            } else {
                if stats.shed % 1_000 == 1 {
                    warn!(
                        shed_total = stats.shed,
                        "ingest: decode lane blocked for {:?} — dropping tx (this feed cannot \
                         replay, so a reconnect would lose the same frames and cost a resubscribe)",
                        policy.pipeline_send_timeout
                    );
                }
                Routed::Dropped
            }
        }
        Err(mpsc::error::SendTimeoutError::Closed(_)) => Routed::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRPC: FeedCaps = FeedCaps {
        replay: true,
        server_filter: true,
        in_place_resubscribe: true,
    };
    const RELAY: FeedCaps = FeedCaps {
        replay: false,
        server_filter: false,
        in_place_resubscribe: false,
    };

    /// A firehose is judged by transactions: block metas still flowing is exactly
    /// the case where a dead transaction stream must still be caught.
    #[test]
    fn a_firehose_is_judged_by_transactions() {
        let (basis, what) = idle_basis(GRPC, StreamScope::ALL, true).unwrap();
        assert_eq!(basis, IdleBasis::Transactions);
        assert_eq!(what, "transaction update");
    }

    /// A pool filter goes minutes without a trade, so transactions cannot judge
    /// it — the block-meta stream can. Without this the guard force-reconnects a
    /// healthy stream every `idle_reconnect_timeout` forever.
    #[test]
    fn a_quiet_pool_filter_is_judged_by_frames_not_transactions() {
        let (basis, what) = idle_basis(GRPC, StreamScope::POOLS, true).unwrap();
        assert_eq!(basis, IdleBasis::AnyFrame);
        assert_eq!(what, "stream frame");
    }

    /// No block metas subscribed ⇒ no liveness signal on a narrow filter, so
    /// silence proves nothing and the guard stands down rather than guessing.
    #[test]
    fn a_pool_filter_with_no_block_metas_has_no_idle_verdict() {
        assert!(idle_basis(GRPC, StreamScope::POOLS, false).is_none());
    }

    /// **The provider bill under `CURVE_SOURCE=nats`.** A server-filtered
    /// subscription with no account to watch carries no transactions, so it must
    /// not ask for block metas either — one metered frame per slot, forever, to
    /// fill a blockhash cache only the AMM buy path reads, when no AMM pool is
    /// tracked to buy on.
    #[test]
    fn block_metas_are_not_requested_without_a_transaction_filter() {
        assert!(!carries_transactions(GRPC, &[]));
        assert!(carries_transactions(GRPC, &["pool".to_string()]));
        // A broadcast subject applies its filter locally, so an empty account
        // list is not an empty subscription — it still carries the subject.
        assert!(carries_transactions(RELAY, &[]));
    }

    /// **The two must agree.** `idle_basis` judges a pools-only scope by frames
    /// that only the block-meta stream supplies, so it has to be told what the
    /// SUBSCRIPTION asked for — not what the host has a hook for. Feed it
    /// `PushHooks::wants_blocks_meta()` (always true on a host that bridges block
    /// metas) while `build_subscription` declines to request them, and the guard
    /// waits for a stream that was never subscribed: every healthy connection
    /// reads as dead after `idle_reconnect_timeout`, forever.
    #[test]
    fn the_idle_basis_reads_the_subscription_not_the_hook() {
        let host_has_the_hook = true;
        for scope in [StreamScope::POOLS, StreamScope::ALL] {
            let requested = host_has_the_hook && carries_transactions(GRPC, &subscribed(scope));
            match idle_basis(GRPC, scope, requested) {
                // A firehose is judged by its transactions either way.
                Some((basis, _)) if scope.program => assert_eq!(basis, IdleBasis::Transactions),
                // Pools-only with nothing to watch: no transactions requested, so
                // no block metas requested, so nothing to judge silence by.
                verdict => assert!(
                    verdict.is_none(),
                    "a subscription with no block metas must have no idle verdict"
                ),
            }
        }
    }

    /// What `subscription_accounts` yields for a scope, without a venue: the
    /// program id for a curve scope, and nothing for pools-only with no pool
    /// tracked — the state this whole path exists for.
    fn subscribed(scope: StreamScope) -> Vec<String> {
        if scope.program {
            vec!["program".to_string()]
        } else {
            Vec::new()
        }
    }

    /// A broadcast subject guarantees frames, not content: the publisher decides
    /// what is on it, so frames are the only thing whose absence means "dead".
    #[test]
    fn a_broadcast_subject_is_judged_by_frames_whatever_its_scope() {
        let (basis, _) = idle_basis(RELAY, StreamScope::CURVE, false).unwrap();
        assert_eq!(basis, IdleBasis::AnyFrame);
        let (basis, _) = idle_basis(RELAY, StreamScope::ALL, true).unwrap();
        assert_eq!(basis, IdleBasis::AnyFrame);
    }

    #[test]
    fn reason_labels_are_distinct_and_only_downstream_is_terminal() {
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
    }

    /// Billing-shaped disconnects must never replay: the backlog is what caused
    /// them, so re-requesting it is self-reinforcing.
    #[test]
    fn only_billing_shaped_reasons_forbid_replay() {
        assert!(DisconnectReason::PipelineBackpressure.forbids_replay());
        assert!(DisconnectReason::Exhausted.forbids_replay());
        assert!(!DisconnectReason::IdleTimeout.forbids_replay());
        assert!(!DisconnectReason::StreamError.forbids_replay());
        assert!(!DisconnectReason::Graceful.forbids_replay());
    }

    /// A provider that refuses us for capacity reasons is a different reconnect
    /// decision from an ordinary stream error, so the feed must be able to say so.
    #[test]
    fn feed_errors_map_onto_reconnect_reasons() {
        assert!(matches!(
            DisconnectReason::of(&FeedError::Connect("x".into())),
            DisconnectReason::ConnectError
        ));
        assert!(matches!(
            DisconnectReason::of(&FeedError::Exhausted("x".into())),
            DisconnectReason::Exhausted
        ));
        assert!(matches!(
            DisconnectReason::of(&FeedError::Closed),
            DisconnectReason::Graceful
        ));
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
    /// silent until the idle watchdog trips) must NOT discard the anchor.
    #[test]
    fn a_no_progress_attempt_keeps_the_gap_replayable() {
        let mut a = anchor(500, 1);
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
        assert_eq!(resolve_from_slot(&mut a, true, 300), Some(501));
    }

    /// `gap_replay_max_window_secs` is reachable, and a refused gap also drops
    /// the anchor — that window is gone, and keeping it would re-log the refusal
    /// on every later reconnect.
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
}
