//! tpsl2-only **scalp-entry arming** — the entry orchestration that sits *ahead*
//! of the generic snipe buy ([`real::buy_until_filled_or_give_up`](super::real)).
//! tpsl1 buys immediately on a token match; tpsl2 first watches the live trade
//! feed until its scalp-continuation gates hold, then buys at that moment. This
//! module is the live watch; the gate logic ([`find_scalp_entry`]) is the shared,
//! sweep-identical decision in `trading_core`.
//!
//! The watch is bounded two ways so a never-dying token can't pin it (and its
//! `token_cache` entry) forever: a finite `MaxAge` deadline derived from the rule's
//! `p_entry_max_age_secs`, or — when unset — an `UntilDead` slot capped by the
//! runtime cache's until-dead-armer limit (see
//! [`StrategyRuntimeCache::begin_until_dead_armer`]).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::time::{sleep, Instant};
use tracing::debug;
use uuid::Uuid;

use trading_core::strategies::registry::Tpsl2Params;
use trading_core::strategies::tpsl_sniper_2::entry::{find_scalp_entry, EntryFill};

use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use trading_core::strategies::runtime_cache::StrategyRuntimeCache;

/// The live watch budget for a rule, derived from its entry-window ceiling:
/// `Some(max_age)` ⇒ watch at most that long after arming starts; `None` ⇒ no
/// ceiling, so the caller watches until the token dies (capped by the
/// concurrent-armer limit). Mirrors the `max_age` the shared [`find_scalp_entry`]
/// gate enforces, so the live deadline can never admit an entry the gate rejects.
pub(crate) fn scalp_watch_window(params: &Tpsl2Params) -> Option<Duration> {
    params
        .p_entry_max_age_secs
        .filter(|&s| s != 0)
        .map(Duration::from_secs)
}

/// How long the scalp-entry watch keeps watching the feed before giving up.
/// Derived from `p_entry_max_age_secs`: a set ceiling is self-limiting; `None`
/// watches until the token dies (bounded by the global concurrent-armer cap). The
/// shared `find_scalp_entry` ceiling is the real guard, so this is only an
/// early-exit that frees the entry slot promptly.
#[derive(Clone, Copy)]
pub(crate) enum ScalpWatchWindow {
    /// Stop watching once this much wall-clock has elapsed since arming began.
    MaxAge(Duration),
    /// No ceiling: watch until `is_dead` or the concurrent-armer cap evicts.
    UntilDead,
}

/// Window + fallback-tick timing for the scalp-entry watch. The entry signal can
/// take far longer to form than a buy fill takes to index, so this is separate from
/// the buy retry config. `for_params` derives the window from the rule's
/// entry-window ceiling; tests construct it directly.
#[derive(Clone, Copy)]
pub(crate) struct ScalpWaitCfg {
    window: ScalpWatchWindow,
    /// Bounded fallback tick between cache checks when no notify arrives.
    interval: Duration,
}

impl ScalpWaitCfg {
    /// Window derived from the rule's `p_entry_max_age_secs` — a finite ceiling, or
    /// until-dead when unset. No fixed timeout to keep in sync by hand.
    pub(crate) fn for_params(params: &Tpsl2Params) -> Self {
        let window = match scalp_watch_window(params) {
            Some(max) => ScalpWatchWindow::MaxAge(max),
            None => ScalpWatchWindow::UntilDead,
        };
        Self {
            window,
            interval: Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS),
        }
    }
}

/// Wait for the rule's scalp entry signal before any buy is sent: watch the WS-fed
/// trade feed (waking on the `TradeSignals` mint lane) until [`find_scalp_entry`]
/// holds on some trade, arming the snipe buy. Returns the qualifying [`EntryFill`]
/// (the **trigger trade**) once armed, or `None` if the watch ends without a signal
/// (the caller then drops the unentered position, exactly as a missed buy does).
///
/// The watch ends when: a signal fires (returns `Some`); the token dies; the
/// `MaxAge` deadline elapses; or — in `UntilDead` mode — the concurrent-armer cap
/// evicts this watch (see [`StrategyRuntimeCache::begin_until_dead_armer`]). The
/// shared `find_scalp_entry` ceiling guarantees no entry past `max_age` regardless,
/// so the deadline is purely an early-exit that frees the entry slot.
///
/// In real mode the qualifying trade is only the **timing** signal — the actual
/// entry price comes from the wallet's own on-chain fill, recorded later. The
/// returned fill is the *target* (trigger-trade) snapshot, persisted before the buy
/// is sent so the gap between the targeted point and the real fill can be derived.
/// This shares `find_scalp_entry` with the paper poll and the backtest, so all three
/// resolve the same entry moment and live honors `p_entry_*`.
pub(crate) async fn await_scalp_entry_signal(
    mint: &str,
    params: &Tpsl2Params,
    position_id: Uuid,
    token_cache: &TokenCache,
    trade_signals: &Arc<TradeSignals>,
    runtime: &Arc<StrategyRuntimeCache>,
    cfg: ScalpWaitCfg,
) -> Option<EntryFill> {
    // The gate logic consumes a `Tpsl2Rule`; build it once from the parsed params
    // (the universal fields are inert placeholders the scalp gates never read) and
    // reuse it across the loop instead of rebuilding per tick.
    let rule = params.to_rule();

    // Wake the instant a trade lands for this mint instead of re-reading the cache
    // on a fixed timer: the ingest pipeline pings the mint lane right after it
    // appends the trade to the cache. The arming signal watches all trades on the
    // mint (not a single wallet), so it uses the mint-only lane (any wallet wakes
    // it). A bounded fallback tick keeps the wait honest if a notify is missed.
    let guard = trade_signals.register_mint(mint);

    // A bounded (max-age) watch is self-limiting via its deadline; an until-dead
    // watch instead occupies a capped armer slot so a never-dying token can't pin
    // it (and its `token_cache` entry) forever. Held for the watch's lifetime.
    let (armer_guard, deadline) = match cfg.window {
        ScalpWatchWindow::MaxAge(max) => (None, Some(Instant::now() + max)),
        ScalpWatchWindow::UntilDead => (Some(runtime.begin_until_dead_armer(position_id)), None),
    };

    // Re-walk the O(n) `find_scalp_entry` only when the token's (monotonic)
    // `trade_count` advanced; `None` forces the first walk.
    let mut last_count: Option<u64> = None;
    loop {
        // Arm the wakeup BEFORE reading the cache so a trade landing in the gap
        // isn't lost (`notify_waiters` stores no permit).
        let notified = guard.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        // Read the mint's trade window from the in-memory cache (kept current by the
        // WS pipeline for every wallet's trades) instead of an unbounded DB scan.
        if let Some(entry) = token_cache.get(mint) {
            let state = entry.value();
            let trade_count = state.trade_count;
            if last_count != Some(trade_count) {
                last_count = Some(trade_count);
                if let Some(fill) = find_scalp_entry(&state.trades, &rule) {
                    return Some(fill);
                }
            }
            // The token died before the signal formed: a dead token draws no more
            // meaningful trades, so the scalp entry can never fire — stop watching.
            // Returning `None` now lets the caller drop the unentered position
            // promptly (and unpin the dead token from `token_cache`). This is the
            // primary terminator for an until-dead watch.
            if state.is_dead(Utc::now()) {
                debug!(mint = %mint, "scalp arming aborted: token died before entry signal");
                return None;
            }
        }

        // Evicted by a newer until-dead armer because the cap was reached — bail so
        // the slot frees and the unentered position is dropped.
        if let Some(g) = &armer_guard {
            if g.is_cancelled() {
                debug!(mint = %mint, "scalp arming aborted: evicted by the until-dead armer cap");
                return None;
            }
        }

        // Bounded mode: stop at the deadline. Until-dead mode has no time bound — it
        // loops on the fallback tick until death or eviction.
        let now = Instant::now();
        let tick = match deadline {
            Some(dl) => {
                if now >= dl {
                    return None;
                }
                (dl - now).min(cfg.interval)
            }
            None => cfg.interval,
        };
        tokio::select! {
            _ = notified.as_mut() => {}
            _ = sleep(tick) => {}
        }
    }
}
