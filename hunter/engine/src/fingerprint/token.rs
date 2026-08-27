//! [`TokenFingerprint`] — one token's **observed** creation axes, the input every
//! fingerprint is graded against.
//!
//! Built once per token at `TokenCreated` (`trading_core::strategies::fingerprint_axes`
//! is the one builder, shared by the live producer and the lab replay driver) and
//! completed at `FirstSlotSettled`. Carried on the token state, so a match is a
//! pure in-memory read.
//!
//! **Every numeric axis is an integer.** Amounts are lamports (`u64`, the on-chain
//! domain, never narrowed), counts are tallies. The conversion from a human-`f64`
//! SOL column happens once, in the builder at the repo boundary — never at match
//! time, where a float would reintroduce the rounding two implementations have to
//! agree on.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One token's observed creation axes. `None` = the token does not carry that
/// value, which **fails** any configured axis: an unknown value can never be shown
/// to satisfy a bound, and failing closed is the only direction that cannot arm a
/// rule on a token nobody screened.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenFingerprint {
    /// SPL token program that minted it. Grouping-only — no fingerprint axis.
    pub token_program_id: Option<String>,
    /// Grouping-only — no fingerprint axis.
    #[serde(default)]
    pub is_cashback_enabled: bool,
    /// Compute-unit limit of the creation transaction.
    pub cu_limit: Option<u64>,
    /// Compute-unit price of the creation transaction.
    pub cu_price: Option<u64>,
    /// Creator's own first buy, in lamports.
    pub init_buy_lamports: Option<u64>,
    /// Creation-instruction `max_sol_cost`, in lamports. Carries the `u64::MAX`
    /// "fill at any price" sentinel exactly — it is a real launch setting, and
    /// narrowing it is what once made one value read as `-1` here and `1.84e19`
    /// on the dashboard.
    pub max_cost_lamports: Option<u64>,
    /// Creation-instruction `spendable_lamports_in`, in lamports.
    pub spendable_lamports_in: Option<u64>,
    /// Buy lamports summed across the creation slot. `None` until that slot
    /// settles — the reason matching is two-phase.
    pub first_slot_buy_lamports: Option<u64>,
    /// Sell lamports summed across the creation slot. `None` as above.
    pub first_slot_sell_lamports: Option<u64>,
    /// Creation instruction labels, exact on-chain order and count.
    #[serde(default)]
    pub ix_labels: Vec<String>,
    /// How many tokens this creator launched before this one.
    ///
    /// **A stateful engine tally, not a token column** — `reduce` stamps it here at
    /// `TokenCreated`, from the running per-creator count, before the match runs.
    /// `None` when the creation event carries no creator wallet, so a configured
    /// axis fails rather than reading an unknown creator as a first-time launcher.
    #[serde(default)]
    pub prior_launches: Option<u32>,
}

/// Read a lamports value from a creation instruction-args object. **The one decode
/// seam** for an on-chain `u64` arg.
///
/// Returns `u64`, the on-chain domain, and never narrows: pump.fun's `buy`/`buy_v2`
/// take `max_sol_cost` as a slippage ceiling, so "fill at any price" is spelled
/// `u64::MAX` — an amount no `i64` can hold. Narrowing it wrapped the value to `-1`
/// on every Rust reader while the dashboard's SQL read the same row as `+1.84e19`.
///
/// **Both persisted shapes are accepted** — a JSON number (what ingest writes) and
/// a numeric string — because JSON numbers are unsafe past 2^53 for any consumer
/// that parses them as `f64`, so the string form is a legitimate storage encoding.
pub fn extract_lamports(instruction: Option<&Value>, key: &str) -> Option<u64> {
    let obj = instruction?;
    obj.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok())))
}

/// Human SOL → lamports, **rounded**. Junk (`NaN`, infinity, negative) is `0`.
///
/// The engine's own copy of `trading_core::config::constants::sol_to_lamports`
/// (this crate is pure — no `config`), locked equal by a guard test over there.
/// Rounding, not truncation, is what makes a lamports → SOL → lamports round-trip
/// exact, so a bound typed from a displayed amount recovers the stored integer.
///
/// This is the **only** float→integer seam on the fingerprint path, and it runs at
/// the repo boundary, never at match time.
///
/// Junk never lands on `u64::MAX`: that value is the "fill at any price" sentinel a
/// real launch sets, so a parse failure resolving to it would manufacture a
/// meaningful setting out of a typo. It collapses to `0` instead.
pub fn sol_to_lamports(sol: f64) -> u64 {
    if !sol.is_finite() || sol <= 0.0 {
        return 0;
    }
    let v = (sol * 1_000_000_000.0).round();
    if v >= u64::MAX as f64 {
        0
    } else {
        v as u64
    }
}

/// Lamports → human SOL, for display only.
pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / 1_000_000_000.0
}

/// Collect an instruction-labels JSON value into a `Vec<String>`, preserving exact
/// on-chain order and count — no dedup, so tokens differing only in how many times
/// an instruction repeats stay distinguishable. Accepts both persisted shapes:
/// bare `["A","B"]` and `{ "instructions": ["A","B"] }`.
pub fn normalize_labels(labels: &Value) -> Vec<String> {
    let arr = match labels {
        Value::Array(a) => a.as_slice(),
        Value::Object(o) => match o.get("instructions") {
            Some(Value::Array(a)) => a.as_slice(),
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_ceiling_arg_decodes_intact_from_both_wire_shapes() {
        let as_num = json!({ "max_cost_lamports": u64::MAX });
        let as_str = json!({ "max_cost_lamports": u64::MAX.to_string() });
        assert_eq!(extract_lamports(Some(&as_num), "max_cost_lamports"), Some(u64::MAX));
        assert_eq!(extract_lamports(Some(&as_str), "max_cost_lamports"), Some(u64::MAX));
        assert_eq!(extract_lamports(Some(&as_num), "absent"), None);
        assert_eq!(extract_lamports(None, "max_cost_lamports"), None);
    }

    #[test]
    fn sol_round_trips_through_lamports() {
        for sol in [0.0, 0.000000001, 0.108, 1.515, 15.15, 1234.5] {
            assert_eq!(lamports_to_sol(sol_to_lamports(sol)), sol);
        }
        // Junk collapses to 0, never to `u64::MAX` — that value is the "fill at any
        // price" SENTINEL, and a parse failure landing on it would manufacture a
        // meaningful launch setting out of a typo.
        assert_eq!(sol_to_lamports(f64::NAN), 0);
        assert_eq!(sol_to_lamports(-1.0), 0);
        assert_eq!(sol_to_lamports(f64::INFINITY), 0);
    }

    #[test]
    fn labels_accept_both_shapes_and_keep_repeats() {
        assert_eq!(normalize_labels(&json!(["A", "B", "B"])), vec!["A", "B", "B"]);
        assert_eq!(normalize_labels(&json!({ "instructions": ["A", "B"] })), vec!["A", "B"]);
        assert!(normalize_labels(&json!({})).is_empty());
        assert!(normalize_labels(&json!(7)).is_empty());
    }
}
