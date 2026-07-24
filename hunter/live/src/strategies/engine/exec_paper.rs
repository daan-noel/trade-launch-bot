//! Paper execution — a transaction-free fill model that turns a
//! `SubmitBuy`/`SubmitSell` into a `FillConfirmed` the engine folds exactly like a
//! real fill.
//!
//! Fills use the **worst-case** adverse window shared with lab simulate/sweep
//! ([`trading_core::strategies::paper_fill`]): after the trigger/fire trade, the
//! fill is the highest buy (entry) or lowest price (exit) in trigger slot `S` +
//! the next observed slot within [`MAX_FILL_WAIT_SLOTS`]. The executor polls the
//! token-cache trade feed until that window is indexed (or a short deadline).
//! Entry uses `market_fill_on_empty_window = true` (same taken-position set as
//! analysis); exit keeps `false` and times out rather than inventing a sell print.
//!
//! There is no on-chain identity for a paper fill, so the executor stashes no
//! signatures — the sink's `record_entry_fill`/`close` just see an empty sig list.
//!
//! PnL is purely price-ratio based (exit_sol = entry_sol · exit_price/entry_price),
//! so the (cosmetic) raw `token_amount` uses pump.fun's 6-decimal scaling.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use trading_core::state::token_cache::{CachedTrade, TokenCache};
use trading_core::strategies::paper_fill::{
    exit_fill_window_closed, find_worst_case_paper_entry_at, find_worst_case_paper_exit_at,
};

use super::{PaperTarget, PositionId, PositionRegistry};

/// pump.fun SPL token decimals (raw units per whole token). Paper `token_amount`
/// is cosmetic — PnL is price-ratio based — so this only needs to be consistent.
const TOKEN_SCALE: f64 = 1_000_000.0;
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
            let token_amount = ((sol / fill.price) * TOKEN_SCALE).round().max(0.0) as u64;
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
        None => Event::FillFailed { intent, reason: FillFailReason::Timeout },
    };
    let _ = fill_tx.send(event).await;
}

/// Fill a paper sell at the worst-case adverse price after `fire_abs_idx`.
/// Live paper keeps the strict window (`market_fill_on_empty_window = false`): an
/// empty window times out as `FillFailed` rather than inventing a fill.
pub async fn run_exit(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    intent: IntentId,
    mint: String,
    entry_token_amount: u64,
    fire_abs_idx: Option<u64>,
) {
    let event = match wait_exit_fill(&token_cache, &mint, fire_abs_idx).await {
        Some(fill) => {
            let sol = (entry_token_amount as f64 / TOKEN_SCALE) * fill.price;
            Event::FillConfirmed {
                intent,
                fill: Fill {
                    price: fill.price,
                    sol,
                    token_amount: entry_token_amount,
                    at: fill.block_time,
                },
            }
        }
        None => Event::FillFailed { intent, reason: FillFailReason::Timeout },
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
    let fire_abs = fire_abs.or_else(|| {
        cache_trades(token_cache, mint).and_then(|(trades, base)| {
            (!trades.is_empty()).then_some(base + trades.len() as u64 - 1)
        })
    })?;
    let deadline = tokio::time::Instant::now() + FILL_WAIT;
    loop {
        if let Some((trades, base)) = cache_trades(token_cache, mint) {
            if let Some(rel) = abs_to_rel(fire_abs, base, trades.len()) {
                let fire_slot = trades[rel].slot;
                let max_slot = trades.last().map(|t| t.slot).unwrap_or(fire_slot);
                let timed_out = tokio::time::Instant::now() >= deadline;
                if exit_fill_window_closed(fire_slot, max_slot) || timed_out {
                    // Live: no market-fill on empty window.
                    return find_worst_case_paper_exit_at(trades.as_slice(), rel, false).map(|f| {
                        ResolvedFill { price: f.price, block_time: f.block_time }
                    });
                }
            } else if fire_abs < base {
                return None;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            if let Some((trades, base)) = cache_trades(token_cache, mint) {
                if let Some(rel) = abs_to_rel(fire_abs, base, trades.len()) {
                    return find_worst_case_paper_exit_at(trades.as_slice(), rel, false).map(|f| {
                        ResolvedFill { price: f.price, block_time: f.block_time }
                    });
                }
            }
            return None;
        }
        tokio::time::sleep(FILL_POLL).await;
    }
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

/// Absolute index of the mint's newest cached trade, if any — the default
/// trigger/fire when the decision loop dispatches a paper submit.
pub fn latest_trade_abs_idx(token_cache: &TokenCache, mint: &str) -> Option<u64> {
    token_cache.get(mint).and_then(|e| {
        let s = e.value();
        (!s.trades.is_empty()).then_some(s.trades_base + s.trades.len() as u64 - 1)
    })
}
