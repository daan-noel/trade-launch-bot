//! Scalp-continuation entry gates (see `@project_plans/@TPSL_plan/
//! tpsl-scalp-continuation-plan.md`). Where the legacy entry decides at token
//! creation from `Token` fields, the scalp gates decide **on the trade stream**:
//! "buy on the first trade where ALL configured gates hold." Each gate is a pure
//! function of a token's chronological trade slice, so the backtest and (later) a
//! live trade-watcher resolve identical entries.
//!
//! The gates, all inert at `None`/`0`:
//!   • `p_min_age_secs`      — skip the launch spike (entry age ≥ N s after t0).
//!   • `p_min_alive_sol`     — still trading (total SOL in the trailing window).
//!   • `p_min_organic_sol`   — new people buying (net SOL by outside wallets).
//!   • `p_pullback_pct` (+ `p_higher_low_secs`) — higher-low continuation shape.
//!   • `p_max_cohort_held`   — launch cohort already sold its bag.
//!   • `p_min_liquidity_sol` — real reserves ≥ N SOL.
//!   • `p_min_organic_liq`   — real reserves from buyers, not one dev deposit.
//!
//! Cohort / outside split reuses [`super::super::cohort`] so it matches rug
//! detection and the E5 exit exactly.

use chrono::{DateTime, Utc};

use super::super::cohort::{cohort_flow, early_cohort_wallets, outside_net_sol};
use super::super::util::{none_if_zero_f64, none_if_zero_u64};
use super::EntryFill;
use crate::config::constants::RUGGED_EARLY_SLOT_WINDOW;
use crate::models::trade::{Trade, TradeType};
use crate::models::Tpsl2StrategyRule;

/// Liveness window for `p_min_alive_sol`: total SOL traded in the trailing
/// `ALIVE_WINDOW_SECS` ending at the candidate trade. Kept a module constant (not
/// a per-rule param) so the "still trading" gate has one well-defined window;
/// sized to the launch-spike horizon. Revisit here if it needs tuning.
const ALIVE_WINDOW_SECS: i64 = 10;

/// Whether a rule configures **any** scalp entry gate. When false, the scalp
/// entry path is skipped entirely (callers fall back to the legacy entry-fill),
/// so a rule with no scalp gates behaves exactly as before.
pub fn rule_configures_any_scalp_gate(rule: &Tpsl2StrategyRule) -> bool {
    none_if_zero_u64(rule.p_min_age_secs).is_some()
        || none_if_zero_f64(rule.p_min_alive_sol).is_some()
        || none_if_zero_f64(rule.p_min_organic_sol).is_some()
        || none_if_zero_f64(rule.p_pullback_pct).is_some()
        || none_if_zero_f64(rule.p_max_cohort_held).is_some()
        || none_if_zero_f64(rule.p_min_liquidity_sol).is_some()
        || none_if_zero_f64(rule.p_min_organic_liq).is_some()
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

    let cohort = early_cohort_wallets(prefix, RUGGED_EARLY_SLOT_WINDOW);
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

/// Walk a token's trades and return the entry fill at the **first trade where
/// every configured scalp gate holds**, or `None` if none qualifies. The
/// candidate must be a buy (we enter by buying, filling at its price). Trades are
/// assumed chronologically sorted upstream.
///
/// Returns `None` when the rule configures no scalp gate — callers must check
/// [`rule_configures_any_scalp_gate`] and fall back to the legacy fill, so this
/// can never silently buy the first trade.
pub fn find_scalp_entry(trades: &[Trade], rule: &Tpsl2StrategyRule) -> Option<EntryFill> {
    if !rule_configures_any_scalp_gate(rule) || trades.is_empty() {
        return None;
    }

    let min_age = none_if_zero_u64(rule.p_min_age_secs).map(|v| v as i64);
    let min_alive = none_if_zero_f64(rule.p_min_alive_sol);
    let min_organic = none_if_zero_f64(rule.p_min_organic_sol);
    let pullback = none_if_zero_f64(rule.p_pullback_pct);
    let higher_low_secs = none_if_zero_u64(rule.p_higher_low_secs).map(|v| v as i64).unwrap_or(0);
    let max_cohort_held = none_if_zero_f64(rule.p_max_cohort_held);
    let min_liq = none_if_zero_f64(rule.p_min_liquidity_sol);
    let min_organic_liq = none_if_zero_f64(rule.p_min_organic_liq);

    for i in 0..trades.len() {
        let cand = &trades[i];
        if cand.trade_type != TradeType::Buy || cand.price_per_token <= 0.0 {
            continue;
        }
        let prefix = &trades[..=i];
        let Some(f) = scalp_features(prefix) else {
            continue;
        };

        if let Some(min) = min_age {
            if f.age_secs < min {
                continue;
            }
        }
        if let Some(min) = min_alive {
            if f.alive_sol < min {
                continue;
            }
        }
        if let Some(min) = min_organic {
            if f.organic_sol < min {
                continue;
            }
        }
        if let Some(max) = max_cohort_held {
            if f.cohort_held_ratio > max {
                continue;
            }
        }
        if let Some(min) = min_liq {
            if f.real_liquidity_sol < min {
                continue;
            }
        }
        if let Some(min) = min_organic_liq {
            if f.organic_liq_sol < min {
                continue;
            }
        }
        if let Some(pb) = pullback {
            if !higher_low_confirmed(prefix, pb, higher_low_secs) {
                continue;
            }
        }

        return Some(EntryFill {
            price: cand.price_per_token,
            tx_signature: cand.tx_signature.clone(),
            block_time: cand.block_time,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rule() -> Tpsl2StrategyRule {
        Tpsl2StrategyRule::new(
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
        r.p_min_age_secs = Some(10);
        let fill = find_scalp_entry(&trades, &r).expect("should enter once old enough");
        // First candidate with age >= 10s is the trade at +12s.
        assert_eq!(fill.block_time, base_time() + chrono::Duration::seconds(12));
    }

    #[test]
    fn alive_gate_requires_recent_volume() {
        let mut r = rule();
        r.p_min_alive_sol = Some(2.0);
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
        r.p_min_organic_sol = Some(1.0);
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
        r.p_max_cohort_held = Some(0.30);
        // Cohort holds its full bag (ratio 1.0) → blocked on the outside buy.
        let holding = vec![buy("dev", 5.0, 100.0, 1, 0), buy("out", 1.0, 5.0, 500, 5)];
        assert!(find_scalp_entry(&holding, &r).is_none());
        // Cohort dumps 90% (ratio 0.1 ≤ 0.3), THEN an outside wallet buys — we
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
        r.p_min_liquidity_sol = Some(10.0);
        let mut low = pbuy(1.0, 1, 0);
        low.real_sol_reserves = Some(3.0);
        let mut high = buy("out", 1.0, 1.0, 2, 2);
        high.real_sol_reserves = Some(15.0);
        // Only the second trade clears 10 SOL real reserves.
        let fill = find_scalp_entry(&vec![low, high], &r).expect("enters once liquid");
        assert_eq!(fill.block_time, base_time() + chrono::Duration::seconds(2));
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
}
