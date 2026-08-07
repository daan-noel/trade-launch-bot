//! Paper execution — a transaction-free fill model that turns a
//! `SubmitBuy`/`SubmitSell` into a `FillConfirmed` the engine folds exactly like a
//! real fill.
//!
//! Fills use the **worst-case** adverse window shared with lab simulate/sweep
//! ([`trading_core::strategies::paper_fill`]): after the trigger/fire trade, the
//! fill is the highest buy (entry) or lowest price (exit) in trigger slot `S` +
//! the next observed slot within [`MAX_FILL_WAIT_SLOTS`]. The executor polls the
//! token-cache trade feed until that window is indexed (or a short deadline).
//! **Both legs** use `market_fill_on_empty_window = true`, exactly like lab
//! replay/simulate and the sweep: an empty window books the trigger/fire trade's
//! own spot instead of failing.
//!
//! The exit leg used to pass `false` ("don't invent a sell print"), which stranded
//! the position instead: a `Dead` exit fires *because* the token stopped trading,
//! so its window is empty by construction, each of the engine's
//! `MAX_EXIT_ATTEMPTS` retries re-fired against that same last trade, and the row
//! landed in `ExitStuck` — which no reaper path can reach (they are all
//! `mode = 'real'`). That was 333 of 742 paper positions (45%), and since the
//! stranded rows are exactly the dead-token losers it biased paper PnL upward.
//! Paper owns no on-chain bag, so a strict window protects nothing here; when the
//! window cannot price the exit at all the position closes at the token's last
//! known spot ([`last_known_price_fill`]) rather than never closing.
//!
//! There is no on-chain identity for a paper fill, so the executor stashes no
//! signatures — the sink's `record_entry_fill`/`close` just see an empty sig list.
//!
//! `Fill::price` is the feed's `price_per_token` = **SOL per RAW token unit**
//! (`Trade::new`: `amount_sol / token_amount`, count in raw units), so a paper
//! `token_amount` is `sol / price` with **no** decimal scaling — the same raw-unit
//! convention `entry_price`/`exit_price` and the real executor use. Scaling it by
//! 1e6 kept SOL PnL right (the factor cancels on the exit leg) but inflated every
//! stored token count 1e6×, which made the repo's post-close
//! `exit_price = exit_sol / sold_token_amount` 1e6× too small and pinned the
//! positions PnL% cell at −100% on every paper row.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use trading_core::state::token_cache::{CachedTrade, TokenCache};
use trading_core::strategies::paper_fill::{
    exit_fill_window_closed, find_worst_case_paper_entry_at, find_worst_case_paper_exit_at,
};

use super::{PaperTarget, PositionId, PositionRegistry};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// How long to wait for the fill window to index before giving up.
const FILL_WAIT: Duration = Duration::from_secs(2);
const FILL_POLL: Duration = Duration::from_millis(100);

/// Fill a paper buy at the worst-case adverse price after `trigger_abs_idx`.
///
/// `trigger_abs_idx` is the absolute cache trade index of the deciding trade
/// (`trades_base + offset`). `None` means no trade yet (enter-on-arm) — wait for
/// the first print and use that as the trigger. When a trigger resolves, its
/// snapshot is written to [`PositionMeta::paper_target`] so the sink can persist
/// `target_*` alongside the worst-case entry fill. Empty window market-fills at
/// the trigger (`market_fill_on_empty_window = true`) so live paper takes the
/// same position set as lab replay/sweep.
pub async fn run_entry(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    registry: PositionRegistry,
    position: Option<PositionId>,
    intent: IntentId,
    mint: String,
    lamports: u64,
    trigger_abs_idx: Option<u64>,
) {
    let event = match wait_entry_fill(&token_cache, &mint, trigger_abs_idx).await {
        Some((fill, trigger)) => {
            if let Some(pid) = position {
                registry.update(pid, |m| m.paper_target = Some(trigger));
            }
            let sol = lamports as f64 / LAMPORTS_PER_SOL;
            // `fill.price` is SOL per RAW unit ⇒ the quotient is already raw units.
            let token_amount = (sol / fill.price).round().max(0.0) as u64;
            Event::FillConfirmed {
                intent,
                fill: Fill {
                    price: fill.price,
                    sol,
                    token_amount,
                    at: fill.block_time,
                },
            }
        }
        None => Event::FillFailed {
            intent,
            reason: FillFailReason::Timeout,
            at: Some(Utc::now()),
        },
    };
    let _ = fill_tx.send(event).await;
}

/// Fill a paper sell at the worst-case adverse price after `fire_abs_idx`.
/// An empty window market-fills at the fire trade (`true`, same as replay/sweep);
/// a window that cannot be priced at all falls back to the token's last known
/// spot. `FillFailed` is left for the one case with no price anywhere — an
/// unknown/never-priced mint.
///
/// `token_amount` is the portion already sized by the decision loop
/// (`Portion::token_amount`) — full remainder or a scale-out leg.
pub async fn run_exit(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    intent: IntentId,
    mint: String,
    token_amount: u64,
    fire_abs_idx: Option<u64>,
) {
    let event = match wait_exit_fill(&token_cache, &mint, fire_abs_idx).await {
        Some(fill) => {
            let sol = token_amount as f64 * fill.price;
            Event::FillConfirmed {
                intent,
                fill: Fill {
                    price: fill.price,
                    sol,
                    token_amount,
                    at: fill.block_time,
                },
            }
        }
        None => Event::FillFailed {
            intent,
            reason: FillFailReason::Timeout,
            at: Some(Utc::now()),
        },
    };
    let _ = fill_tx.send(event).await;
}

struct ResolvedFill {
    price: f64,
    block_time: chrono::DateTime<chrono::Utc>,
}

fn paper_target_from(t: &CachedTrade) -> PaperTarget {
    PaperTarget {
        price: t.price_per_token,
        // `CachedTrade::token_amount` is already raw SPL units (same as entry fill).
        token_amount: t.token_amount.round().max(0.0) as u64,
        time: t.block_time,
        // Cache rows are signature-free; sink persists an empty tx (UI still shows
        // the target↔entry price gap).
        tx: String::new(),
    }
}

async fn wait_entry_fill(
    token_cache: &Arc<TokenCache>,
    mint: &str,
    mut trigger_abs: Option<u64>,
) -> Option<(ResolvedFill, PaperTarget)> {
    let deadline = tokio::time::Instant::now() + FILL_WAIT;
    loop {
        if let Some((trades, base)) = cache_trades(token_cache, mint) {
            if trigger_abs.is_none() {
                if !trades.is_empty() {
                    trigger_abs = Some(base + trades.len() as u64 - 1);
                }
            }
            if let Some(t_abs) = trigger_abs {
                if let Some(rel) = abs_to_rel(t_abs, base, trades.len()) {
                    let trigger_slot = trades[rel].slot;
                    let max_slot = trades.last().map(|t| t.slot).unwrap_or(trigger_slot);
                    let timed_out = tokio::time::Instant::now() >= deadline;
                    if exit_fill_window_closed(trigger_slot, max_slot) || timed_out {
                        let trigger = paper_target_from(&trades[rel]);
                        return find_worst_case_paper_entry_at(trades.as_slice(), rel, true).map(
                            |f| {
                                (
                                    ResolvedFill { price: f.price, block_time: f.block_time },
                                    trigger,
                                )
                            },
                        );
                    }
                } else if t_abs < base {
                    // Trigger trimmed out of the retained window — fail closed.
                    return None;
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            // Last chance resolve if we have a trigger in-window.
            if let (Some(t_abs), Some((trades, base))) =
                (trigger_abs, cache_trades(token_cache, mint))
            {
                if let Some(rel) = abs_to_rel(t_abs, base, trades.len()) {
                    let trigger = paper_target_from(&trades[rel]);
                    return find_worst_case_paper_entry_at(trades.as_slice(), rel, true).map(|f| {
                        (
                            ResolvedFill { price: f.price, block_time: f.block_time },
                            trigger,
                        )
                    });
                }
            }
            return None;
        }
        tokio::time::sleep(FILL_POLL).await;
    }
}

async fn wait_exit_fill(
    token_cache: &Arc<TokenCache>,
    mint: &str,
    fire_abs: Option<u64>,
) -> Option<ResolvedFill> {
    // No fire trade at all (mint never cached / every trade trimmed) — there is no
    // window to wait for, so price it at the last known spot straight away.
    let Some(fire_abs) = fire_abs.or_else(|| latest_trade_abs_idx(token_cache, mint)) else {
        return last_known_price_fill(token_cache, mint);
    };
    let deadline = tokio::time::Instant::now() + FILL_WAIT;
    loop {
        let timed_out = tokio::time::Instant::now() >= deadline;
        if let Some((trades, base)) = cache_trades(token_cache, mint) {
            match abs_to_rel(fire_abs, base, trades.len()) {
                Some(rel) => {
                    let fire_slot = trades[rel].slot;
                    let max_slot = trades.last().map(|t| t.slot).unwrap_or(fire_slot);
                    if exit_fill_window_closed(fire_slot, max_slot) || timed_out {
                        return find_worst_case_paper_exit_at(trades.as_slice(), rel, true)
                            .map(|f| ResolvedFill { price: f.price, block_time: f.block_time })
                            .or_else(|| last_known_price_fill(token_cache, mint));
                    }
                }
                // Fire trade trimmed out of the retained window while we waited.
                None if fire_abs < base => return last_known_price_fill(token_cache, mint),
                None => {}
            }
        }
        if timed_out {
            return last_known_price_fill(token_cache, mint);
        }
        tokio::time::sleep(FILL_POLL).await;
    }
}

/// Last-resort exit price: the token's last known spot from the cache, stamped at
/// its last trade. Reached only when the fill window itself cannot be priced (dead
/// token whose trades were trimmed, or a manual close on a mint with nothing
/// cached). Paper holds no on-chain bag, so closing at the last observed price is
/// strictly more honest than leaving the row permanently open — the analysis
/// death-close (`ExitReason::Dead`) books a silent-death token the same way.
/// `None` only for a mint the cache has never priced.
fn last_known_price_fill(token_cache: &TokenCache, mint: &str) -> Option<ResolvedFill> {
    token_cache.get(mint).and_then(|e| {
        let s = e.value();
        s.current_price
            .filter(|p| *p > 0.0)
            .map(|price| ResolvedFill {
                price,
                block_time: s.last_trade_at.unwrap_or_else(chrono::Utc::now),
            })
    })
}

fn cache_trades(token_cache: &TokenCache, mint: &str) -> Option<(Arc<Vec<CachedTrade>>, u64)> {
    token_cache.get(mint).map(|e| {
        let s = e.value();
        (Arc::clone(&s.trades), s.trades_base)
    })
}

fn abs_to_rel(abs: u64, base: u64, len: usize) -> Option<usize> {
    if abs < base {
        return None;
    }
    let rel = (abs - base) as usize;
    (rel < len).then_some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Utc};

    use trading_core::config::constants::MAX_FILL_WAIT_SLOTS;
    use trading_core::models::token::Token;
    use trading_core::models::trade::{Trade, TradeType};
    use trading_core::state::token_cache::TokenState;

    const MINT: &str = "MINT-paper-exit";

    fn token() -> Token {
        Token::new(
            MINT.into(),
            "creator".into(),
            "Paper Exit".into(),
            "PXT".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            serde_json::Value::Array(vec![]),
            "create-sig".into(),
            None,
            Utc::now() - ChronoDuration::minutes(10),
        )
    }

    fn trade(sol: f64, tokens: u64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            MINT.into(),
            "w".into(),
            TradeType::Buy,
            sol,
            tokens,
            format!("sig-{slot}-{secs}"),
            slot,
            Utc::now() - ChronoDuration::minutes(10) + ChronoDuration::seconds(secs),
        )
    }

    fn cache_with(trades: Vec<Trade>) -> Arc<TokenCache> {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token());
        for t in trades {
            state.add_trade(t);
        }
        cache.insert(MINT.into(), state);
        cache
    }

    /// The regression this module exists for: a `Dead` exit fires *because* the
    /// token stopped printing, so the fill window is empty by construction. It must
    /// market-fill at the fire trade (what replay/simulate books), NOT return `None`
    /// — `None` walked the engine's retries into a permanent `ExitStuck`.
    #[tokio::test]
    async fn exit_market_fills_when_window_has_no_trade() {
        // Fire at slot 100; the only later print is past MAX_FILL_WAIT_SLOTS, so it
        // closes the window without being IN it — an empty window, resolved now.
        let cache = cache_with(vec![
            trade(1.0, 1_000_000, 100, 0),
            trade(0.4, 1_000_000, 100 + MAX_FILL_WAIT_SLOTS + 1, 5),
        ]);
        let fill = wait_exit_fill(&cache, MINT, Some(0)).await.expect("market fill at fire");
        assert!((fill.price - 1e-6).abs() < 1e-12, "fills at the fire trade's own spot");
    }

    /// Worst-case adversity is unchanged by the market-fill fallback: when the
    /// window *does* have prints, the exit still takes the lowest of them.
    #[tokio::test]
    async fn exit_still_takes_the_worst_price_in_a_populated_window() {
        let cache = cache_with(vec![
            trade(1.0, 1_000_000, 100, 0),   // fire  @ 1e-6
            trade(0.5, 1_000_000, 101, 1),   // in window, lowest
            trade(0.9, 1_000_000, 101, 1),   // in window
            trade(0.1, 1_000_000, 100 + MAX_FILL_WAIT_SLOTS + 1, 5), // closes it, out of window
        ]);
        let fill = wait_exit_fill(&cache, MINT, Some(0)).await.expect("windowed fill");
        assert!((fill.price - 0.5e-6).abs() < 1e-12, "lowest price in the window");
    }

    /// A manual close on a mint whose trades are gone (trimmed / never cached) has
    /// no window to price at all — it closes at the token's last known spot instead
    /// of stranding the row.
    #[tokio::test]
    async fn exit_falls_back_to_last_known_price_without_trades() {
        let cache = Arc::new(TokenCache::new());
        let mut state = TokenState::new(token());
        state.current_price = Some(2e-6);
        state.last_trade_at = Some(Utc::now() - ChronoDuration::minutes(3));
        cache.insert(MINT.into(), state);

        let fill = wait_exit_fill(&cache, MINT, None).await.expect("last known price");
        assert!((fill.price - 2e-6).abs() < 1e-12);
    }

    /// The one honest failure left: a mint the cache has never priced.
    #[tokio::test]
    async fn exit_fails_when_no_price_exists_anywhere() {
        let cache = Arc::new(TokenCache::new());
        assert!(wait_exit_fill(&cache, MINT, None).await.is_none());
    }
}

/// Absolute index of the mint's newest cached trade, if any — the default
/// trigger/fire when the decision loop dispatches a paper submit.
///
/// `then`, not `then_some`: the latter evaluates its argument eagerly, so a cached
/// token with **no** trades computed `base + 0 - 1` and underflowed — a debug
/// panic on the decision-loop thread, and in release a wrapped `u64::MAX` index
/// that no window could ever resolve. A tracked mint sits trade-less between its
/// `TokenCreated` and its first print, and again after a front-trim.
pub fn latest_trade_abs_idx(token_cache: &TokenCache, mint: &str) -> Option<u64> {
    token_cache.get(mint).and_then(|e| {
        let s = e.value();
        (!s.trades.is_empty()).then(|| s.trades_base + s.trades.len() as u64 - 1)
    })
}
