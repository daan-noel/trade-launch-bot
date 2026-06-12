//! Entry — the single place that decides **whether a new token is bought** and
//! **at what fill**.
//!
//! Two concerns:
//!   • criteria matching ([`token_matches_buy_rule`] / [`find_first_matching_buy_rule`])
//!     — does a token satisfy a rule's buy filters?
//!   • fill resolution ([`find_entry_fill_in_trades`]) — at what price/tx/time
//!     did we actually enter?
//!
//! **To add an entry criterion:** write a `check_*` function returning a
//! [`CriterionOutcome`] and add it to the [`CRITERIA`] list. Every configured
//! criterion must be satisfied for a match, and a rule that configures *no*
//! criterion never matches (so a misconfigured rule can't buy every token).

use chrono::{DateTime, Utc};
use tracing::warn;
use uuid::Uuid;

use super::util::{none_if_zero_f64, none_if_zero_u64};
use crate::models::trade::{Trade, TradeType};
use crate::models::{Tpsl1Rule, Token};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// The result of testing one entry criterion against a token.
enum CriterionOutcome {
    /// The rule doesn't set this criterion (param None/zero) — inert, ignored.
    NotConfigured,
    /// Configured and the token satisfies it.
    Satisfied,
    /// Configured and the token fails it — the rule cannot match this token.
    Rejected,
}

/// Every entry criterion, evaluated in order. Adding a filter = add its
/// `check_*` here; nothing else changes.
const CRITERIA: &[fn(&Token, &Tpsl1Rule) -> CriterionOutcome] = &[
    check_initial_buy_sol,
    check_compute_unit_limit,
    check_compute_unit_price,
    check_max_sol_cost,
    check_spendable_sol_in,
    check_instruction_labels,
];

/// Whether a token satisfies a rule's buy criteria. A rule must configure at
/// least one criterion, and every configured criterion must be satisfied.
/// Shared by the live entry gate and the backtest.
pub fn token_matches_buy_rule(token: &Token, rule: &Tpsl1Rule) -> bool {
    let mut any_configured = false;
    for check in CRITERIA {
        match check(token, rule) {
            CriterionOutcome::NotConfigured => {}
            CriterionOutcome::Satisfied => any_configured = true,
            CriterionOutcome::Rejected => return false,
        }
    }
    any_configured
}

/// The first active rule whose criteria the token satisfies, or `None`. A rule
/// that configures no criterion is skipped with a warning rather than matching
/// every token.
pub fn find_first_matching_buy_rule(token: &Token, rules: &[Tpsl1Rule]) -> Option<Uuid> {
    for rule in rules {
        if !rule.is_active {
            continue;
        }
        if !rule_configures_any_criterion(rule) {
            warn!(
                "Rule {} has no entry criteria — skipping (would otherwise match every token)",
                rule.id
            );
            continue;
        }
        if token_matches_buy_rule(token, rule) {
            return Some(rule.id);
        }
    }
    None
}

/// Whether a rule sets at least one entry criterion (used to skip — and warn
/// about — a misconfigured, match-everything rule).
fn rule_configures_any_criterion(rule: &Tpsl1Rule) -> bool {
    none_if_zero_f64(rule.p_token_initial_buy_sol).is_some()
        || none_if_zero_u64(rule.p_token_cu_limit).is_some()
        || none_if_zero_u64(rule.p_token_cu_price).is_some()
        || none_if_zero_f64(rule.p_token_max_sol_cost).is_some()
        || none_if_zero_f64(rule.p_token_spendable_sol_in).is_some()
        || rule.p_token_ix_labels.as_array().map_or(false, |a| !a.is_empty())
}

// ── Criteria ─────────────────────────────────────────────────────────────────
// Each reads its rule param (None/zero ⇒ NotConfigured), then compares the
// token's value within the rule's `tolerance_pct` band. `eps` absorbs float
// rounding and is preserved per-criterion from the original matcher.

/// True when `token_val` is within the rule's tolerance band around `rule_val`.
fn within_tolerance(token_val: f64, rule_val: f64, tolerance_pct: f64, eps: f64) -> bool {
    let tol = rule_val.abs() * (tolerance_pct * 0.01);
    (token_val - rule_val).abs() <= tol + eps
}

fn check_initial_buy_sol(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_initial_buy_sol) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.initial_buy_sol {
        Some(v) if within_tolerance(v, rule_val, rule.tolerance_pct, 1e-9) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_compute_unit_limit(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_u64(rule.p_token_cu_limit) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.cu_limit {
        Some(v) if within_tolerance(v as f64, rule_val as f64, rule.tolerance_pct, 1e-15) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_compute_unit_price(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_u64(rule.p_token_cu_price) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.cu_price {
        Some(v) if within_tolerance(v as f64, rule_val as f64, rule.tolerance_pct, 1e-9) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_max_sol_cost(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_max_sol_cost) else {
        return CriterionOutcome::NotConfigured;
    };
    match instruction_arg_as_sol(token, "max_sol_cost") {
        Some(sol) if within_tolerance(sol, rule_val, rule.tolerance_pct, 1e-15) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_spendable_sol_in(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_spendable_sol_in) else {
        return CriterionOutcome::NotConfigured;
    };
    match instruction_arg_as_sol(token, "spendable_sol_in") {
        Some(sol) if within_tolerance(sol, rule_val, rule.tolerance_pct, 1e-15) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_instruction_labels(token: &Token, rule: &Tpsl1Rule) -> CriterionOutcome {
    if !rule.p_token_ix_labels.as_array().map_or(false, |a| !a.is_empty()) {
        return CriterionOutcome::NotConfigured;
    }
    // Presence check only for now (TODO: per-label matching).
    if token.instruction_labels.as_array().map_or(false, |a| !a.is_empty()) {
        CriterionOutcome::Satisfied
    } else {
        CriterionOutcome::Rejected
    }
}

/// Read a lamports field from the token's creation-instruction args (accepting a
/// number or a numeric string) and convert it to SOL.
fn instruction_arg_as_sol(token: &Token, key: &str) -> Option<f64> {
    let lamports = token.initial_buy_instruction.as_ref()?.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    })?;
    Some(lamports as f64 / LAMPORTS_PER_SOL)
}

// ── Fill resolution ──────────────────────────────────────────────────────────

/// The resolved entry: the price/tx/time of the trade the position enters on.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryFill {
    pub price: f64,
    pub tx_signature: String,
    pub block_time: DateTime<Utc>,
}

/// Resolve the entry fill from a token's trade history: the highest-priced buy
/// in the first slot block plus the first `second_block_cap` buys of the second
/// block. Shared by the backtest (cap 1) and the live paper entry poll (cap 5).
pub fn find_entry_fill_in_trades(trades: &[Trade], second_block_cap: usize) -> Option<EntryFill> {
    if trades.is_empty() {
        return None;
    }

    let first_slot = trades[0].slot;
    let second_slot = trades.iter().find(|t| t.slot > first_slot).map(|t| t.slot);

    let mut candidates: Vec<&Trade> = Vec::new();
    for t in trades.iter() {
        if t.trade_type != TradeType::Buy {
            continue;
        }
        if t.slot == first_slot {
            candidates.push(t);
        } else if let Some(second_slot) = second_slot {
            if t.slot == second_slot {
                let already = candidates.iter().filter(|c| c.slot == second_slot).count();
                if already < second_block_cap {
                    candidates.push(t);
                }
            }
        }
    }

    candidates
        .into_iter()
        .max_by(|a, b| {
            a.price_per_token
                .partial_cmp(&b.price_per_token)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|t| EntryFill {
            price: t.price_per_token,
            tx_signature: t.tx_signature.clone(),
            block_time: t.block_time,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::TradeType;
    use serde_json::{json, Value};

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn token_with(
        initial_buy_sol: Option<f64>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        initial_buy_instruction: Option<Value>,
        instruction_labels: Value,
    ) -> Token {
        Token::new(
            "mint".into(),
            "creator".into(),
            "name".into(),
            "SYM".into(),
            None,
            None,
            None,
            initial_buy_sol,
            initial_buy_instruction,
            cu_limit,
            cu_price,
            false,
            false,
            instruction_labels,
            "create-sig".into(),
            base_time(),
        )
    }

    /// An active rule with the given entry criteria; exit params inert.
    fn rule_with_entry(
        p_token_initial_buy_sol: Option<f64>,
        p_token_cu_limit: Option<u64>,
        p_token_cu_price: Option<u64>,
        p_token_ix_labels: Value,
        p_token_max_sol_cost: Option<f64>,
        p_token_spendable_sol_in: Option<f64>,
        tolerance_pct: f64,
    ) -> Tpsl1Rule {
        let mut r = Tpsl1Rule::new(
            "test".into(),
            p_token_initial_buy_sol,
            p_token_cu_limit,
            p_token_cu_price,
            p_token_ix_labels,
            "paper".into(),
            1.0,
            50.0,
            20.0,
            p_token_max_sol_cost,
            p_token_spendable_sol_in,
            None,
            None,
            Some(tolerance_pct),
            None,
            None,
            None,
            None,
        );
        r.is_active = true;
        r
    }

    fn buy(price: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            price,
            1.0,
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + chrono::Duration::seconds(secs),
        )
    }

    #[test]
    fn matches_initial_buy_sol_within_tolerance() {
        let rule = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        let near = token_with(Some(1.05), None, None, None, json!([]));
        let far = token_with(Some(1.2), None, None, None, json!([]));
        assert!(token_matches_buy_rule(&near, &rule));
        assert!(!token_matches_buy_rule(&far, &rule));
    }

    #[test]
    fn rejects_when_token_field_missing() {
        let rule = rule_with_entry(None, Some(100_000), None, json!([]), None, None, 10.0);
        let token = token_with(Some(1.0), None, None, None, json!([])); // cu_limit None
        assert!(!token_matches_buy_rule(&token, &rule));
    }

    #[test]
    fn max_sol_cost_read_from_instruction_args() {
        let rule = rule_with_entry(None, None, None, json!([]), Some(1.0), None, 0.0);
        let ix = json!({ "max_sol_cost": 1_000_000_000u64 }); // 1 SOL
        let token = token_with(None, None, None, Some(ix), json!([]));
        assert!(token_matches_buy_rule(&token, &rule));
    }

    #[test]
    fn instruction_labels_require_presence() {
        let rule = rule_with_entry(None, None, None, json!(["create"]), None, None, 0.0);
        let with_labels = token_with(None, None, None, None, json!(["create"]));
        let without = token_with(None, None, None, None, json!([]));
        assert!(token_matches_buy_rule(&with_labels, &rule));
        assert!(!token_matches_buy_rule(&without, &rule));
    }

    // The degenerate-case fix: a rule with only a tolerance and no real criteria
    // must NOT match every token (the old inline check_buy_entry did).
    #[test]
    fn rule_with_no_criteria_never_matches() {
        let rule = rule_with_entry(None, None, None, json!([]), None, None, 10.0);
        let token = token_with(Some(1.0), Some(100_000), None, None, json!([]));
        assert!(!token_matches_buy_rule(&token, &rule));
        assert_eq!(
            find_first_matching_buy_rule(&token, std::slice::from_ref(&rule)),
            None
        );
    }

    #[test]
    fn find_first_matching_skips_inactive_and_returns_first_match() {
        let token = token_with(Some(1.0), None, None, None, json!([]));
        let mut inactive = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        inactive.is_active = false;
        let active = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);

        let rules = vec![inactive, active.clone()];
        assert_eq!(find_first_matching_buy_rule(&token, &rules), Some(active.id));
    }

    #[test]
    fn entry_fill_picks_highest_among_admitted_buys() {
        let trades = vec![buy(1.0, 5, 1), buy(2.0, 5, 1), buy(9.0, 6, 2)];
        let fill = find_entry_fill_in_trades(&trades, 1).expect("entry fill");
        // First block (slot 5) plus one admitted second-block buy (slot 6, cap 1);
        // the highest price across them wins — here the 9.0 second-block trade.
        assert_eq!(fill.price, 9.0);
    }
}
