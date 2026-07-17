//! Cache keys for the lab matched/simulate analysis paths.
//!
//! [`fingerprint_key`] covers the token-creation filters shared by the matched
//! table and the backtest candidate scan (Stage‑1 fingerprint only — scalp /
//! swing entry gates and exit knobs are excluded). [`sim_key`] covers every
//! knob that affects a backtest's per-token walk (params JSONB + universal
//! sizing columns). Mirrors the frontend `fingerprintKey()` in
//! `ruleColorGroups.ts` — kept in sync by `fingerprint_key_parity_tests`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;

use crate::models::LegacyStrategyRule;
use crate::strategies::registry::{StrategyImpl, StrategyParams};

/// Stable string for the token-creation fingerprint (Stage‑1 filters only).
/// Two rules with the same fingerprint over the same analysis window share one
/// whole-table scan + one lake load.
pub fn fingerprint_key(rule: &LegacyStrategyRule) -> String {
    let Some(strategy) = StrategyImpl::from_id(&rule.strategy_id) else {
        return hash_label("unknown");
    };
    let Ok(params) = strategy.parse_params(&rule.params) else {
        return hash_label("invalid");
    };
    let canonical = fingerprint_canonical(&params);
    hash_label(&format!("{}|{}", strategy.id(), canonical))
}

/// Stable string for a full backtest configuration (fingerprint + entry/exit/
/// sizing). Unchanged → the finished sim result can be reused without re-walking
/// trades.
pub fn sim_key(rule: &LegacyStrategyRule) -> String {
    let mut h = DefaultHasher::new();
    rule.strategy_id.hash(&mut h);
    rule.buy_amount_sol.to_bits().hash(&mut h);
    rule.max_concurrent_tokens.hash(&mut h);
    rule.max_total_tokens.hash(&mut h);
    // Params JSON as stored — every gate the decision layer reads lives here or
    // in the universal columns hashed above.
    rule.params.to_string().hash(&mut h);
    format!("sim_{:016x}", h.finish())
}

fn hash_label(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("fp_{:016x}", h.finish())
}

/// Pipe-joined fingerprint fields — must stay aligned with the frontend
/// `fingerprintKey()` in `ruleColorGroups.ts`.
fn fingerprint_canonical(params: &StrategyParams) -> String {
    let (
        initial_buy,
        first_slot_buy,
        first_slot_sell,
        cu_limit,
        cu_price,
        max_sol,
        spendable,
        labels,
        bucket_width,
    ) = match params {
        StrategyParams::Tpsl1(p) => (
            opt_f64(p.p_token_initial_buy_sol),
            opt_f64(p.p_token_first_slot_buy_sol),
            opt_f64(p.p_token_first_slot_sell_sol),
            opt_u64(p.p_token_cu_limit),
            opt_u64(p.p_token_cu_price),
            opt_f64(p.p_token_max_sol_cost),
            opt_f64(p.p_token_spendable_sol_in),
            canonical_ix_labels(&p.p_token_ix_labels),
            p.bucket_width_sol.to_string(),
        ),
        StrategyParams::Tpsl2(p) => (
            opt_f64(p.p_token_initial_buy_sol),
            opt_f64(p.p_token_first_slot_buy_sol),
            opt_f64(p.p_token_first_slot_sell_sol),
            opt_u64(p.p_token_cu_limit),
            opt_u64(p.p_token_cu_price),
            opt_f64(p.p_token_max_sol_cost),
            opt_f64(p.p_token_spendable_sol_in),
            canonical_ix_labels(&p.p_token_ix_labels),
            p.bucket_width_sol.to_string(),
        ),
        StrategyParams::Swing1(p) => (
            opt_f64(p.p_token_initial_buy_sol),
            opt_f64(p.p_token_first_slot_buy_sol),
            opt_f64(p.p_token_first_slot_sell_sol),
            opt_u64(p.p_token_cu_limit),
            opt_u64(p.p_token_cu_price),
            opt_f64(p.p_token_max_sol_cost),
            opt_f64(p.p_token_spendable_sol_in),
            canonical_ix_labels(&p.p_token_ix_labels),
            p.bucket_width_sol.to_string(),
        ),
    };
    [
        initial_buy,
        first_slot_buy,
        first_slot_sell,
        cu_limit,
        cu_price,
        max_sol,
        spendable,
        labels,
        bucket_width,
    ]
    .join("|")
}

fn opt_f64(v: Option<f64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

fn opt_u64(v: Option<u64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

fn canonical_ix_labels(v: &Value) -> String {
    let mut labels: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    labels.sort_unstable();
    labels.join(",")
}

#[cfg(test)]
mod fingerprint_key_parity_tests {
    use super::*;
    use crate::strategies::registry::Tpsl1Params;
    use serde_json::json;

    /// Golden vector mirroring `fingerprintKey()` in `ruleColorGroups.ts`.
    fn ts_fingerprint_key(
        initial_buy: Option<f64>,
        first_slot_buy: Option<f64>,
        first_slot_sell: Option<f64>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        max_sol: Option<f64>,
        spendable: Option<f64>,
        labels: &[&str],
        bucket_width: f64,
    ) -> String {
        let mut sorted: Vec<&str> = labels.to_vec();
        sorted.sort_unstable();
        [
            initial_buy.map(|v| v.to_string()).unwrap_or_default(),
            first_slot_buy.map(|v| v.to_string()).unwrap_or_default(),
            first_slot_sell.map(|v| v.to_string()).unwrap_or_default(),
            cu_limit.map(|v| v.to_string()).unwrap_or_default(),
            cu_price.map(|v| v.to_string()).unwrap_or_default(),
            max_sol.map(|v| v.to_string()).unwrap_or_default(),
            spendable.map(|v| v.to_string()).unwrap_or_default(),
            sorted.join(","),
            bucket_width.to_string(),
        ]
        .join("|")
    }

    #[test]
    fn fingerprint_canonical_matches_frontend_join() {
        let params = StrategyParams::Tpsl1(Tpsl1Params {
            p_token_initial_buy_sol: Some(1.05),
            p_token_cu_limit: Some(100_000),
            p_token_cu_price: Some(500),
            p_token_max_sol_cost: None,
            p_token_spendable_sol_in: Some(0.5),
            p_token_first_slot_buy_sol: Some(0.1),
            p_token_first_slot_sell_sol: None,
            p_token_ix_labels: json!(["B", "A"]),
            bucket_width_sol: 0.1,
            p_exit_take_profit: 50.0,
            p_exit_stop_loss: 20.0,
            p_exit_trailing_stop_pct: None,
            p_exit_time_stop_secs: None,
            p_exit_stall_secs: None,
            p_exit_liquidity_drop_pct: None,
        });
        let canonical = fingerprint_canonical(&params);
        let expected = ts_fingerprint_key(
            Some(1.05),
            Some(0.1),
            None,
            Some(100_000),
            Some(500),
            None,
            Some(0.5),
            &["B", "A"],
            0.1,
        );
        assert_eq!(canonical, expected);
    }

    #[test]
    fn sim_key_changes_on_exit_not_fingerprint() {
        use chrono::Utc;
        use uuid::Uuid;

        let mut base = LegacyStrategyRule {
            id: Uuid::new_v4(),
            strategy_id: "tpsl_sniper_1".into(),
            rule_name: "r".into(),
            buy_amount_sol: 0.1,
            trade_mode: "paper".into(),
            is_active: true,
            max_concurrent_tokens: None,
            max_total_tokens: None,
            params: json!({
                "p_token_initial_buy_sol": 1.0,
                "bucket_width_sol": 0.1,
                "p_exit_take_profit": 50.0,
                "p_exit_stop_loss": 20.0,
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let fp1 = fingerprint_key(&base);
        let sim1 = sim_key(&base);

        base.params["p_exit_take_profit"] = json!(80.0);
        assert_eq!(fingerprint_key(&base), fp1);
        assert_ne!(sim_key(&base), sim1);

        base.params["p_token_initial_buy_sol"] = json!(2.0);
        assert_ne!(fingerprint_key(&base), fp1);
    }
}
