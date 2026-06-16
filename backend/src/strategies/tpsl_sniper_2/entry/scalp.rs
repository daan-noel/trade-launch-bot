//! Scalp-continuation entry gates (see `@project_plans/@TPSL_plan/
//! tpsl-scalp-continuation-plan.md`). Where the legacy entry decides at token
//! creation from `Token` fields, the scalp gates decide **on the trade stream**:
//! "buy on the first trade where ALL configured gates hold." Each gate is a pure
//! function of a token's chronological trade slice, so the backtest and (later) a
//! live trade-watcher resolve identical entries.
//!
//! The gates, all inert at `None`/`0`:
//!   • `p_entry_min_age_secs`      — skip the launch spike (entry age ≥ N s after t0).
//!   • `p_entry_min_alive_sol`     — still trading (total SOL in the trailing window).
//!   • `p_entry_min_organic_sol`   — new people buying (net SOL by outside wallets).
//!   • `p_entry_pullback_pct` (+ `p_entry_higher_low_secs`) — higher-low continuation shape.
//!   • `p_entry_max_cohort_held`   — launch cohort already sold its bag.
//!   • `p_entry_min_liquidity_sol` — real reserves ≥ N SOL.
//!   • `p_entry_min_organic_liq`   — real reserves from buyers, not one dev deposit.
//!
//! Cohort / outside split reuses [`super::super::cohort`] so it matches rug
//! detection and the E5 exit exactly.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use super::super::cohort::{cohort_flow, early_cohort_wallets, outside_net_sol};
use super::super::util::{none_if_zero_f64, none_if_zero_u64};
use super::EntryFill;
use crate::config::constants::EARLY_COHORT_SLOT_WINDOW;
use crate::models::trade::{Trade, TradeRow};
use crate::models::Tpsl2Rule;

/// Liveness window for `p_entry_min_alive_sol`: total SOL traded in the trailing
/// `ALIVE_WINDOW_SECS` ending at the candidate trade. Kept a module constant (not
/// a per-rule param) so the "still trading" gate has one well-defined window;
/// sized to the launch-spike horizon. Revisit here if it needs tuning.
const ALIVE_WINDOW_SECS: i64 = 10;

/// Whether a rule configures **any** scalp entry gate. tpsl2 has no other entry
/// path (the legacy first-slot fill was removed), so when this is false the rule
/// can never resolve an entry — the backtest rejects it up front and the live
/// paper poll never fills.
pub fn rule_configures_any_scalp_gate(rule: &Tpsl2Rule) -> bool {
    none_if_zero_u64(rule.p_entry_min_age_secs).is_some()
        || none_if_zero_f64(rule.p_entry_min_alive_sol).is_some()
        || none_if_zero_f64(rule.p_entry_min_organic_sol).is_some()
        || none_if_zero_f64(rule.p_entry_pullback_pct).is_some()
        || none_if_zero_f64(rule.p_entry_max_cohort_held).is_some()
        || none_if_zero_f64(rule.p_entry_min_liquidity_sol).is_some()
        || none_if_zero_f64(rule.p_entry_min_organic_liq).is_some()
}

/// Trade-window features computed over a **causal prefix** (`trades[0..=i]`, the
/// candidate being `prefix.last()`). Only data available up to the candidate is
/// used, so a backtest entry is decidable live too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalpFeatures {
    /// Seconds from the token's first trade to the candidate.
    pub age_secs: i64,
    /// Total SOL (buys + sells, any wallet) in the trailing `ALIVE_WINDOW_SECS`.
    pub alive_sol: f64,
    /// Net SOL bought by outside (non-cohort) wallets so far.
    pub organic_sol: f64,
    /// Launch cohort's held ratio (net / bought); `1` holding, `→0` sold out.
    pub cohort_held_ratio: f64,
    /// Latest known real SOL reserves at the candidate.
    pub real_liquidity_sol: f64,
    /// Real reserves attributable to outside buyers (`real − cohort net SOL`).
    pub organic_liq_sol: f64,
}

/// Compute the trade-window features for a causal prefix. `prefix` must be
/// non-empty and chronologically sorted; the candidate is the last element.
///
/// Retained as the per-prefix feature oracle that pins the linearized
/// `find_scalp_entry` (see `linearized_scalp_entry_matches_prefix_oracle`); the
/// live path no longer recomputes features per candidate (was O(n²)).
#[cfg_attr(not(test), allow(dead_code))]
pub fn scalp_features(prefix: &[Trade]) -> Option<ScalpFeatures> {
    let first = prefix.first()?;
    let cand = prefix.last()?;

    let age_secs = (cand.block_time - first.block_time).num_seconds();

    let alive_cutoff = cand.block_time - chrono::Duration::seconds(ALIVE_WINDOW_SECS);
    let alive_sol: f64 = prefix
        .iter()
        .filter(|t| t.block_time >= alive_cutoff)
        .map(|t| t.sol_amount)
        .sum();

    let cohort = early_cohort_wallets(prefix, EARLY_COHORT_SLOT_WINDOW);
    let flow = cohort_flow(prefix, &cohort);
    let organic_sol = outside_net_sol(prefix, &cohort);

    // Latest known real reserves: scan back for the most recent trade carrying a
    // real_sol_reserves snapshot (the plan mandates REAL, never virtual).
    let real_liquidity_sol = prefix
        .iter()
        .rev()
        .find_map(|t| t.real_sol_reserves)
        .unwrap_or(0.0);

    // Of the real SOL in the pool, the part NOT contributed by the cohort/dev.
    let organic_liq_sol = (real_liquidity_sol - flow.net_sol).max(0.0);

    Some(ScalpFeatures {
        age_secs,
        alive_sol,
        organic_sol,
        cohort_held_ratio: flow.held_ratio(),
        real_liquidity_sol,
        organic_liq_sol,
    })
}

/// Higher-low continuation gate. Walks the price series detecting **swing lows**
/// (a fall from a local high that then turns back up) and returns true once a
/// *higher low* has formed: a swing low that bottomed **above** the previous one,
/// with the two bottoms at least `min_span_secs` apart (filters sub-second
/// fakes).
///
/// Only the **first** swing low must qualify as a real pullback — fall
/// ≥ `pullback_pct` off the local high — which establishes that the chart is
/// genuinely swinging and filters tiny wiggles. Once that swing is established, a
/// subsequent swing low merely has to be *higher* than it to confirm (matching
/// the plan's example, where dip 2 at −12% off the bounce still counts). A
/// freefall that never turns back up never confirms.
///
/// Retained as the per-prefix oracle behind [`higher_low_confirmed_index`] (the
/// linearized one-pass form the live `find_scalp_entry` gates on).
#[cfg_attr(not(test), allow(dead_code))]
pub fn higher_low_confirmed(prefix: &[Trade], pullback_pct: f64, min_span_secs: i64) -> bool {
    if pullback_pct <= 0.0 {
        return false;
    }

    let series: Vec<(DateTime<Utc>, f64)> = prefix
        .iter()
        .map(|t| (t.block_time, t.price_per_token))
        .filter(|(_, p)| *p > 0.0)
        .collect();
    let Some(&(_, p0)) = series.first() else {
        return false;
    };

    let mut high = p0; // running high since the last completed swing low
    let mut cur_low: Option<(DateTime<Utc>, f64)> = None; // min of the active fall
    let mut prev_low: Option<(DateTime<Utc>, f64)> = None; // last *established* swing low
    let mut established = false; // has the first ≥pullback% dip qualified?

    for &(t, price) in series.iter().skip(1) {
        match cur_low {
            // Climbing: raise the high until price starts falling.
            None => {
                if price >= high {
                    high = price;
                } else {
                    cur_low = Some((t, price));
                }
            }
            Some((low_time, low)) => {
                if price < low {
                    cur_low = Some((t, price)); // the fall deepens
                } else if price > low {
                    // Turn-up → a swing low completed at (low_time, low).
                    let drop_pct = if high > 0.0 { (high - low) / high * 100.0 } else { 0.0 };
                    if !established {
                        // The first swing low must be a real pullback to count.
                        if drop_pct >= pullback_pct {
                            established = true;
                            prev_low = Some((low_time, low));
                        }
                    } else if let Some((prev_time, prev_bottom)) = prev_low {
                        if low > prev_bottom
                            && (low_time - prev_time).num_seconds() >= min_span_secs
                        {
                            return true; // higher low, formed over enough time
                        }
                        prev_low = Some((low_time, low));
                    }
                    // Start a fresh upswing from the recovery price.
                    high = price;
                    cur_low = None;
                }
            }
        }
    }
    false
}

/// The first `trades` index at which [`higher_low_confirmed`] would return true
/// for the prefix ending there, or `None` if it never confirms over the whole
/// slice. Runs the identical swing-detection state machine in one forward pass and
/// records the original trade index of the turn-up that completes the confirming
/// higher low — so `find_scalp_entry` can gate on `i >= idx` instead of re-walking
/// the prefix per candidate (the confirmation is monotonic in prefix length).
fn higher_low_confirmed_index<T: TradeRow>(
    trades: &[T],
    pullback_pct: f64,
    min_span_secs: i64,
) -> Option<usize> {
    if pullback_pct <= 0.0 {
        return None;
    }
    // (original index, time, price) for price-positive trades — mirrors the
    // `series` filter in `higher_low_confirmed`.
    let series: Vec<(usize, DateTime<Utc>, f64)> = trades
        .iter()
        .enumerate()
        .map(|(i, t)| (i, t.block_time(), t.price_per_token()))
        .filter(|(_, _, p)| *p > 0.0)
        .collect();
    let &(_, _, p0) = series.first()?;

    let mut high = p0;
    let mut cur_low: Option<(DateTime<Utc>, f64)> = None;
    let mut prev_low: Option<(DateTime<Utc>, f64)> = None;
    let mut established = false;

    for &(idx, t, price) in series.iter().skip(1) {
        match cur_low {
            None => {
                if price >= high {
                    high = price;
                } else {
                    cur_low = Some((t, price));
                }
            }
            Some((low_time, low)) => {
                if price < low {
                    cur_low = Some((t, price));
                } else if price > low {
                    let drop_pct = if high > 0.0 { (high - low) / high * 100.0 } else { 0.0 };
                    if !established {
                        if drop_pct >= pullback_pct {
                            established = true;
                            prev_low = Some((low_time, low));
                        }
                    } else if let Some((prev_time, prev_bottom)) = prev_low {
                        if low > prev_bottom && (low_time - prev_time).num_seconds() >= min_span_secs
                        {
                            // Confirms at this turn-up trade (original index `idx`).
                            return Some(idx);
                        }
                        prev_low = Some((low_time, low));
                    }
                    high = price;
                    cur_low = None;
                }
            }
        }
    }
    None
}

/// The launch cohort for a token's trade slice — the wallet set every scalp gate
/// splits cohort-vs-outside flow against. Depends **only** on the trades (the
/// first trade's slot + `EARLY_COHORT_SLOT_WINDOW`), never on a rule's params, so
/// the sweep computes it **once per token** (see `Strategy::prepare_token`) and
/// reuses it across every entry-param tuple instead of rebuilding the `HashSet`
/// per resolve. The live/backtest paths call [`find_scalp_entry`], which computes
/// it inline — same definition, so decisions stay byte-identical.
pub fn scalp_cohort<T: TradeRow>(trades: &[T]) -> HashSet<T::Wallet> {
    early_cohort_wallets(trades, EARLY_COHORT_SLOT_WINDOW)
}

/// Walk a token's trades and return the entry fill at the **first trade where
/// every configured scalp gate holds**, or `None` if none qualifies. The
/// candidate must be a buy (we enter by buying, filling at its price). Trades are
/// assumed chronologically sorted upstream.
///
/// Returns `None` when the rule configures no scalp gate; callers gate on
/// [`rule_configures_any_scalp_gate`] (the backtest rejects such a rule up front),
/// so this is never a silent buy-everything path.
///
/// Computes the launch cohort inline ([`scalp_cohort`]); the sweep hot path calls
/// [`find_scalp_entry_with_cohort`] with a per-token-cached cohort instead.
pub fn find_scalp_entry<T: TradeRow>(trades: &[T], rule: &Tpsl2Rule) -> Option<EntryFill> {
    if !rule_configures_any_scalp_gate(rule) || trades.is_empty() {
        return None;
    }
    let cohort = scalp_cohort(trades);
    find_scalp_entry_with_cohort(trades, rule, &cohort)
}

/// [`find_scalp_entry`] with the launch `cohort` supplied by the caller — the form
/// the sweep uses so the cohort `HashSet` is built once per token, not once per
/// entry-param tuple. `cohort` MUST equal [`scalp_cohort(trades)`](scalp_cohort)
/// for the slice; passing a mismatched set changes the cohort/outside split and so
/// the resolved entry. The live path goes through [`find_scalp_entry`], which
/// computes it from the same definition, keeping decisions identical.
pub fn find_scalp_entry_with_cohort<T: TradeRow>(
    trades: &[T],
    rule: &Tpsl2Rule,
    cohort: &HashSet<T::Wallet>,
) -> Option<EntryFill> {
    if !rule_configures_any_scalp_gate(rule) || trades.is_empty() {
        return None;
    }

    let min_age = none_if_zero_u64(rule.p_entry_min_age_secs).map(|v| v as i64);
    let min_alive = none_if_zero_f64(rule.p_entry_min_alive_sol);
    let min_organic = none_if_zero_f64(rule.p_entry_min_organic_sol);
    let pullback = none_if_zero_f64(rule.p_entry_pullback_pct);
    let higher_low_secs = none_if_zero_u64(rule.p_entry_higher_low_secs).map(|v| v as i64).unwrap_or(0);
    let max_cohort_held = none_if_zero_f64(rule.p_entry_max_cohort_held);
    let min_liq = none_if_zero_f64(rule.p_entry_min_liquidity_sol);
    let min_organic_liq = none_if_zero_f64(rule.p_entry_min_organic_liq);

    // O(n) preamble (M3): the launch cohort is fixed by the slice's first trade
    // (cutoff = first.slot + EARLY_COHORT_SLOT_WINDOW), a superset of every prefix
    // cohort — and identical to it for any real launch series, where a cohort
    // wallet's first trade IS its in-window buy (a wallet can't sell before it has
    // bought on the curve). Carrying the cohort/outside flows, the trailing
    // alive-window sum, and the last-seen real reserves forward as the walk advances
    // replaces the per-candidate prefix rescans (was O(n²)). The cohort itself is
    // supplied by the caller (hoisted once per token in the sweep; computed inline
    // by [`find_scalp_entry`] on the live path).

    // First index (if any) at which the higher-low shape is confirmed; the
    // confirmation is monotonic in prefix length, so one forward pass suffices.
    let higher_low_idx = pullback.and_then(|pb| higher_low_confirmed_index(trades, pb, higher_low_secs));

    let first_time = trades[0].block_time();
    // Running cohort/outside flow accumulators (mirror `cohort_flow` / `outside_net_sol`).
    let mut cohort_bought = 0.0f64;
    let mut cohort_net_tokens = 0.0f64;
    let mut cohort_net_sol = 0.0f64;
    let mut organic_sol = 0.0f64; // outside (non-cohort) net SOL
    let mut last_real_reserves = 0.0f64; // most recent real_sol_reserves snapshot
    // Trailing alive-window: a running sum + a front cursor over `trades`.
    let mut alive_sol = 0.0f64;
    let mut alive_lo = 0usize;

    for i in 0..trades.len() {
        let t = &trades[i];
        let block_time = t.block_time();
        let sol_amount = t.sol_amount();
        // Fold this trade into the running flow/liquidity accumulators FIRST so the
        // features below reflect the prefix `[0..=i]` (the candidate inclusive).
        let in_cohort = cohort.contains(t.wallet());
        if t.is_buy() {
            if in_cohort {
                cohort_bought += t.token_amount();
                cohort_net_tokens += t.token_amount();
                cohort_net_sol += sol_amount;
            } else {
                organic_sol += sol_amount;
            }
        } else if in_cohort {
            cohort_net_tokens -= t.token_amount();
            cohort_net_sol -= sol_amount;
        } else {
            organic_sol -= sol_amount;
        }
        if let Some(r) = t.real_sol_reserves() {
            last_real_reserves = r;
        }
        // Slide the alive window to [cand.block_time - ALIVE_WINDOW_SECS, cand].
        alive_sol += sol_amount;
        let alive_cutoff = block_time - chrono::Duration::seconds(ALIVE_WINDOW_SECS);
        while alive_lo <= i && trades[alive_lo].block_time() < alive_cutoff {
            alive_sol -= trades[alive_lo].sol_amount();
            alive_lo += 1;
        }

        if !t.is_buy() || t.price_per_token() <= 0.0 {
            continue;
        }

        let age_secs = (block_time - first_time).num_seconds();
        let cohort_held_ratio = if cohort_bought > 0.0 {
            cohort_net_tokens.max(0.0) / cohort_bought
        } else {
            0.0
        };
        let organic_liq_sol = (last_real_reserves - cohort_net_sol).max(0.0);

        if let Some(min) = min_age {
            if age_secs < min {
                continue;
            }
        }
        if let Some(min) = min_alive {
            if alive_sol < min {
                continue;
            }
        }
        if let Some(min) = min_organic {
            if organic_sol < min {
                continue;
            }
        }
        if let Some(max) = max_cohort_held {
            // `max` is whole-percent (0–100); held_ratio is 0–1. Divide to compare,
            // matching the E1/E4/E5 percent convention.
            if cohort_held_ratio > max / 100.0 {
                continue;
            }
        }
        if let Some(min) = min_liq {
            if last_real_reserves < min {
                continue;
            }
        }
        if let Some(min) = min_organic_liq {
            if organic_liq_sol < min {
                continue;
            }
        }
        if pullback.is_some() {
            match higher_low_idx {
                Some(idx) if i >= idx => {}
                _ => continue,
            }
        }

        return Some(EntryFill {
            price: t.price_per_token(),
            amount_sol: sol_amount,
            tx_signature: t.tx_signature().to_string(),
            block_time,
        });
    }
    None
}

/// Paper worst-case entry. Given the mint's chronological `trades` and the
/// trigger trade identified by `target_tx` (the scalp-entry signal), choose the
/// adverse entry fill that the paper position is recorded at — distinct from the
/// trigger so the paper target/entry gap is real:
///   • candidate pool = trades in the trigger's slot S or S+1, strictly after the
///     trigger by `(slot, leg_index)`, excluding the trigger tx, ANY trade type;
///   • drop dust ([`Trade::is_dust`]) and `price_per_token <= 0`;
///   • pick the highest `price_per_token` (worst case); tie → latest by
///     `(slot, leg_index)`;
///   • empty pool → fall back to the trigger itself (entry == target).
///
/// `amount_sol` of the returned entry is always the TRIGGER trade's `sol_amount`
/// (the size we would have filled), not the adverse trade's. If `target_tx` is
/// absent from `trades` (shouldn't happen — it was just resolved from this slice)
/// the trigger can't be located, so the first usable trade is returned as a safe
/// degenerate fallback.
///
/// Caveat: `leg_index` orders legs *within* a tx, so `(slot, leg_index)` does not
/// fully order two distinct txs in the same slot; ties are resolved by the
/// highest-price / latest rule as specified.
pub fn find_worst_case_paper_entry<T: TradeRow>(trades: &[T], target_tx: &str) -> EntryFill {
    let entry_from = |t: &T, amount_sol: f64| EntryFill {
        price: t.price_per_token(),
        amount_sol,
        tx_signature: t.tx_signature().to_string(),
        block_time: t.block_time(),
    };

    let Some(target) = trades.iter().find(|t| t.tx_signature() == target_tx) else {
        // The trigger isn't in the slice — degrade to the first usable trade, or
        // (no trades at all) a zero-priced empty fill keyed to the missing tx.
        return trades
            .iter()
            .find(|t| t.price_per_token() > 0.0 && !Trade::is_dust(t.sol_amount()))
            .map(|t| entry_from(t, t.sol_amount()))
            .unwrap_or(EntryFill {
                price: 0.0,
                amount_sol: 0.0,
                tx_signature: target_tx.to_string(),
                block_time: trades.first().map(|t| t.block_time()).unwrap_or_else(Utc::now),
            });
    };
    let trigger_sol = target.sol_amount();
    let target_slot = target.slot();
    let target_key = (target_slot, target.leg_index());

    // Adverse candidates: same slot or the next, strictly after the trigger,
    // non-dust, priced. The worst case is the highest price; ties break to the
    // latest by (slot, leg_index).
    let worst = trades
        .iter()
        .filter(|t| t.slot() == target_slot || t.slot() == target_slot + 1)
        .filter(|t| (t.slot(), t.leg_index()) > target_key && t.tx_signature() != target_tx)
        .filter(|t| t.price_per_token() > 0.0 && !Trade::is_dust(t.sol_amount()))
        .max_by(|a, b| {
            a.price_per_token()
                .total_cmp(&b.price_per_token())
                .then_with(|| (a.slot(), a.leg_index()).cmp(&(b.slot(), b.leg_index())))
        });

    // Worst-case adverse trade if any, else fall back to the trigger itself.
    // Either way `amount_sol` is the trigger trade's SOL.
    worst
        .map(|t| entry_from(t, trigger_sol))
        .unwrap_or_else(|| entry_from(target, trigger_sol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::TradeType;

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A trade with explicit side/sol/tokens/slot/seconds-after-base.
    fn t(wallet: &str, side: TradeType, sol: f64, tokens: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            wallet.into(),
            side,
            sol,
            tokens,
            format!("sig-{wallet}-{slot}-{secs}"),
            slot,
            base_time() + chrono::Duration::seconds(secs),
        )
    }

    fn buy(wallet: &str, sol: f64, tokens: f64, slot: u64, secs: i64) -> Trade {
        t(wallet, TradeType::Buy, sol, tokens, slot, secs)
    }

    /// A buy whose price_per_token = `price` (token_amount = 1), at `slot`/`secs`.
    fn pbuy(price: f64, slot: u64, secs: i64) -> Trade {
        buy("w", price, 1.0, slot, secs)
    }

    /// An inert rule (all scalp gates off) we then set fields on.
    fn rule() -> Tpsl2Rule {
        Tpsl2Rule::new(
            "scalp".into(),
            None, None, None,
            serde_json::Value::Array(vec![]),
            "paper".into(),
            0.05, 20.0, 25.0,
            None, None, None, None,
            Some(0.0),
            None, None, None, None,
        )
    }

    #[test]
    fn no_scalp_gate_means_no_scalp_entry() {
        let trades = vec![pbuy(1.0, 1, 0), pbuy(1.1, 2, 5)];
        assert!(find_scalp_entry(&trades, &rule()).is_none());
    }

    #[test]
    fn age_gate_waits_for_min_age() {
        let trades = vec![pbuy(1.0, 1, 0), pbuy(1.0, 2, 4), pbuy(1.0, 3, 12)];
        let mut r = rule();
        r.p_entry_min_age_secs = Some(10);
        let fill = find_scalp_entry(&trades, &r).expect("should enter once old enough");
        // First candidate with age >= 10s is the trade at +12s.
        assert_eq!(fill.block_time, base_time() + chrono::Duration::seconds(12));
    }

    #[test]
    fn alive_gate_requires_recent_volume() {
        let mut r = rule();
        r.p_entry_min_alive_sol = Some(2.0);
        // A lone 1 SOL buy: alive window total is 1.0 < 2.0 → no entry.
        let quiet = vec![pbuy(1.0, 1, 0)];
        assert!(find_scalp_entry(&quiet, &r).is_none());
        // Three 1 SOL trades within the 10s window → 3.0 >= 2.0 at the last.
        let busy = vec![pbuy(1.0, 1, 0), pbuy(1.0, 2, 2), pbuy(1.0, 3, 4)];
        assert!(find_scalp_entry(&busy, &r).is_some());
    }

    #[test]
    fn organic_gate_ignores_cohort_buys() {
        let mut r = rule();
        r.p_entry_min_organic_sol = Some(1.0);
        // Only cohort (early-slot) buys → organic stays 0 → never enters.
        let cohort_only = vec![buy("dev", 5.0, 100.0, 1, 0), buy("dev2", 5.0, 100.0, 2, 1)];
        assert!(find_scalp_entry(&cohort_only, &r).is_none());
        // A later outside wallet buys 2 SOL → organic 2.0 >= 1.0.
        let mut with_outside = cohort_only.clone();
        with_outside.push(buy("outsider", 2.0, 20.0, 500, 5));
        let fill = find_scalp_entry(&with_outside, &r).expect("outside demand qualifies");
        assert_eq!(fill.tx_signature, "sig-outsider-500-5");
    }

    #[test]
    fn cohort_held_gate_requires_cohort_sold_down() {
        let mut r = rule();
        r.p_entry_max_cohort_held = Some(30.0); // percent: ≤ 30% held
        // Cohort holds its full bag (ratio 1.0) → blocked on the outside buy.
        let holding = vec![buy("dev", 5.0, 100.0, 1, 0), buy("out", 1.0, 5.0, 500, 5)];
        assert!(find_scalp_entry(&holding, &r).is_none());
        // Cohort dumps 90% (ratio 0.1 ≤ 0.30), THEN an outside wallet buys — we
        // enter on that later buy, where the overhang is already gone.
        let dumped = vec![
            buy("dev", 5.0, 100.0, 1, 0),
            t("dev", TradeType::Sell, 4.0, 90.0, 600, 6),
            buy("out", 1.0, 5.0, 700, 7),
        ];
        let fill = find_scalp_entry(&dumped, &r).expect("enters once cohort has sold down");
        assert_eq!(fill.tx_signature, "sig-out-700-7");
    }

    #[test]
    fn liquidity_gate_uses_real_reserves() {
        let mut r = rule();
        r.p_entry_min_liquidity_sol = Some(10.0);
        let mut low = pbuy(1.0, 1, 0);
        low.real_sol_reserves = Some(3.0);
        let mut high = buy("out", 1.0, 1.0, 2, 2);
        high.real_sol_reserves = Some(15.0);
        // Only the second trade clears 10 SOL real reserves.
        let fill = find_scalp_entry(&vec![low, high], &r).expect("enters once liquid");
        assert_eq!(fill.block_time, base_time() + chrono::Duration::seconds(2));
    }

    // ── M3: linearized find_scalp_entry matches the per-prefix oracle ─────────

    /// Brute-force reference: the pre-M3 O(n²) behavior — for each candidate buy,
    /// recompute features over the prefix via `scalp_features` and check every
    /// gate. The linearized `find_scalp_entry` must agree on every input.
    fn scalp_entry_oracle(trades: &[Trade], rule: &Tpsl2Rule) -> Option<EntryFill> {
        if !rule_configures_any_scalp_gate(rule) || trades.is_empty() {
            return None;
        }
        let min_age = none_if_zero_u64(rule.p_entry_min_age_secs).map(|v| v as i64);
        let min_alive = none_if_zero_f64(rule.p_entry_min_alive_sol);
        let min_organic = none_if_zero_f64(rule.p_entry_min_organic_sol);
        let pullback = none_if_zero_f64(rule.p_entry_pullback_pct);
        let hl_secs = none_if_zero_u64(rule.p_entry_higher_low_secs).map(|v| v as i64).unwrap_or(0);
        let max_cohort_held = none_if_zero_f64(rule.p_entry_max_cohort_held);
        let min_liq = none_if_zero_f64(rule.p_entry_min_liquidity_sol);
        let min_organic_liq = none_if_zero_f64(rule.p_entry_min_organic_liq);
        for i in 0..trades.len() {
            let cand = &trades[i];
            if cand.trade_type != TradeType::Buy || cand.price_per_token <= 0.0 {
                continue;
            }
            let prefix = &trades[..=i];
            let Some(f) = scalp_features(prefix) else { continue };
            if min_age.is_some_and(|m| f.age_secs < m) { continue; }
            if min_alive.is_some_and(|m| f.alive_sol < m) { continue; }
            if min_organic.is_some_and(|m| f.organic_sol < m) { continue; }
            if max_cohort_held.is_some_and(|m| f.cohort_held_ratio > m / 100.0) { continue; }
            if min_liq.is_some_and(|m| f.real_liquidity_sol < m) { continue; }
            if min_organic_liq.is_some_and(|m| f.organic_liq_sol < m) { continue; }
            if let Some(pb) = pullback {
                if !higher_low_confirmed(prefix, pb, hl_secs) { continue; }
            }
            return Some(EntryFill {
                price: cand.price_per_token,
                amount_sol: cand.sol_amount,
                tx_signature: cand.tx_signature.clone(),
                block_time: cand.block_time,
            });
        }
        None
    }

    #[test]
    fn linearized_scalp_entry_matches_prefix_oracle() {
        // A mixed series exercising every gate: cohort buys, a cohort dump, outside
        // demand, reserve snapshots, and a higher-low price shape.
        let mut low = buy("dev", 5.0, 100.0, 1, 0);
        low.real_sol_reserves = Some(4.0);
        let trades = vec![
            low,
            buy("dev2", 3.0, 60.0, 2, 1),
            t("dev", TradeType::Sell, 4.0, 90.0, 50, 3),
            {
                let mut x = buy("out1", 2.0, 18.0, 500, 6);
                x.real_sol_reserves = Some(12.0);
                x
            },
            t("out1", TradeType::Sell, 0.5, 4.0, 520, 8),
            {
                let mut x = buy("out2", 1.5, 12.0, 540, 11);
                x.price_per_token = 0.9; // higher-low after the early dip
                x.real_sol_reserves = Some(13.0);
                x
            },
        ];

        // Sweep several gate combinations; each must agree with the oracle.
        let combos: &[(Option<u64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>)] = &[
            (Some(5), None, None, None, None, None),
            (None, Some(2.0), None, None, None, None),
            (None, None, Some(1.0), None, None, None),
            (None, None, None, Some(30.0), None, None), // cohort-held %: ≤ 30%
            (None, None, None, None, Some(10.0), None),
            (None, None, None, None, None, Some(5.0)),
            (Some(5), Some(1.0), Some(1.0), Some(50.0), Some(5.0), Some(1.0)),
        ];
        for &(age, alive, organic, cohort_held, liq, organic_liq) in combos {
            let mut r = rule();
            r.p_entry_min_age_secs = age;
            r.p_entry_min_alive_sol = alive;
            r.p_entry_min_organic_sol = organic;
            r.p_entry_max_cohort_held = cohort_held;
            r.p_entry_min_liquidity_sol = liq;
            r.p_entry_min_organic_liq = organic_liq;
            assert_eq!(
                find_scalp_entry(&trades, &r),
                scalp_entry_oracle(&trades, &r),
                "linearized find_scalp_entry diverged from the per-prefix oracle for combo {:?}",
                (age, alive, organic, cohort_held, liq, organic_liq)
            );
        }
    }

    #[test]
    fn with_cohort_matches_inline_cohort() {
        // The #7 cohort hoist: feeding `scalp_cohort(trades)` into
        // `find_scalp_entry_with_cohort` must resolve byte-identical entries to the
        // inline-cohort `find_scalp_entry` — that equality is what lets the sweep
        // build the cohort once per token without changing any decision.
        let trades = vec![
            buy("dev", 5.0, 100.0, 1, 0),
            buy("dev2", 3.0, 60.0, 2, 1),
            t("dev", TradeType::Sell, 4.0, 90.0, 50, 3),
            buy("out", 1.0, 5.0, 500, 5),
            buy("out2", 1.5, 12.0, 540, 7),
        ];
        let cohort = scalp_cohort(&trades);
        for &(age, organic, cohort_held, liq) in &[
            (Some(5u64), None, None, None),
            (None, Some(1.0f64), None, None),
            (None, None, Some(50.0f64), None),
            (None, None, None, Some(0.0f64)),
            (Some(5), Some(1.0), Some(50.0), None),
        ] {
            let mut r = rule();
            r.p_entry_min_age_secs = age;
            r.p_entry_min_organic_sol = organic;
            r.p_entry_max_cohort_held = cohort_held;
            r.p_entry_min_liquidity_sol = liq;
            assert_eq!(
                find_scalp_entry(&trades, &r),
                find_scalp_entry_with_cohort(&trades, &r, &cohort),
                "hoisted-cohort entry diverged from inline-cohort for {:?}",
                (age, organic, cohort_held, liq)
            );
        }
    }

    // ── higher_low_confirmed ─────────────────────────────────────────────────

    #[test]
    fn higher_low_confirms_on_rising_lows() {
        // 1.00 up, dip to 0.85 (−15%), bounce 1.05, dip to 0.92 (higher low), up.
        let trades = vec![
            pbuy(1.00, 1, 0),
            pbuy(0.85, 2, 2),
            pbuy(1.05, 3, 4),
            pbuy(0.92, 4, 6),
            pbuy(0.98, 5, 8),
        ];
        assert!(higher_low_confirmed(&trades, 15.0, 0));
    }

    #[test]
    fn wiggles_below_pullback_never_confirm() {
        // 1.00 → 0.98 → 1.01 → 0.99: never dips 15%.
        let trades = vec![pbuy(1.00, 1, 0), pbuy(0.98, 2, 2), pbuy(1.01, 3, 4), pbuy(0.99, 4, 6)];
        assert!(!higher_low_confirmed(&trades, 15.0, 0));
    }

    #[test]
    fn freefall_makes_no_higher_low() {
        let trades = vec![pbuy(1.00, 1, 0), pbuy(0.80, 2, 2), pbuy(0.50, 3, 4), pbuy(0.30, 4, 6)];
        assert!(!higher_low_confirmed(&trades, 15.0, 0));
    }

    #[test]
    fn higher_low_respects_min_span() {
        // Same higher-low shape but the two dips are only 4s apart.
        let trades = vec![
            pbuy(1.00, 1, 0),
            pbuy(0.85, 2, 2),
            pbuy(1.05, 3, 3),
            pbuy(0.92, 4, 6),
            pbuy(0.98, 5, 7),
        ];
        // dip1 @ +2s, dip2 @ +6s → 4s span: a 10s minimum blocks it, 3s allows it.
        assert!(!higher_low_confirmed(&trades, 15.0, 10));
        assert!(higher_low_confirmed(&trades, 15.0, 3));
    }

    // ── find_worst_case_paper_entry ──────────────────────────────────────────

    /// A priced buy at `slot`/`leg`/`secs` with an explicit `sol` size and a tx
    /// signature unique across `(slot, leg)` so same-slot legs don't collide.
    fn leg(sol: f64, tokens: f64, slot: u64, leg: u32, secs: i64) -> Trade {
        let mut tr = buy("w", sol, tokens, slot, secs);
        tr.leg_index = leg;
        tr.tx_signature = format!("sig-{slot}-{leg}");
        tr
    }

    #[test]
    fn worst_case_picks_highest_price_after_trigger_same_block() {
        // Trigger at slot 100 leg 0 (price 1.0). Two adverse buys later in the
        // same slot: price 1.2 and 1.5 → the 1.5 is the worst case.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.2, 1.0, 100, 1, 0),
            leg(1.5, 1.0, 100, 2, 0),
        ];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature);
        assert_eq!(entry.tx_signature, "sig-100-2");
        assert_eq!(entry.price, 1.5);
        // amount_sol is the TRIGGER's sol, not the adverse trade's.
        assert_eq!(entry.amount_sol, trigger.sol_amount);
    }

    #[test]
    fn worst_case_spans_trigger_slot_and_next() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.3, 1.0, 100, 1, 0), // same slot
            leg(1.8, 1.0, 101, 0, 1), // next slot — highest
            leg(2.0, 1.0, 102, 0, 2), // S+2 — out of window, ignored
        ];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature);
        assert_eq!(entry.price, 1.8);
    }

    #[test]
    fn worst_case_filters_dust_and_zero_price() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        // Adverse candidates: one dust (sub-MIN_TRADE_SOL), one zero-priced
        // (token_amount 0 → price 0), one valid at 1.1.
        let dust_sol = crate::config::constants::MIN_TRADE_SOL / 2.0;
        let mut dust = leg(dust_sol, 1.0, 100, 1, 0);
        dust.sol_amount = dust_sol;
        let mut zero = leg(1.0, 0.0, 100, 2, 0); // tokens 0 → price 0
        zero.price_per_token = 0.0;
        let valid = leg(1.1, 1.0, 100, 3, 0);
        let trades = vec![trigger.clone(), dust, zero, valid];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature);
        assert_eq!(entry.price, 1.1);
    }

    #[test]
    fn worst_case_tie_breaks_to_latest() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.5, 1.0, 100, 1, 0),
            leg(1.5, 1.0, 101, 0, 1), // same price, later key → wins the tie
        ];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature);
        assert_eq!(entry.block_time, base_time() + chrono::Duration::seconds(1));
    }

    #[test]
    fn worst_case_empty_pool_falls_back_to_trigger() {
        // Trigger is the only candidate (nothing strictly after it in S/S+1).
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![trigger.clone()];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature);
        assert_eq!(entry.tx_signature, trigger.tx_signature);
        assert_eq!(entry.price, trigger.price_per_token);
        assert_eq!(entry.amount_sol, trigger.sol_amount);
    }
}
