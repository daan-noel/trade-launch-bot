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
//! Passing `false` on the exit leg ("don't invent a sell print") strands the
//! position instead: a `Dead` exit fires *because* the token stopped trading, so
//! its window is empty by construction, each of the engine's `MAX_EXIT_ATTEMPTS`
//! retries re-fires against that same last trade, and the row lands in `ExitStuck`
//! — which no reaper path can reach (they are all `mode = 'real'`). The stranded
//! rows are exactly the dead-token losers, so the bias is upward on paper PnL.
//! Paper owns no on-chain bag, so a strict window protects nothing here; when the
//! window cannot price the exit at all the position closes at the token's last
//! known spot ([`last_known_price_fill`]) rather than never closing.
//!
//! A paper fill has no transaction of its own, but it is priced against one real
//! print. The executor stashes that print's [`PrintKey`] under the intent
//! ([`FillSigStore`]) and the sink resolves its signature from `trades`, so the row
//! names the print the fill copied: the same meaning simulate's `entry_tx`/`exit_tx`
//! carry, and what the chart and trades table key on. The last-known-spot fallback
//! prices off no print, so it stashes nothing.
//!
//! **The money is the kernel's, not a price ratio.** `Fill::price` stays the
//! print's spot — the engine's decision basis, SOL per RAW token unit — while
//! `Fill::sol` and `Fill::token_amount` come from `kernel::buy_fill` /
//! `sell_proceeds` against the print's own depth: the entry books what the order
//! would take from the wallet (fee, impact, fixed leg) and the tokens it would
//! receive, the exit what the sell would return. So a paper row's
//! `(exit − entry) / entry` is the same all-in number a real row books from its
//! wallet, and the same one simulate reports. Token counts are raw units with
//! **no** decimal scaling — the raw-unit convention `entry_price`/`exit_price` and
//! the real executor use.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use trading_core::state::token_cache::{CachedTrade, TokenCache};
use trading_core::strategies::kernel::{buy_fill, sell_proceeds, CostModel};
use trading_core::strategies::paper_fill::{
    exit_fill_window_closed, find_worst_case_paper_entry_at, find_worst_case_paper_exit_at,
    PaperFill,
};

use super::{FillSigStore, FillSigs, PositionId, PositionRegistry, PrintKey, TargetSnapshot};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// How long to wait for the fill window to index before giving up.
const FILL_WAIT: Duration = Duration::from_secs(2);
const FILL_POLL: Duration = Duration::from_millis(100);

/// Fill a paper buy at the worst-case adverse price after `trigger_abs_idx`.
///
/// `trigger_abs_idx` is the absolute cache trade index of the deciding trade
/// (`trades_base + offset`). `None` means no trade yet (enter-on-arm) — wait for
/// the first print and use that as the trigger. When a trigger resolves, its
/// snapshot is written to [`PositionMeta::target_snapshot`] so the sink can persist
/// `target_*` alongside the worst-case entry fill. Empty window market-fills at
/// the trigger (`market_fill_on_empty_window = true`) so live paper takes the
/// same position set as lab replay/sweep.
#[allow(clippy::too_many_arguments)]
pub async fn run_entry(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    registry: PositionRegistry,
    fill_sigs: FillSigStore,
    position: Option<PositionId>,
    intent: IntentId,
    mint: String,
    lamports: u64,
    trigger_abs_idx: Option<u64>,
) {
    let event = match wait_entry_fill(&token_cache, &mint, trigger_abs_idx).await {
        Some((fill, trigger)) => {
            if let Some(pid) = position {
                registry.update(pid, |m| m.target_snapshot = Some(trigger));
            }
            stash_print(&fill_sigs, &intent, fill.print);
            let notional = lamports as f64 / LAMPORTS_PER_SOL;
            // `fill.price` is SOL per RAW unit ⇒ the token count is already raw units.
            let (tokens, paid) =
                buy_fill(notional, fill.price, fill.reserve_sol, &fill.costs());
            Event::FillConfirmed {
                intent,
                fill: Fill {
                    price: fill.price,
                    sol: paid,
                    token_amount: tokens.round().max(0.0) as u64,
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
/// (`Portion::token_amount`) — full remainder or a scale-out leg; `empties_bag`
/// says it is the remainder, whose sell also pays the rent-reclaim close.
#[allow(clippy::too_many_arguments)]
pub async fn run_exit(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    fill_sigs: FillSigStore,
    intent: IntentId,
    mint: String,
    token_amount: u64,
    empties_bag: bool,
    fire_abs_idx: Option<u64>,
) {
    // Spawned on the decision: a clock exit (`held`/`stall`/`time`) fires on a tick
    // long after the print it fills at, and a real sell always lands after its
    // decision, so the exit is stamped no earlier than now (simulate's replay does
    // the same with its logical clock).
    let decided_at = Utc::now();
    let event = match wait_exit_fill(&token_cache, &mint, fire_abs_idx).await {
        Some(fill) => {
            stash_print(&fill_sigs, &intent, fill.print);
            let sol = sell_proceeds(
                token_amount as f64,
                fill.price,
                fill.reserve_sol,
                &fill.costs(),
                empties_bag,
            );
            Event::FillConfirmed {
                intent,
                fill: Fill {
                    price: fill.price,
                    sol,
                    token_amount,
                    at: fill.block_time.max(decided_at),
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
    /// Priced SOL depth of the pool state `price` is the spot of — what our own
    /// impact is charged against.
    reserve_sol: Option<f64>,
    block_time: chrono::DateTime<chrono::Utc>,
    /// The print that priced this fill; `None` for the last-known-spot fallback.
    print: Option<PrintKey>,
    /// The pool's PumpSwap fee (`TokenState::current_venue_fee_bps`); `None` on the
    /// curve.
    venue_fee_bps: Option<f64>,
}

impl ResolvedFill {
    fn priced_by(trades: &[CachedTrade], f: &PaperFill, venue_fee_bps: Option<f64>) -> Self {
        Self {
            price: f.price,
            reserve_sol: f.reserve_sol,
            block_time: f.block_time,
            print: trades.get(f.trade_idx).map(PrintKey::of),
            venue_fee_bps,
        }
    }

    /// The cost model this fill trades under: the curve model, at the pool's own
    /// fee once the token trades on PumpSwap.
    fn costs(&self) -> CostModel {
        CostModel::pumpfun_with_impact().at_venue_fee(self.venue_fee_bps)
    }
}

/// Hand the sink the print a paper fill copied, before the `FillConfirmed` that
/// makes it look (the sink takes it while folding that event's delta).
fn stash_print(fill_sigs: &FillSigStore, intent: &IntentId, print: Option<PrintKey>) {
    if print.is_some() {
        fill_sigs.put(intent.clone(), FillSigs { print, ..FillSigs::default() });
    }
}

fn target_snapshot_from(t: &CachedTrade) -> TargetSnapshot {
    TargetSnapshot {
        // The trigger's spot - the series the paper entry fills on, so the gap
        // between them is the fill model's adverse move alone.
        price: trading_core::models::trade::TradeRow::fill_basis(t),
        // `CachedTrade::token_amount` is already raw SPL units (same as entry fill).
        token_amount: t.token_amount.round().max(0.0) as u64,
        print: PrintKey::of(t),
    }
}

async fn wait_entry_fill(
    token_cache: &Arc<TokenCache>,
    mint: &str,
    mut trigger_abs: Option<u64>,
) -> Option<(ResolvedFill, TargetSnapshot)> {
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
                        let trigger = target_snapshot_from(&trades[rel]);
                        return find_worst_case_paper_entry_at(trades.as_slice(), rel, true)
                            .map(|f| (ResolvedFill::priced_by(&trades, &f, pool_fee_bps(token_cache, mint)), trigger));
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
                    let trigger = target_snapshot_from(&trades[rel]);
                    return find_worst_case_paper_entry_at(trades.as_slice(), rel, true)
                        .map(|f| (ResolvedFill::priced_by(&trades, &f, pool_fee_bps(token_cache, mint)), trigger));
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
                            .map(|f| ResolvedFill::priced_by(&trades, &f, pool_fee_bps(token_cache, mint)))
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
                reserve_sol: s.current_reserve_sol.filter(|r| r.is_finite() && *r > 0.0),
                block_time: s.last_trade_at.unwrap_or_else(chrono::Utc::now),
                print: None,
                venue_fee_bps: s.current_venue_fee_bps,
            })
    })
}

fn pool_fee_bps(token_cache: &TokenCache, mint: &str) -> Option<f64> {
    token_cache.get(mint).and_then(|e| e.value().current_venue_fee_bps)
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

    /// Place a print at its own intra-slot position, so two prints in one slot have
    /// distinct `PrintKey`s the way real ones do.
    fn at_tx(mut t: Trade, tx_index: u32) -> Trade {
        t.tx_index = tx_index;
        t
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
        assert_eq!(fill.print.map(|p| p.slot), Some(100), "and names the fire print");
    }

    fn populated_window() -> Arc<TokenCache> {
        cache_with(vec![
            trade(1.0, 1_000_000, 100, 0),                 // fire  @ 1e-6
            at_tx(trade(0.5, 1_000_000, 101, 1), 1),       // in window, lowest
            at_tx(trade(0.9, 1_000_000, 101, 1), 2),       // in window
            trade(0.1, 1_000_000, 100 + MAX_FILL_WAIT_SLOTS + 1, 5), // closes it, out of window
        ])
    }

    /// Worst-case adversity is unchanged by the market-fill fallback: when the
    /// window *does* have prints, the exit still takes the lowest of them, and the
    /// fill names THAT print, not the fire print, so the trades table tints the row
    /// the price came from.
    #[tokio::test]
    async fn exit_still_takes_the_worst_price_in_a_populated_window() {
        let cache = populated_window();
        let fill = wait_exit_fill(&cache, MINT, Some(0)).await.expect("windowed fill");
        assert!((fill.price - 0.5e-6).abs() < 1e-12, "lowest price in the window");
        assert_eq!(fill.print.map(|p| (p.slot, p.tx_index)), Some((101, 1)));
    }

    /// The executor hands the sink the print under the fill's intent, before the
    /// `FillConfirmed`, and claims no signature or slot of its own, so the latency
    /// columns stay empty for a simulated fill.
    #[tokio::test]
    async fn exit_fill_stashes_the_print_it_copied() {
        use hunter_engine::event::{Mint, RuleId};
        let store = FillSigStore::new();
        let (tx, mut rx) = mpsc::channel(1);
        let intent = IntentId { rule: RuleId(uuid::Uuid::nil()), mint: Mint::from(MINT), seq: 7 };
        run_exit(tx, populated_window(), store.clone(), intent.clone(), MINT.into(), 1_000, true, Some(0))
            .await;
        let Some(Event::FillConfirmed { fill, .. }) = rx.recv().await else {
            panic!("expected a FillConfirmed");
        };
        // The money is the kernel's sell — fee, fixed leg and close — at the print's
        // spot, never a bare `tokens × price`.
        let want = sell_proceeds(1_000.0, 0.5e-6, None, &CostModel::pumpfun_with_impact(), true);
        assert!((fill.sol - want).abs() < 1e-15, "{} vs {want}", fill.sol);
        assert!(fill.sol < 1_000.0 * 0.5e-6);
        let fs = store.take(&intent).expect("print stashed under the intent");
        assert!(fs.sigs.is_empty() && fs.slot.is_none() && fs.token_account.is_none());
        assert_eq!(fs.print.map(|p| (p.slot, p.tx_index)), Some((101, 1)));
    }

    /// A migrated token's paper sell pays its PumpSwap pool's own fee (95 bps here),
    /// not the curve's 125.
    #[tokio::test]
    async fn exit_on_a_pumpswap_pool_pays_the_pool_fee() {
        let cache = populated_window();
        cache.get_mut(MINT).unwrap().current_venue_fee_bps = Some(95.0);
        let (tx, mut rx) = mpsc::channel(1);
        let intent = IntentId {
            rule: hunter_engine::event::RuleId(uuid::Uuid::nil()),
            mint: hunter_engine::event::Mint::from(MINT),
            seq: 8,
        };
        run_exit(tx, cache, FillSigStore::new(), intent, MINT.into(), 1_000, true, Some(0)).await;
        let Some(Event::FillConfirmed { fill, .. }) = rx.recv().await else {
            panic!("expected a FillConfirmed");
        };
        let amm = CostModel { fee_bps_per_leg: 95.0, ..CostModel::pumpfun_with_impact() };
        let want = sell_proceeds(1_000.0, 0.5e-6, None, &amm, true);
        assert!((fill.sol - want).abs() < 1e-15, "{} vs {want}", fill.sol);
        let curve = sell_proceeds(1_000.0, 0.5e-6, None, &CostModel::pumpfun_with_impact(), true);
        assert!(fill.sol > curve, "a cheaper pool fee returns more than the curve fee");
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
        assert!(fill.print.is_none(), "no print priced it, so none is named");
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

/// The trigger snapshot for a **real** entry — the most recent cached print at
/// dispatch time, which is the trade the fold just decided on.
///
/// Paper resolves its trigger inside the fill loop (it has to: the worst-case
/// fill is chosen relative to it). Real submits immediately and learns its fill
/// slot only on confirm, so the trigger has to be captured here or it is lost.
/// Without it a real position stores `entry_slot` with no `target_slot` to
/// subtract, and the row cannot answer what it cost to be late (mig 0004).
pub fn latest_trade_target(token_cache: &TokenCache, mint: &str) -> Option<TargetSnapshot> {
    let (trades, _) = cache_trades(token_cache, mint)?;
    trades.last().map(target_snapshot_from)
}
