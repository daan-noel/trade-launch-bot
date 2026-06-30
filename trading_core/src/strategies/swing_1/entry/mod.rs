//! swing1 entry resolution — gate on the kill→volume latch, then take the first
//! confirmed higher-low in the volume phase (minimal lag), filling worst-case.
//!
//! Sequence (all causal, so backtest == live):
//!   1. Scan the raw alternating leg ledger ([`detect_swing_legs_raw`]).
//!   2. [`classify_phase`] → the volume-phase latch (kill→volume transition).
//!   3. If latched, find the first higher-low confirmation
//!      ([`higher_low_confirmed_index`], reused from tpsl2) on trades **after**
//!      the latch leg, within the optional `p_entry_max_age_secs` window.
//!   4. Resolve the worst-case paper fill at that trigger index
//!      ([`find_worst_case_paper_entry_at`], reused from tpsl2).
//!
//! [`detect_swing_legs_raw`]: super::swing::detect_swing_legs_raw
//! [`classify_phase`]: super::classifier::classify_phase
//! [`higher_low_confirmed_index`]:
//!   crate::strategies::tpsl_sniper_2::entry::higher_low_confirmed_index
//! [`find_worst_case_paper_entry_at`]:
//!   crate::strategies::tpsl_sniper_2::entry::find_worst_case_paper_entry_at

use chrono::Duration;

use crate::models::trade::TradeRow;
use crate::models::{Swing1Rule, Token};
use crate::strategies::tpsl_sniper_2::entry::{higher_low_confirmed_index, EntryFill};

use super::swing::detect_swing_legs_raw;
use super::{classifier, phase_profile_from_rule, rule_configures_any_entry_gate, swing_params_from_rule};

/// swing1 has **no** token-creation gate — the dev-fingerprint filter is applied
/// upstream and the strategy decides entirely from the trade stream. Always
/// `true` (kept for registry symmetry with tpsl1/tpsl2's `matches_entry`).
pub fn token_matches_buy_rule(_token: &Token, _rule: &Swing1Rule) -> bool {
    true
}

/// The resolved entry: the trigger trade's **index** in `trades` plus its
/// worst-case paper fill. `None` when the token never transitions to the volume
/// phase, never confirms a higher-low in the entry window, or the rule configures
/// no entry gate.
pub fn find_phase_entry<T: TradeRow>(
    trades: &[T],
    rule: &Swing1Rule,
) -> Option<(usize, crate::strategies::tpsl_sniper_2::entry::EntryFill)> {
    if !rule_configures_any_entry_gate(rule) || trades.is_empty() {
        return None;
    }

    // 1–2. Detect the swing chain and latch the kill→volume transition.
    let sparams = swing_params_from_rule(rule);
    let profile = phase_profile_from_rule(rule);
    let legs = detect_swing_legs_raw(trades, &sparams);
    let latch = classifier::classify_phase(&legs, &profile);
    if !latch.volume_phase_latched {
        return None;
    }
    let latch_leg_idx = latch.latched_leg_index?;
    // Entry confirmation may only begin at/after the latching low's terminal pivot.
    let latch_end_ms = legs[latch_leg_idx].end_at;

    // The entry-window ceiling: confirmation must land within `max_age` secs of the
    // latch (None/0 ⇒ arm until the token dies, matching live's bounded armer).
    let max_age_secs = rule.p_entry_max_age_secs.filter(|&s| s != 0);
    let pullback = rule.p_entry_pullback_pct.filter(|&p| p != 0.0)?;
    let hl_secs = rule.p_entry_higher_low_secs.unwrap_or(0) as i64;

    // 3. Restrict the higher-low search to trades after the latch. `higher_low_
    //    confirmed_index` returns an index into the slice it is given, so offset it
    //    back to the full-slice index.
    let start_idx = trades
        .iter()
        .position(|t| t.block_time().timestamp_millis() >= latch_end_ms)
        .unwrap_or(trades.len());
    if start_idx >= trades.len() {
        return None;
    }
    let post = &trades[start_idx..];
    let hl_local = higher_low_confirmed_index(post, pullback, hl_secs)?;
    let trigger_idx = start_idx + hl_local;

    // Enforce the entry-window ceiling against the latch time.
    if let Some(max_age) = max_age_secs {
        let latch_time = chrono::DateTime::from_timestamp_millis(latch_end_ms)?;
        if trades[trigger_idx].block_time() > latch_time + Duration::seconds(max_age as i64) {
            return None;
        }
    }

    // 4. Worst-case paper fill at the trigger, priced off the **canonical GMGN
    //    spot** (`chart_spot_price`), not execution `price_per_token`. swing1 is
    //    validated against the Parquet lake, whose `SweepTrade` rows carry the
    //    reserve pair but a **zeroed `price_per_token`** — tpsl2's shared
    //    `find_worst_case_paper_entry_at` reads `price_per_token` and so returns a
    //    ~0 fill there, which the sweep then discards (zero fires). Pricing the fill
    //    off the same spot the leg detector uses keeps live/sweep parity (Step 0).
    let fill = find_worst_case_spot_entry_at(trades, trigger_idx)?;
    if fill.price <= 0.0 {
        return None;
    }
    Some((trigger_idx, fill))
}

/// Worst-case paper entry fill at `target_idx`, priced by the shared
/// [`chart_spot_price`](TradeRow::chart_spot_price) (reserve pair → pool → exec
/// fallback) instead of execution `price_per_token`. Mirrors tpsl2's
/// `find_worst_case_paper_entry_at` slot-window selection exactly — the trigger
/// slot plus the first post-trigger buy slot if within `MAX_FILL_WAIT_SLOTS` — but
/// every price read (the candidate filter AND the worst-case max) goes through the
/// canonical spot so the fill matches the leg-detector / live price. "Worst case" =
/// the highest spot among the qualifying buys in the fill window.
fn find_worst_case_spot_entry_at<T: TradeRow>(
    trades: &[T],
    target_idx: usize,
) -> Option<EntryFill> {
    use crate::config::constants::MAX_FILL_WAIT_SLOTS;
    use crate::models::trade::Trade;

    let trigger_slot = trades[target_idx].slot();
    let post = trades.get(target_idx + 1..).unwrap_or(&[]);

    // Canonical spot, gating on a finite positive price.
    let spot = |t: &T| t.chart_spot_price().filter(|p| p.is_finite() && *p > 0.0);

    // First slot > trigger with a qualifying buy — only for the proximity check.
    let next_slot = post
        .iter()
        .filter(|t| t.slot() > trigger_slot && t.is_buy() && spot(t).is_some() && !Trade::is_dust(t.sol_amount()))
        .map(|t| t.slot())
        .next();

    // Fill window: trigger slot (always) + next_slot if close enough.
    let best = post
        .iter()
        .filter(|t| {
            let s = t.slot();
            let in_s = s == trigger_slot;
            let in_next = next_slot.is_some_and(|ns| s == ns && ns <= trigger_slot + MAX_FILL_WAIT_SLOTS);
            (in_s || in_next) && t.is_buy() && spot(t).is_some() && !Trade::is_dust(t.sol_amount())
        })
        .max_by(|a, b| spot(a).unwrap().total_cmp(&spot(b).unwrap()))?;

    Some(EntryFill {
        price: spot(best).unwrap(),
        amount_tokens: best.token_amount(),
        tx_signature: best.tx_signature().to_string(),
        block_time: best.block_time(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;

    fn t(kind: TradeType, ms: i64, sol: f64, tokens: u64, vsol: f64, vtok: u64, slot: u64) -> Trade {
        let mut tr = Trade::new(
            "mint".into(),
            "w".into(),
            kind,
            sol,
            tokens,
            format!("sig-{ms}"),
            slot,
            Utc::now(),
        );
        tr.block_time = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        tr.reserve_sol = Some(vsol);
        tr.reserve_token = Some(vtok);
        tr
    }

    /// A rule with no entry gate never enters.
    #[test]
    fn no_entry_gate_resolves_none() {
        let rule = Swing1Rule::new(
            "r".into(), None, None, None, serde_json::json!([]), "paper".into(),
            1.0, 50.0, 20.0, None, None, None, None, None, None, None, None, None,
        );
        let trades = vec![t(TradeType::Buy, 0, 1.0, 1_000, 30.0, 1_000_000, 1)];
        assert!(find_phase_entry(&trades, &rule).is_none());
    }
}
