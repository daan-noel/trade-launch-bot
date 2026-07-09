//! Scalp-continuation entry gates (see `@project_plans/@TPSL_plan/
//! tpsl-scalp-continuation-plan.md`). Where the legacy entry decides at token
//! creation from `Token` fields, the scalp gates decide **on the trade stream**:
//! "buy on the first trade where ALL configured gates hold." Each gate is a pure
//! function of a token's chronological trade slice, so the backtest and (later) a
//! live trade-watcher resolve identical entries.
//!
//! The gates, all inert at `None`/`0`:
//!   • `p_entry_min_age_secs`      — skip the launch spike (entry age ≥ N s after t0).
//!   • `p_entry_max_age_secs`      — entry-window ceiling (entry age ≤ N s after t0).
//!     A bound, NOT a positive gate: it never makes a rule enter on its own, so it
//!     is deliberately excluded from `rule_configures_any_scalp_gate`. Because the
//!     ceiling lives here, the backtest, paper, and real paths all stop entering
//!     past it by construction — the live deadline merely derives from it.
//!   • `p_entry_min_alive_sol`     — still trading (total SOL in the trailing window).
//!   • `p_entry_min_net_buy_sol`   — net demand: net SOL bought so far
//!     (Σbuys − Σsells, any wallet). A demand *flow*, not pool liquidity.
//!   • `p_entry_pullback_pct` (+ `p_entry_higher_low_secs`) — higher-low continuation shape.
//!   • `p_entry_min_liquidity_sol` — real reserves ≥ N SOL.

use chrono::{DateTime, Utc};

use super::super::util::{none_if_zero_f64, none_if_zero_u64};
use super::EntryFill;
use crate::config::constants::MAX_FILL_WAIT_SLOTS;
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
        || none_if_zero_f64(rule.p_entry_min_net_buy_sol).is_some()
        || none_if_zero_f64(rule.p_entry_pullback_pct).is_some()
        || none_if_zero_f64(rule.p_entry_min_liquidity_sol).is_some()
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
    /// Net SOL bought so far (Σbuys − Σsells, any wallet) — net demand.
    pub net_buy_sol: f64,
    /// Latest known real SOL reserves at the candidate.
    pub real_liquidity_sol: f64,
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
        .map(|t| t.amount_sol)
        .sum();

    let net_buy_sol: f64 = prefix
        .iter()
        .map(|t| if t.is_buy() { t.amount_sol } else { -t.amount_sol })
        .sum();

    // Latest known real reserves: scan back for the most recent trade carrying a
    // real_reserve_sol snapshot (the plan mandates REAL, never virtual).
    let real_liquidity_sol = prefix
        .iter()
        .rev()
        .find_map(|t| t.real_reserve_sol)
        .unwrap_or(0.0);

    Some(ScalpFeatures { age_secs, alive_sol, net_buy_sol, real_liquidity_sol })
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
pub fn higher_low_confirmed_index<T: TradeRow>(
    trades: &[T],
    pullback_pct: f64,
    min_span_secs: i64,
) -> Option<usize> {
    if pullback_pct <= 0.0 {
        return None;
    }

    // Single forward pass over the price-positive trades — no intermediate `series`
    // Vec (this resolves once per entry-key per token, so the alloc was pure waste).
    // The first price-positive trade seeds `high` and is skipped (mirrors the old
    // `series.first()` + `skip(1)`); price≤0 rows are ignored inline.
    let mut high = 0.0f64;
    let mut seeded = false;
    let mut cur_low: Option<(DateTime<Utc>, f64)> = None;
    let mut prev_low: Option<(DateTime<Utc>, f64)> = None;
    let mut established = false;

    for (idx, tr) in trades.iter().enumerate() {
        let price = tr.price_per_token();
        if price <= 0.0 {
            continue;
        }
        if !seeded {
            high = price;
            seeded = true;
            continue;
        }
        let t = tr.block_time();
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

/// Walk a token's trades and return the entry fill at the **first trade where
/// every configured scalp gate holds**, or `None` if none qualifies. The
/// candidate must be a buy (we enter by buying, filling at its price). Trades are
/// assumed chronologically sorted upstream.
///
/// Returns `None` when the rule configures no scalp gate; callers gate on
/// [`rule_configures_any_scalp_gate`] (the backtest rejects such a rule up front),
/// so this is never a silent buy-everything path.
///
/// Thin wrapper over [`find_scalp_entry_indexed`] dropping the trigger index —
/// used by every caller that doesn't need it (live/backtest/registry).
pub fn find_scalp_entry<T: TradeRow>(trades: &[T], rule: &Tpsl2Rule) -> Option<EntryFill> {
    find_scalp_entry_indexed(trades, rule).map(|(_, fill)| fill)
}

/// [`find_scalp_entry`] that also returns the trigger trade's **index** in
/// `trades`. The sweep uses this directly (its slim `CorpusTrade` carries no
/// `tx_signature`) to hand the index straight to [`find_worst_case_paper_entry_at`]
/// (string-free); the live cache row (`CachedTrade`) is likewise signature-free, so
/// the paper entry resolver uses this form too.
pub fn find_scalp_entry_indexed<T: TradeRow>(
    trades: &[T],
    rule: &Tpsl2Rule,
) -> Option<(usize, EntryFill)> {
    if !rule_configures_any_scalp_gate(rule) || trades.is_empty() {
        return None;
    }

    let min_age = none_if_zero_u64(rule.p_entry_min_age_secs).map(|v| v as i64);
    // Entry-window ceiling (a bound, not a gate). Once a candidate's age exceeds
    // it, no later candidate can qualify (age is monotonic), so we `break`.
    let max_age = none_if_zero_u64(rule.p_entry_max_age_secs).map(|v| v as i64);
    let min_alive = none_if_zero_f64(rule.p_entry_min_alive_sol);
    let min_net_buy = none_if_zero_f64(rule.p_entry_min_net_buy_sol);
    let pullback = none_if_zero_f64(rule.p_entry_pullback_pct);
    let higher_low_secs = none_if_zero_u64(rule.p_entry_higher_low_secs).map(|v| v as i64).unwrap_or(0);
    let min_liq = none_if_zero_f64(rule.p_entry_min_liquidity_sol);

    // O(n) single forward pass (M3): carrying the running net-SOL flow, the
    // trailing alive-window sum, and the last-seen real reserves forward as the
    // walk advances replaces per-candidate prefix rescans (was O(n²)).

    // First index (if any) at which the higher-low shape is confirmed; the
    // confirmation is monotonic in prefix length, so one forward pass suffices.
    let higher_low_idx = pullback.and_then(|pb| higher_low_confirmed_index(trades, pb, higher_low_secs));

    let first_time = trades[0].block_time();
    let mut net_buy_sol = 0.0f64; // running net SOL bought so far (any wallet)
    let mut last_real_reserves = 0.0f64; // most recent real_reserve_sol snapshot
    // Trailing alive-window: a running sum + a front cursor over `trades`.
    let mut alive_sol = 0.0f64;
    let mut alive_lo = 0usize;

    for i in 0..trades.len() {
        let t = &trades[i];
        let block_time = t.block_time();
        let amount_sol = t.amount_sol();
        // Fold this trade into the running accumulators FIRST so the features
        // below reflect the prefix `[0..=i]` (the candidate inclusive).
        if t.is_buy() {
            net_buy_sol += amount_sol;
        } else {
            net_buy_sol -= amount_sol;
        }
        if let Some(r) = t.real_reserve_sol() {
            last_real_reserves = r;
        }
        // Slide the alive window to [cand.block_time - ALIVE_WINDOW_SECS, cand].
        alive_sol += amount_sol;
        let alive_cutoff = block_time - chrono::Duration::seconds(ALIVE_WINDOW_SECS);
        while alive_lo <= i && trades[alive_lo].block_time() < alive_cutoff {
            alive_sol -= trades[alive_lo].amount_sol();
            alive_lo += 1;
        }

        if !t.is_buy() || t.price_per_token() <= 0.0 {
            continue;
        }

        let age_secs = (block_time - first_time).num_seconds();
        // Past the entry-window ceiling: no later (older) candidate can qualify, so
        // stop the walk entirely. This is the shared bound that keeps sim/paper/real
        // honoring the identical window.
        if let Some(max) = max_age {
            if age_secs > max {
                break;
            }
        }

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
        if let Some(min) = min_net_buy {
            if net_buy_sol < min {
                continue;
            }
        }
        if let Some(min) = min_liq {
            if last_real_reserves < min {
                continue;
            }
        }
        if pullback.is_some() {
            match higher_low_idx {
                Some(idx) if i >= idx => {}
                _ => continue,
            }
        }

        return Some((
            i,
            EntryFill {
                price: t.price_per_token(),
                amount_tokens: t.token_amount(),
                tx_signature: t.tx_signature().to_string(),
                slot: t.slot(),
                block_time,
            },
        ));
    }
    None
}

/// Paper worst-case entry. Given the mint's chronological `trades` and the trigger
/// trade identified by `target_tx` (the scalp-entry signal), delegates to
/// [`find_worst_case_paper_entry_at`] after locating the trigger by signature.
/// Returns `None` when the trigger isn't in the slice or when no qualifying buy
/// exists within [`MAX_FILL_WAIT_SLOTS`] after the trigger.
pub fn find_worst_case_paper_entry<T: TradeRow>(trades: &[T], target_tx: &str) -> Option<EntryFill> {
    let idx = trades.iter().position(|t| t.tx_signature() == target_tx)?;
    find_worst_case_paper_entry_at(trades, idx)
}

/// [`find_worst_case_paper_entry`] keyed by the trigger trade's **index** rather
/// than its `tx_signature`. The sweep resolves the trigger index from
/// [`find_scalp_entry_indexed`] and calls this directly, so its
/// [`CorpusTrade`] rows need carry no signature string at all (Phase 1.2).
///
/// Fill model: window = trigger slot S (always) + the next observed slot after S
/// if it's within [`MAX_FILL_WAIT_SLOTS`]. Only trades at indices > `target_idx`
/// are considered (i.e. trades after the trigger tx in the same slot are eligible).
/// Fill = highest price in the window (worst case for us). Returns `None` when the
/// window contains no qualifying buy (not dust, `price_per_token > 0`).
///
/// `target_idx` must index a real trade in `trades`.
///
/// `CorpusTrade` lives in the local-only sweep crate (`sweep::projection::CorpusTrade`).
pub fn find_worst_case_paper_entry_at<T: TradeRow>(trades: &[T], target_idx: usize) -> Option<EntryFill> {
    let trigger_slot = trades[target_idx].slot();
    let post = trades.get(target_idx + 1..).unwrap_or(&[]);

    // First slot > trigger_slot that has a qualifying buy — used only for the
    // MAX_FILL_WAIT_SLOTS proximity check, not to restrict the window.
    let next_slot = post
        .iter()
        .filter(|t| t.slot() > trigger_slot && t.is_buy() && t.price_per_token() > 0.0 && !Trade::is_dust(t.amount_sol()))
        .map(|t| t.slot())
        .next();

    // Window: trigger slot S (always) + next_slot only if close enough.
    let best = post
        .iter()
        .filter(|t| {
            let s = t.slot();
            let in_s = s == trigger_slot;
            let in_next = next_slot.is_some_and(|ns| s == ns && ns <= trigger_slot + MAX_FILL_WAIT_SLOTS);
            (in_s || in_next) && t.is_buy() && t.price_per_token() > 0.0 && !Trade::is_dust(t.amount_sol())
        })
        .max_by(|a, b| a.price_per_token().total_cmp(&b.price_per_token()))?;

    Some(EntryFill {
        price: best.price_per_token(),
        amount_tokens: best.token_amount(),
        tx_signature: best.tx_signature().to_string(),
        slot: best.slot(),
        block_time: best.block_time(),
    })
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
            tokens as u64,
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
            Some(0.1),
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
    fn max_age_gate_rejects_entries_past_the_ceiling() {
        // Candidates at +0s, +4s, +12s. A 10s ceiling rejects the +12s trade; a
        // floor+ceiling window of [2, 10] admits only the +4s trade.
        let trades = vec![pbuy(1.0, 1, 0), pbuy(1.0, 2, 4), pbuy(1.0, 3, 12)];

        // Ceiling alone is NOT a positive gate: a rule with only max_age set never
        // enters (it isn't a configured scalp gate).
        let mut ceiling_only = rule();
        ceiling_only.p_entry_max_age_secs = Some(10);
        assert!(
            find_scalp_entry(&trades, &ceiling_only).is_none(),
            "max_age alone must not make a rule enter"
        );

        // Window [2, 10]: the +0s trade is too young, the +12s too old, so the
        // first (and only) qualifying candidate is the +4s trade.
        let mut windowed = rule();
        windowed.p_entry_min_age_secs = Some(2);
        windowed.p_entry_max_age_secs = Some(10);
        let fill = find_scalp_entry(&trades, &windowed).expect("the +4s trade is inside the window");
        assert_eq!(fill.block_time, base_time() + chrono::Duration::seconds(4));

        // A min_age beyond the ceiling leaves an empty window → no entry.
        let mut empty = rule();
        empty.p_entry_min_age_secs = Some(13);
        empty.p_entry_max_age_secs = Some(10);
        assert!(find_scalp_entry(&trades, &empty).is_none(), "empty window enters nothing");
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
    fn net_buy_gate_requires_net_demand() {
        let mut r = rule();
        r.p_entry_min_net_buy_sol = Some(1.0);
        // No buys at all yet → net SOL is 0 → never enters.
        let none_yet = vec![t("dev", TradeType::Sell, 5.0, 100.0, 1, 0)];
        assert!(find_scalp_entry(&none_yet, &r).is_none());
        // A buy of 2 SOL clears the 1.0 SOL net-demand floor.
        let with_buy = vec![buy("dev", 2.0, 20.0, 500, 5)];
        let fill = find_scalp_entry(&with_buy, &r).expect("net demand clears the floor");
        assert_eq!(fill.tx_signature, "sig-dev-500-5");
    }

    #[test]
    fn liquidity_gate_uses_real_reserves() {
        let mut r = rule();
        r.p_entry_min_liquidity_sol = Some(10.0);
        let mut low = pbuy(1.0, 1, 0);
        low.real_reserve_sol = Some(3.0);
        let mut high = buy("out", 1.0, 1.0, 2, 2);
        high.real_reserve_sol = Some(15.0);
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
        let max_age = none_if_zero_u64(rule.p_entry_max_age_secs).map(|v| v as i64);
        let min_alive = none_if_zero_f64(rule.p_entry_min_alive_sol);
        let min_net_buy = none_if_zero_f64(rule.p_entry_min_net_buy_sol);
        let pullback = none_if_zero_f64(rule.p_entry_pullback_pct);
        let hl_secs = none_if_zero_u64(rule.p_entry_higher_low_secs).map(|v| v as i64).unwrap_or(0);
        let min_liq = none_if_zero_f64(rule.p_entry_min_liquidity_sol);
        for i in 0..trades.len() {
            let cand = &trades[i];
            if cand.trade_type != TradeType::Buy || cand.price_per_token <= 0.0 {
                continue;
            }
            let prefix = &trades[..=i];
            let Some(f) = scalp_features(prefix) else { continue };
            if max_age.is_some_and(|m| f.age_secs > m) { break; }
            if min_age.is_some_and(|m| f.age_secs < m) { continue; }
            if min_alive.is_some_and(|m| f.alive_sol < m) { continue; }
            if min_net_buy.is_some_and(|m| f.net_buy_sol < m) { continue; }
            if min_liq.is_some_and(|m| f.real_liquidity_sol < m) { continue; }
            if let Some(pb) = pullback {
                if !higher_low_confirmed(prefix, pb, hl_secs) { continue; }
            }
            return Some(EntryFill {
                price: cand.price_per_token,
                // EntryFill.amount_tokens is f64; token_amount is exact raw u64.
                amount_tokens: cand.token_amount as f64,
                tx_signature: cand.tx_signature.clone(),
                slot: cand.slot,
                block_time: cand.block_time,
            });
        }
        None
    }

    #[test]
    fn linearized_scalp_entry_matches_prefix_oracle() {
        // A mixed series exercising every gate: buys/sells, reserve snapshots, and
        // a higher-low price shape.
        let mut low = buy("dev", 5.0, 100.0, 1, 0);
        low.real_reserve_sol = Some(4.0);
        let trades = vec![
            low,
            buy("dev2", 3.0, 60.0, 2, 1),
            t("dev", TradeType::Sell, 4.0, 90.0, 50, 3),
            {
                let mut x = buy("out1", 2.0, 18.0, 500, 6);
                x.real_reserve_sol = Some(12.0);
                x
            },
            t("out1", TradeType::Sell, 0.5, 4.0, 520, 8),
            {
                let mut x = buy("out2", 1.5, 12.0, 540, 11);
                x.price_per_token = 0.9; // higher-low after the early dip
                x.real_reserve_sol = Some(13.0);
                x
            },
        ];

        // Sweep several gate combinations; each must agree with the oracle. The
        // trailing field is `max_age` (the entry-window ceiling) so the linearized
        // `break` is pinned against the oracle alongside every other gate.
        type Combo = (Option<u64>, Option<f64>, Option<f64>, Option<f64>, Option<u64>);
        let combos: &[Combo] = &[
            (Some(5), None, None, None, None),
            (None, Some(2.0), None, None, None),
            (None, None, Some(1.0), None, None),
            (None, None, None, Some(10.0), None),
            (Some(5), Some(1.0), Some(1.0), Some(5.0), None),
            // max_age alone is inert (not a positive gate) → no entry.
            (None, None, None, None, Some(5)),
            // min_age floor paired with a max_age ceiling = a real window.
            (Some(1), None, None, None, Some(8)),
            // A real gate under a tight ceiling that cuts the walk short.
            (None, Some(1.0), None, None, Some(2)),
        ];
        for &(age, alive, net_buy, liq, max_age) in combos {
            let mut r = rule();
            r.p_entry_min_age_secs = age;
            r.p_entry_min_alive_sol = alive;
            r.p_entry_min_net_buy_sol = net_buy;
            r.p_entry_min_liquidity_sol = liq;
            r.p_entry_max_age_secs = max_age;
            assert_eq!(
                find_scalp_entry(&trades, &r),
                scalp_entry_oracle(&trades, &r),
                "linearized find_scalp_entry diverged from the per-prefix oracle for combo {:?}",
                (age, alive, net_buy, liq, max_age)
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

    /// A priced buy at `slot`/`leg`/`secs` with explicit sol/tokens and a unique sig.
    fn leg(sol: f64, tokens: f64, slot: u64, leg: u32, secs: i64) -> Trade {
        let mut tr = buy("w", sol, tokens, slot, secs);
        tr.leg_index = leg;
        tr.tx_signature = format!("sig-{slot}-{leg}");
        tr
    }

    #[test]
    fn worst_case_fills_in_window_of_trigger_and_next_slot() {
        // Trigger at slot 100. Window = {slot 100 (after trigger), slot 101 (next_slot)}.
        // Slot 102 is beyond next_slot and excluded.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.2, 1.0, 100, 1, 0), // same slot, after trigger — included
            leg(1.5, 1.0, 101, 0, 1), // next slot — included
            leg(1.8, 1.0, 101, 1, 1), // next slot — higher price, worst case
            leg(2.0, 1.0, 102, 0, 2), // beyond next_slot — excluded
        ];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature)
            .expect("qualifying buy in window");
        assert_eq!(entry.price, 1.8); // max price across {slot 100, slot 101}
    }

    #[test]
    fn worst_case_fills_from_trigger_slot_when_no_next_slot() {
        // Buy only in the trigger slot (after the trigger tx) → fills from slot S.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![
            trigger.clone(),
            leg(1.5, 1.0, 100, 1, 0), // same slot, after trigger — included
        ];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature)
            .expect("qualifying buy in trigger slot");
        assert!((entry.price - 1.5).abs() < 1e-9);
    }

    #[test]
    fn worst_case_filters_dust_and_zero_price_in_fill_slot() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let dust_sol = crate::config::constants::MIN_TRADE_SOL / 2.0;
        let mut dust = leg(dust_sol, 1.0, 101, 0, 1);
        dust.amount_sol = dust_sol;
        let mut zero = leg(1.0, 0.0, 101, 1, 1); // price 0 → excluded
        zero.price_per_token = 0.0;
        let valid = leg(1.1, 1.0, 101, 2, 1);
        let trades = vec![trigger.clone(), dust, zero, valid];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature)
            .expect("valid buy in slot 101");
        assert_eq!(entry.price, 1.1);
    }

    #[test]
    fn worst_case_no_fill_when_all_post_trigger_are_sells() {
        // Only sells after the trigger → no qualifying buy → None.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let mut sell = leg(0.9, 1.0, 101, 0, 1);
        sell.trade_type = crate::models::trade::TradeType::Sell;
        let trades = vec![trigger.clone(), sell];
        assert!(find_worst_case_paper_entry(&trades, &trigger.tx_signature).is_none());
    }

    #[test]
    fn worst_case_no_fill_when_window_empty() {
        // Nothing after the trigger at all.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![trigger.clone()];
        assert!(find_worst_case_paper_entry(&trades, &trigger.tx_signature).is_none());
    }

    #[test]
    fn worst_case_no_fill_past_max_wait() {
        // next_slot is beyond MAX_FILL_WAIT_SLOTS and trigger slot has no post-trigger buys → None.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let late = leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS + 1, 0, 5);
        let trades = vec![trigger.clone(), late];
        assert!(find_worst_case_paper_entry(&trades, &trigger.tx_signature).is_none());
    }

    #[test]
    fn worst_case_fills_at_max_wait_boundary() {
        // First qualifying buy is exactly MAX_FILL_WAIT_SLOTS away → Some.
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let boundary = leg(1.5, 1.0, 100 + MAX_FILL_WAIT_SLOTS, 0, 5);
        let trades = vec![trigger.clone(), boundary];
        let entry = find_worst_case_paper_entry(&trades, &trigger.tx_signature)
            .expect("boundary slot qualifies");
        assert!((entry.price - 1.5).abs() < 1e-9);
    }

    #[test]
    fn worst_case_unknown_trigger_returns_none() {
        let trigger = leg(1.0, 1.0, 100, 0, 0);
        let trades = vec![trigger, leg(1.5, 1.0, 101, 0, 1)];
        assert!(find_worst_case_paper_entry(&trades, "missing-sig").is_none());
    }
}
