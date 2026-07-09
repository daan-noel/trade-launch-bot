//! swing1-only **phase-entry arming** — the entry orchestration that sits *ahead*
//! of the generic snipe buy ([`real::buy_until_filled_or_give_up`](super::real)).
//! Unlike tpsl1 (immediate buy) or tpsl2 (scalp-arm), swing1's real gate is the
//! kill→volume phase latch + confirmed higher-low: this module watches the live
//! trade feed until that latch fires, then buys at that moment. The gate logic
//! ([`find_phase_entry`]) is the shared, sweep/backtest-identical decision in
//! `trading_core`, and is the exact fn the paper entry resolver
//! (`execution::paper::resolve_paper_entry_swing1`) already calls — this module is
//! only the live watch wrapper, mirroring [`super::scalp::await_scalp_entry_signal`].
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

use trading_core::strategies::registry::Swing1Params;
use trading_core::strategies::swing_1::entry::find_phase_entry;
use trading_core::strategies::tpsl_sniper_2::entry::EntryFill;

use crate::state::token_cache::TokenCache;
use crate::state::trade_signals::TradeSignals;
use trading_core::strategies::runtime_cache::StrategyRuntimeCache;

/// The live watch budget for a rule, derived from its entry-window ceiling:
/// `Some(max_age)` ⇒ watch at most that long after arming starts; `None` ⇒ no
/// ceiling, so the caller watches until the token dies (capped by the
/// concurrent-armer limit). Mirrors the `max_age` the shared [`find_phase_entry`]
/// gate enforces, so the live deadline can never admit an entry the gate rejects.
pub(crate) fn swing_watch_window(params: &Swing1Params) -> Option<Duration> {
    params
        .p_entry_max_age_secs
        .filter(|&s| s != 0)
        .map(Duration::from_secs)
}

/// How long the phase-entry watch keeps watching the feed before giving up. Derived
/// from `p_entry_max_age_secs`: a set ceiling is self-limiting; `None` watches until
/// the token dies (bounded by the global concurrent-armer cap). The shared
/// `find_phase_entry` ceiling is the real guard, so this is only an early-exit that
/// frees the entry slot promptly.
#[derive(Clone, Copy)]
pub(crate) enum SwingWatchWindow {
    /// Stop watching once this much wall-clock has elapsed since arming began.
    MaxAge(Duration),
    /// No ceiling: watch until `is_dead` or the concurrent-armer cap evicts.
    UntilDead,
}

/// Window + fallback-tick timing for the phase-entry watch. Mirrors
/// `scalp::ScalpWaitCfg`; `for_params` derives the window from the rule's
/// entry-window ceiling.
#[derive(Clone, Copy)]
pub(crate) struct SwingWaitCfg {
    window: SwingWatchWindow,
    /// Bounded fallback tick between cache checks when no notify arrives.
    interval: Duration,
}

impl SwingWaitCfg {
    /// Window derived from the rule's `p_entry_max_age_secs` — a finite ceiling, or
    /// until-dead when unset. No fixed timeout to keep in sync by hand.
    pub(crate) fn for_params(params: &Swing1Params) -> Self {
        let window = match swing_watch_window(params) {
            Some(max) => SwingWatchWindow::MaxAge(max),
            None => SwingWatchWindow::UntilDead,
        };
        Self {
            window,
            interval: Duration::from_millis(super::SCALP_ENTRY_WAIT_INTERVAL_MS),
        }
    }
}

/// Wait for the rule's phase-entry signal before any buy is sent: watch the WS-fed
/// trade feed (waking on the `TradeSignals` mint lane) until [`find_phase_entry`]
/// holds on some trade — the kill→volume latch plus a confirmed higher-low —
/// arming the snipe buy. Returns the qualifying [`EntryFill`] (the **trigger
/// trade**, already worst-case-spot-priced) once armed, or `None` if the watch ends
/// without a signal (the caller then drops the unentered position, exactly as a
/// missed buy does).
///
/// The watch ends when: a signal fires (returns `Some`); the token dies; the
/// `MaxAge` deadline elapses; or — in `UntilDead` mode — the concurrent-armer cap
/// evicts this watch (see [`StrategyRuntimeCache::begin_until_dead_armer`]). The
/// shared `find_phase_entry` ceiling guarantees no entry past `max_age` regardless,
/// so the deadline is purely an early-exit that frees the entry slot.
///
/// Unlike tpsl2's scalp arm there is **no target↔entry gap**: `find_phase_entry`
/// already resolves the worst-case, canonical-spot-priced fill at the trigger, so
/// the returned fill is recorded as the target directly — mirroring
/// `execution::paper::resolve_paper_entry_swing1`, which calls this same gate.
pub(crate) async fn await_swing_entry_signal(
    mint: &str,
    params: &Swing1Params,
    position_id: Uuid,
    token_cache: &TokenCache,
    trade_signals: &Arc<TradeSignals>,
    runtime: &Arc<StrategyRuntimeCache>,
    cfg: SwingWaitCfg,
) -> Option<EntryFill> {
    // The gate logic consumes a `Swing1Rule`; build it once from the parsed params
    // and reuse it across the loop instead of rebuilding per tick.
    let rule = params.to_rule();

    // Wake the instant a trade lands for this mint instead of re-reading the cache
    // on a fixed timer — same wake pattern as the scalp watch.
    let guard = trade_signals.register_mint(mint);

    // A bounded (max-age) watch is self-limiting via its deadline; an until-dead
    // watch instead occupies a capped armer slot so a never-dying token can't pin
    // it (and its `token_cache` entry) forever. Held for the watch's lifetime.
    let (armer_guard, deadline) = match cfg.window {
        SwingWatchWindow::MaxAge(max) => (None, Some(Instant::now() + max)),
        SwingWatchWindow::UntilDead => (Some(runtime.begin_until_dead_armer(position_id)), None),
    };

    // Re-walk the O(n) `find_phase_entry` only when the token's (monotonic)
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
                if let Some((_trigger_idx, fill)) = find_phase_entry(&state.trades, &rule) {
                    return Some(fill);
                }
            }
            // The token died before the signal formed: a dead token draws no more
            // meaningful trades, so the phase entry can never fire — stop watching.
            // Returning `None` now lets the caller drop the unentered position
            // promptly (and unpin the dead token from `token_cache`). This is the
            // primary terminator for an until-dead watch.
            if state.is_dead(Utc::now()) {
                debug!(mint = %mint, "swing arming aborted: token died before entry signal");
                return None;
            }
        }

        // Evicted by a newer until-dead armer because the cap was reached — bail so
        // the slot frees and the unentered position is dropped.
        if let Some(g) = &armer_guard {
            if g.is_cancelled() {
                debug!(mint = %mint, "swing arming aborted: evicted by the until-dead armer cap");
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
