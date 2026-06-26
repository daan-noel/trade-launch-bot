//! Domain core for the TPSL rule families (the **core** layer of the crate-split
//! plan). These helpers own request→model mapping, percent/criteria validation,
//! and the rule-repo write — and nothing else. They touch ONLY the rule repo +
//! validation: never the live runtime cache (`tpslN_cache`), the SSE bus, or RPC.
//!
//! That keeps the same domain logic usable by both runtime edges: the deploy
//! handler appends a cache reload + `emit_rules_changed`; the local handler
//! appends nothing. The CRUD handler shrinks to a thin wrapper over these.
//!
//! tpsl1 and tpsl2 carry distinct rule/request types, so each gets its own set
//! (intentional clones — a fix in one usually belongs in both).

use uuid::Uuid;

/// Outcome of a rule CRUD write, mapped to an HTTP status by the calling edge.
/// The domain core never references actix, so deploy and local handlers render
/// (and log) this identically.
pub enum RuleWriteError {
    /// 400 — a validation / criteria check rejected the (proposed) rule.
    Invalid(String),
    /// 500 — a repository call failed; carries the error for logging.
    Repo(anyhow::Error),
}

// ===========================================================================
// tpsl1
// ===========================================================================

pub mod tpsl1 {
    use super::*;
    use crate::api::handlers::strategies::tpsl1::{CreateRuleRequest, UpdateRuleRequest};
    use crate::models::Tpsl1Rule;
    use crate::storage::repositories::tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo;

    /// Reject percent params outside their valid range before a rule is persisted.
    /// Every percent field is whole-percent (see percent-params-unify-plan): Take
    /// Profit is unbounded above (only `> 0`); every other percent is clamped to
    /// `0–100` (`0` is the disable sentinel and stays legal). `Err(message)` on the
    /// first violation.
    pub fn validate_percent_ranges(rule: &Tpsl1Rule) -> Result<(), String> {
        if rule.p_exit_take_profit <= 0.0 {
            return Err("Take Profit % must be greater than 0".into());
        }
        let bounded: [(&str, f64); 4] = [
            ("Stop Loss %", rule.p_exit_stop_loss),
            ("Tolerance %", rule.tolerance_pct),
            ("Trailing Stop %", rule.p_exit_trailing_stop_pct.unwrap_or(0.0)),
            ("Liquidity Drop %", rule.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
        ];
        for (name, v) in bounded {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("{name} must be between 0 and 100"));
            }
        }
        Ok(())
    }

    /// Map a create request into a new (unpersisted) domain rule.
    pub fn build_rule(req: &CreateRuleRequest) -> Tpsl1Rule {
        Tpsl1Rule::new(
            req.rule_name.clone(),
            req.p_token_initial_buy_sol,
            req.p_token_cu_limit,
            req.p_token_cu_price,
            req.p_token_ix_labels.clone(),
            req.trade_mode.clone(),
            req.buy_amount,
            req.p_exit_take_profit,
            req.p_exit_stop_loss,
            req.p_token_max_sol_cost,
            req.p_token_spendable_sol_in,
            req.p_max_concurrent_tokens,
            req.p_max_total_tokens,
            req.tolerance_pct,
            req.p_exit_trailing_stop_pct,
            req.p_exit_time_stop_secs,
            req.p_exit_stall_secs,
            req.p_exit_liquidity_drop_pct,
        )
    }

    /// Apply a partial update onto an existing rule in place. Returns whether the
    /// `trade_mode` changed (the caller's cache-reload path differs for a mode
    /// flip). `is_active` is intentionally NOT applied — activation/pause is owned
    /// by the dedicated lifecycle endpoints so the paper-run side effects can't
    /// drift; this only edits rule fields.
    pub fn apply_update(rule: &mut Tpsl1Rule, req: &UpdateRuleRequest) -> bool {
        if let Some(name) = &req.rule_name {
            rule.rule_name = name.clone();
        }
        if let Some(buy_amount) = req.buy_amount {
            rule.buy_amount = buy_amount;
        }
        if let Some(p_exit_take_profit) = req.p_exit_take_profit {
            rule.p_exit_take_profit = p_exit_take_profit;
        }
        if let Some(p_exit_stop_loss) = req.p_exit_stop_loss {
            rule.p_exit_stop_loss = p_exit_stop_loss;
        }
        if let Some(trailing_stop_pct) = req.p_exit_trailing_stop_pct {
            rule.p_exit_trailing_stop_pct = Some(trailing_stop_pct);
        }
        if let Some(time_stop_secs) = req.p_exit_time_stop_secs {
            rule.p_exit_time_stop_secs = Some(time_stop_secs);
        }
        if let Some(stall_secs) = req.p_exit_stall_secs {
            rule.p_exit_stall_secs = Some(stall_secs);
        }
        if let Some(liquidity_drop_pct) = req.p_exit_liquidity_drop_pct {
            rule.p_exit_liquidity_drop_pct = Some(liquidity_drop_pct);
        }
        if let Some(initial_buy_sol_opt) = &req.p_token_initial_buy_sol {
            rule.p_token_initial_buy_sol = initial_buy_sol_opt.clone();
        }
        if let Some(cu_limit_opt) = &req.p_token_cu_limit {
            rule.p_token_cu_limit = cu_limit_opt.clone();
        }
        if let Some(cu_price_opt) = &req.p_token_cu_price {
            rule.p_token_cu_price = cu_price_opt.clone();
        }
        if let Some(ix_labels_opt) = &req.p_token_ix_labels {
            rule.p_token_ix_labels = ix_labels_opt
                .clone()
                .unwrap_or_else(|| serde_json::Value::Array(vec![]));
        }
        if let Some(max_sol_cost_opt) = &req.p_token_max_sol_cost {
            rule.p_token_max_sol_cost = max_sol_cost_opt.clone();
        }
        if let Some(spendable_sol_in_opt) = &req.p_token_spendable_sol_in {
            rule.p_token_spendable_sol_in = spendable_sol_in_opt.clone();
        }
        if let Some(max_concurrent_tokens_opt) = &req.p_max_concurrent_tokens {
            rule.p_max_concurrent_tokens = max_concurrent_tokens_opt.clone();
        }
        if let Some(max_total_tokens_opt) = &req.p_max_total_tokens {
            rule.p_max_total_tokens = max_total_tokens_opt.clone();
        }
        if let Some(tolerance_pct) = req.tolerance_pct {
            rule.tolerance_pct = tolerance_pct;
        }
        // Switching real<->paper changes which table this rule's stats come from,
        // so the caller must fully recompute the cached counters (a plain
        // `reload_rules` only swaps the rule list, leaving stale stats). Computed
        // before `trade_mode` is overwritten.
        let mode_changed = req
            .trade_mode
            .as_ref()
            .is_some_and(|m| *m != rule.trade_mode);
        if let Some(trade_mode) = &req.trade_mode {
            rule.trade_mode = trade_mode.clone();
        }
        mode_changed
    }

    /// Validate + persist a new rule. Returns the persisted rule (the caller
    /// builds the enriched response).
    pub async fn create(
        repo: &Tpsl1StrategyRuleRepo,
        req: &CreateRuleRequest,
    ) -> Result<Tpsl1Rule, RuleWriteError> {
        let rule = build_rule(req);
        validate_percent_ranges(&rule).map_err(RuleWriteError::Invalid)?;
        repo.insert(&rule).await.map_err(RuleWriteError::Repo)?;
        Ok(rule)
    }

    /// Apply a partial update onto an already-loaded rule, validate, and persist.
    /// Returns `(persisted_rule, mode_changed)`. The caller loads the rule first
    /// (and runs any cache-dependent live-freeze guard) before calling this.
    pub async fn apply_and_persist(
        repo: &Tpsl1StrategyRuleRepo,
        mut rule: Tpsl1Rule,
        req: &UpdateRuleRequest,
    ) -> Result<(Tpsl1Rule, bool), RuleWriteError> {
        let mode_changed = apply_update(&mut rule, req);
        validate_percent_ranges(&rule).map_err(RuleWriteError::Invalid)?;
        repo.update(&rule).await.map_err(RuleWriteError::Repo)?;
        Ok((rule, mode_changed))
    }

    /// Delete a rule. Idempotent — deleting a missing rule is not an error
    /// (mirrors the repo's `DELETE … WHERE id = $1`).
    pub async fn delete(
        repo: &Tpsl1StrategyRuleRepo,
        rule_id: Uuid,
    ) -> Result<(), RuleWriteError> {
        repo.delete(rule_id).await.map_err(RuleWriteError::Repo)
    }
}

// ===========================================================================
// tpsl2 (clone of tpsl1 + scalp-continuation gates + entry-window validation)
// ===========================================================================

pub mod tpsl2 {
    use super::*;
    use crate::api::handlers::strategies::tpsl2::{CreateRuleRequest, UpdateRuleRequest};
    use crate::models::Tpsl2Rule;
    use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;

    /// Reject percent params outside their valid range. Same model as tpsl1, plus
    /// the scalp Pullback %, Cohort Exit Ratio %, and Max Cohort Held %.
    pub fn validate_percent_ranges(rule: &Tpsl2Rule) -> Result<(), String> {
        if rule.p_exit_take_profit <= 0.0 {
            return Err("Take Profit % must be greater than 0".into());
        }
        let bounded: [(&str, f64); 6] = [
            ("Stop Loss %", rule.p_exit_stop_loss),
            ("Tolerance %", rule.tolerance_pct),
            ("Trailing Stop %", rule.p_exit_trailing_stop_pct.unwrap_or(0.0)),
            ("Liquidity Drop %", rule.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
            ("Pullback %", rule.p_entry_pullback_pct.unwrap_or(0.0)),
            ("Cohort Exit Ratio %", rule.p_exit_cohort_ratio.unwrap_or(0.0)),
        ];
        for (name, v) in bounded {
            if !(0.0..=100.0).contains(&v) {
                return Err(format!("{name} must be between 0 and 100"));
            }
        }
        // Max Cohort Held is bounded the same way but lives in its own field.
        let held = rule.p_entry_max_cohort_held.unwrap_or(0.0);
        if !(0.0..=100.0).contains(&held) {
            return Err("Max Cohort Held % must be between 0 and 100".into());
        }
        Ok(())
    }

    /// Reject an empty entry window: when both `p_entry_min_age_secs` (floor) and
    /// `p_entry_max_age_secs` (ceiling) are set, the ceiling must be strictly
    /// greater than the floor, else `[min_age, max_age]` admits nothing. `0`/`None`
    /// disables each bound (ignore_zero), so a one-sided window is always valid.
    pub fn validate_entry_window(rule: &Tpsl2Rule) -> Result<(), String> {
        let nz = |v: Option<u64>| v.filter(|&x| x != 0);
        if let (Some(min), Some(max)) =
            (nz(rule.p_entry_min_age_secs), nz(rule.p_entry_max_age_secs))
        {
            if max <= min {
                return Err(format!(
                    "Max Age ({max}s) must be greater than Min Age ({min}s)"
                ));
            }
        }
        Ok(())
    }

    /// A tpsl2 rule's only entry path is the scalp trade-window gates, so at least
    /// one must be configured. Runs in both create and update.
    fn validate_scalp_gate(rule: &Tpsl2Rule) -> Result<(), String> {
        if crate::strategies::tpsl_sniper_2::entry::rule_configures_any_scalp_gate(rule) {
            Ok(())
        } else {
            Err("Rule configures no scalp entry gate".into())
        }
    }

    /// Map a create request into a new (unpersisted) domain rule, including the
    /// scalp-continuation gates set post-`new()` (so the shared model's
    /// constructor signature — and tpsl1's call sites — stay untouched).
    pub fn build_rule(req: &CreateRuleRequest) -> Tpsl2Rule {
        let mut rule = Tpsl2Rule::new(
            req.rule_name.clone(),
            req.p_token_initial_buy_sol,
            req.p_token_cu_limit,
            req.p_token_cu_price,
            req.p_token_ix_labels.clone(),
            req.trade_mode.clone(),
            req.buy_amount,
            req.p_exit_take_profit,
            req.p_exit_stop_loss,
            req.p_token_max_sol_cost,
            req.p_token_spendable_sol_in,
            req.p_max_concurrent_tokens,
            req.p_max_total_tokens,
            req.tolerance_pct,
            req.p_exit_trailing_stop_pct,
            req.p_exit_time_stop_secs,
            req.p_exit_stall_secs,
            req.p_exit_liquidity_drop_pct,
        );
        rule.p_entry_min_age_secs = req.p_entry_min_age_secs;
        rule.p_entry_max_age_secs = req.p_entry_max_age_secs;
        rule.p_entry_min_alive_sol = req.p_entry_min_alive_sol;
        rule.p_entry_min_organic_sol = req.p_entry_min_organic_sol;
        rule.p_entry_pullback_pct = req.p_entry_pullback_pct;
        rule.p_entry_higher_low_secs = req.p_entry_higher_low_secs;
        rule.p_entry_max_cohort_held = req.p_entry_max_cohort_held;
        rule.p_entry_min_liquidity_sol = req.p_entry_min_liquidity_sol;
        rule.p_entry_min_organic_liq = req.p_entry_min_organic_liq;
        rule.p_exit_cohort_ratio = req.p_exit_cohort_ratio;
        rule
    }

    /// Apply a partial update onto an existing rule in place; returns whether the
    /// `trade_mode` changed. `is_active` is intentionally NOT applied (lifecycle
    /// endpoints own it).
    pub fn apply_update(rule: &mut Tpsl2Rule, req: &UpdateRuleRequest) -> bool {
        if let Some(name) = &req.rule_name {
            rule.rule_name = name.clone();
        }
        if let Some(buy_amount) = req.buy_amount {
            rule.buy_amount = buy_amount;
        }
        if let Some(p_exit_take_profit) = req.p_exit_take_profit {
            rule.p_exit_take_profit = p_exit_take_profit;
        }
        if let Some(p_exit_stop_loss) = req.p_exit_stop_loss {
            rule.p_exit_stop_loss = p_exit_stop_loss;
        }
        if let Some(trailing_stop_pct) = req.p_exit_trailing_stop_pct {
            rule.p_exit_trailing_stop_pct = Some(trailing_stop_pct);
        }
        if let Some(time_stop_secs) = req.p_exit_time_stop_secs {
            rule.p_exit_time_stop_secs = Some(time_stop_secs);
        }
        if let Some(stall_secs) = req.p_exit_stall_secs {
            rule.p_exit_stall_secs = Some(stall_secs);
        }
        if let Some(liquidity_drop_pct) = req.p_exit_liquidity_drop_pct {
            rule.p_exit_liquidity_drop_pct = Some(liquidity_drop_pct);
        }
        // Scalp-continuation gates (present → set; 0 disables).
        if let Some(v) = req.p_entry_min_age_secs {
            rule.p_entry_min_age_secs = Some(v);
        }
        if let Some(v) = req.p_entry_max_age_secs {
            rule.p_entry_max_age_secs = Some(v);
        }
        if let Some(v) = req.p_entry_min_alive_sol {
            rule.p_entry_min_alive_sol = Some(v);
        }
        if let Some(v) = req.p_entry_min_organic_sol {
            rule.p_entry_min_organic_sol = Some(v);
        }
        if let Some(v) = req.p_entry_pullback_pct {
            rule.p_entry_pullback_pct = Some(v);
        }
        if let Some(v) = req.p_entry_higher_low_secs {
            rule.p_entry_higher_low_secs = Some(v);
        }
        if let Some(v) = req.p_entry_max_cohort_held {
            rule.p_entry_max_cohort_held = Some(v);
        }
        if let Some(v) = req.p_entry_min_liquidity_sol {
            rule.p_entry_min_liquidity_sol = Some(v);
        }
        if let Some(v) = req.p_entry_min_organic_liq {
            rule.p_entry_min_organic_liq = Some(v);
        }
        if let Some(v) = req.p_exit_cohort_ratio {
            rule.p_exit_cohort_ratio = Some(v);
        }
        if let Some(initial_buy_sol_opt) = &req.p_token_initial_buy_sol {
            rule.p_token_initial_buy_sol = initial_buy_sol_opt.clone();
        }
        if let Some(cu_limit_opt) = &req.p_token_cu_limit {
            rule.p_token_cu_limit = cu_limit_opt.clone();
        }
        if let Some(cu_price_opt) = &req.p_token_cu_price {
            rule.p_token_cu_price = cu_price_opt.clone();
        }
        if let Some(ix_labels_opt) = &req.p_token_ix_labels {
            rule.p_token_ix_labels = ix_labels_opt
                .clone()
                .unwrap_or_else(|| serde_json::Value::Array(vec![]));
        }
        if let Some(max_sol_cost_opt) = &req.p_token_max_sol_cost {
            rule.p_token_max_sol_cost = max_sol_cost_opt.clone();
        }
        if let Some(spendable_sol_in_opt) = &req.p_token_spendable_sol_in {
            rule.p_token_spendable_sol_in = spendable_sol_in_opt.clone();
        }
        if let Some(max_concurrent_tokens_opt) = &req.p_max_concurrent_tokens {
            rule.p_max_concurrent_tokens = max_concurrent_tokens_opt.clone();
        }
        if let Some(max_total_tokens_opt) = &req.p_max_total_tokens {
            rule.p_max_total_tokens = max_total_tokens_opt.clone();
        }
        if let Some(tolerance_pct) = req.tolerance_pct {
            rule.tolerance_pct = tolerance_pct;
        }
        let mode_changed = req
            .trade_mode
            .as_ref()
            .is_some_and(|m| *m != rule.trade_mode);
        if let Some(trade_mode) = &req.trade_mode {
            rule.trade_mode = trade_mode.clone();
        }
        mode_changed
    }

    /// Validate + persist a new rule. Validation order is preserved: scalp gate →
    /// percent ranges → entry window.
    pub async fn create(
        repo: &Tpsl2StrategyRuleRepo,
        req: &CreateRuleRequest,
    ) -> Result<Tpsl2Rule, RuleWriteError> {
        let rule = build_rule(req);
        validate_scalp_gate(&rule).map_err(RuleWriteError::Invalid)?;
        validate_percent_ranges(&rule).map_err(RuleWriteError::Invalid)?;
        validate_entry_window(&rule).map_err(RuleWriteError::Invalid)?;
        repo.insert(&rule).await.map_err(RuleWriteError::Repo)?;
        Ok(rule)
    }

    /// Apply a partial update onto an already-loaded rule, validate, and persist.
    /// Returns `(persisted_rule, mode_changed)`. Validation order matches create.
    /// The caller loads the rule first (and runs any cache-dependent live-freeze
    /// guard) before calling this.
    pub async fn apply_and_persist(
        repo: &Tpsl2StrategyRuleRepo,
        mut rule: Tpsl2Rule,
        req: &UpdateRuleRequest,
    ) -> Result<(Tpsl2Rule, bool), RuleWriteError> {
        let mode_changed = apply_update(&mut rule, req);
        validate_scalp_gate(&rule).map_err(RuleWriteError::Invalid)?;
        validate_percent_ranges(&rule).map_err(RuleWriteError::Invalid)?;
        validate_entry_window(&rule).map_err(RuleWriteError::Invalid)?;
        repo.update(&rule).await.map_err(RuleWriteError::Repo)?;
        Ok((rule, mode_changed))
    }

    /// Delete a rule (idempotent).
    pub async fn delete(
        repo: &Tpsl2StrategyRuleRepo,
        rule_id: Uuid,
    ) -> Result<(), RuleWriteError> {
        repo.delete(rule_id).await.map_err(RuleWriteError::Repo)
    }
}
