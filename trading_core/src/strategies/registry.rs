//! Unified strategy **registry** — enum-dispatched routing over the
//! `tpsl_sniper_1` / `tpsl_sniper_2` decision modules.
//!
//! Static dispatch (a plain enum, no `dyn`/vtable) to respect the hot-path
//! budget. The decision *logic* stays in those modules (intentional clones — a
//! fix in one usually belongs in both); this file only:
//!   • maps `strategy_id` ⇄ [`StrategyImpl`],
//!   • parses the `strategy_rules.params` JSONB **once** into a typed
//!     [`StrategyParams`] ([`Tpsl1Params`] / [`Tpsl2Params`]), and
//!   • dispatches entry/exit resolution to the unchanged `find_*` fns.
//!
//! Universal knobs (`buy_amount`, `trade_mode`, caps) are the typed columns on
//! [`StrategyRule`](crate::models::StrategyRule); only the strategy-specific
//! gates live in params. [`Tpsl1Params::to_rule`] / [`Tpsl2Params::to_rule`]
//! rebuild the `Tpsl1Rule` / `Tpsl2Rule` the decision fns expect (universal
//! fields filled with inert placeholders the gates never read), so the registry
//! runs the **identical** code path as the legacy edges → exact decision parity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::models::trade::TradeRow;
use crate::models::{StrategyRule, Token, Tpsl1Rule, Tpsl2Rule};

use super::{tpsl_sniper_1 as t1, tpsl_sniper_2 as t2};

/// Which strategy family a rule belongs to. `strategy_id` string ⇄ enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyImpl {
    Tpsl1,
    Tpsl2,
}

impl StrategyImpl {
    /// Resolve a `strategy_id` (the canonical id, with the short `tpsl1`/`tpsl2`
    /// aliases accepted) to a strategy. `None` for an unknown id.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "tpsl_sniper_1" | "tpsl1" => Some(Self::Tpsl1),
            "tpsl_sniper_2" | "tpsl2" => Some(Self::Tpsl2),
            _ => None,
        }
    }

    /// The canonical `strategy_id` string persisted on rules/runs/positions.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Tpsl1 => "tpsl_sniper_1",
            Self::Tpsl2 => "tpsl_sniper_2",
        }
    }

    /// Parse the `strategy_rules.params` JSONB into the typed params for this
    /// strategy (done once at rule-load, never per-event).
    pub fn parse_params(&self, params: &Value) -> Result<StrategyParams, serde_json::Error> {
        Ok(match self {
            Self::Tpsl1 => StrategyParams::Tpsl1(serde_json::from_value(params.clone())?),
            Self::Tpsl2 => StrategyParams::Tpsl2(serde_json::from_value(params.clone())?),
        })
    }
}

fn empty_array() -> Value {
    json!([])
}

/// Rebuild the `Tpsl1Rule` the tpsl1 decision/backtest layer consumes from a
/// unified [`StrategyRule`]: the gate params come from the `params` JSONB (via
/// [`Tpsl1Params::to_rule`]); the universal knobs (`id`, name, `buy_amount`,
/// `trade_mode`, caps) are copied from the row's typed columns (`to_rule` leaves
/// them as inert placeholders). Errors only if `params` isn't valid tpsl1 JSON.
pub fn tpsl1_decision_rule(sr: &StrategyRule) -> Result<Tpsl1Rule, serde_json::Error> {
    let StrategyParams::Tpsl1(p) = StrategyImpl::Tpsl1.parse_params(&sr.params)? else {
        unreachable!("Tpsl1.parse_params always yields Tpsl1 params")
    };
    let mut r = p.to_rule();
    r.id = sr.id;
    r.rule_name = sr.rule_name.clone();
    r.buy_amount = sr.buy_amount;
    r.trade_mode = sr.trade_mode.clone();
    r.p_max_concurrent_tokens = sr.max_concurrent_tokens.map(|v| v as u64);
    r.p_max_total_tokens = sr.max_total_tokens.map(|v| v as u64);
    Ok(r)
}

/// tpsl2 twin of [`tpsl1_decision_rule`] — rebuild the `Tpsl2Rule` from a unified
/// [`StrategyRule`].
pub fn tpsl2_decision_rule(sr: &StrategyRule) -> Result<Tpsl2Rule, serde_json::Error> {
    let StrategyParams::Tpsl2(p) = StrategyImpl::Tpsl2.parse_params(&sr.params)? else {
        unreachable!("Tpsl2.parse_params always yields Tpsl2 params")
    };
    let mut r = p.to_rule();
    r.id = sr.id;
    r.rule_name = sr.rule_name.clone();
    r.buy_amount = sr.buy_amount;
    r.trade_mode = sr.trade_mode.clone();
    r.p_max_concurrent_tokens = sr.max_concurrent_tokens.map(|v| v as u64);
    r.p_max_total_tokens = sr.max_total_tokens.map(|v| v as u64);
    Ok(r)
}

/// tpsl1 strategy-specific gate params (the JSONB "brain"). Field names match the
/// legacy `Tpsl1Rule` `p_*` fields so the column deserializes straight in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tpsl1Params {
    #[serde(default)]
    pub p_token_initial_buy_sol: Option<f64>,
    #[serde(default)]
    pub p_token_cu_limit: Option<u64>,
    #[serde(default)]
    pub p_token_cu_price: Option<u64>,
    #[serde(default)]
    pub p_token_max_sol_cost: Option<f64>,
    #[serde(default)]
    pub p_token_spendable_sol_in: Option<f64>,
    #[serde(default = "empty_array")]
    pub p_token_ix_labels: Value,
    #[serde(default)]
    pub tolerance_pct: f64,
    pub p_exit_take_profit: f64,
    pub p_exit_stop_loss: f64,
    #[serde(default)]
    pub p_exit_trailing_stop_pct: Option<f64>,
    #[serde(default)]
    pub p_exit_time_stop_secs: Option<u64>,
    #[serde(default)]
    pub p_exit_stall_secs: Option<u64>,
    #[serde(default)]
    pub p_exit_liquidity_drop_pct: Option<f64>,
}

impl Tpsl1Params {
    /// Lift the gate params out of a full rule (the inverse of [`to_rule`]).
    pub fn from_rule(r: &Tpsl1Rule) -> Self {
        Self {
            p_token_initial_buy_sol: r.p_token_initial_buy_sol,
            p_token_cu_limit: r.p_token_cu_limit,
            p_token_cu_price: r.p_token_cu_price,
            p_token_max_sol_cost: r.p_token_max_sol_cost,
            p_token_spendable_sol_in: r.p_token_spendable_sol_in,
            p_token_ix_labels: r.p_token_ix_labels.clone(),
            tolerance_pct: r.tolerance_pct,
            p_exit_take_profit: r.p_exit_take_profit,
            p_exit_stop_loss: r.p_exit_stop_loss,
            p_exit_trailing_stop_pct: r.p_exit_trailing_stop_pct,
            p_exit_time_stop_secs: r.p_exit_time_stop_secs,
            p_exit_stall_secs: r.p_exit_stall_secs,
            p_exit_liquidity_drop_pct: r.p_exit_liquidity_drop_pct,
        }
    }

    /// Rebuild the `Tpsl1Rule` the decision fns consume. Universal fields are
    /// inert placeholders the entry/exit gates never read (`buy_amount`,
    /// `trade_mode`, caps live on `StrategyRule`); `is_active` is forced true so
    /// the single-rule match gate evaluates.
    pub fn to_rule(&self) -> Tpsl1Rule {
        let mut r = Tpsl1Rule::new(
            String::new(),
            self.p_token_initial_buy_sol,
            self.p_token_cu_limit,
            self.p_token_cu_price,
            self.p_token_ix_labels.clone(),
            "paper".into(),
            0.0,
            self.p_exit_take_profit,
            self.p_exit_stop_loss,
            self.p_token_max_sol_cost,
            self.p_token_spendable_sol_in,
            None,
            None,
            Some(self.tolerance_pct),
            self.p_exit_trailing_stop_pct,
            self.p_exit_time_stop_secs,
            self.p_exit_stall_secs,
            self.p_exit_liquidity_drop_pct,
        );
        r.is_active = true;
        r
    }
}

/// tpsl2 strategy-specific gate params — the tpsl1 set plus the scalp-continuation
/// entry gates and the E5 cohort-dump exit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tpsl2Params {
    #[serde(default)]
    pub p_token_initial_buy_sol: Option<f64>,
    #[serde(default)]
    pub p_token_cu_limit: Option<u64>,
    #[serde(default)]
    pub p_token_cu_price: Option<u64>,
    #[serde(default)]
    pub p_token_max_sol_cost: Option<f64>,
    #[serde(default)]
    pub p_token_spendable_sol_in: Option<f64>,
    #[serde(default = "empty_array")]
    pub p_token_ix_labels: Value,
    #[serde(default)]
    pub tolerance_pct: f64,
    pub p_exit_take_profit: f64,
    pub p_exit_stop_loss: f64,
    #[serde(default)]
    pub p_exit_trailing_stop_pct: Option<f64>,
    #[serde(default)]
    pub p_exit_time_stop_secs: Option<u64>,
    #[serde(default)]
    pub p_exit_stall_secs: Option<u64>,
    #[serde(default)]
    pub p_exit_liquidity_drop_pct: Option<f64>,
    // Scalp-continuation entry gates (all inert at None/0).
    #[serde(default)]
    pub p_entry_min_age_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_max_age_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_min_alive_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_min_organic_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_pullback_pct: Option<f64>,
    #[serde(default)]
    pub p_entry_higher_low_secs: Option<u64>,
    #[serde(default)]
    pub p_entry_max_cohort_held: Option<f64>,
    #[serde(default)]
    pub p_entry_min_liquidity_sol: Option<f64>,
    #[serde(default)]
    pub p_entry_min_organic_liq: Option<f64>,
    // E5 cohort-dump exit.
    #[serde(default)]
    pub p_exit_cohort_ratio: Option<f64>,
}

impl Tpsl2Params {
    /// Lift the gate params out of a full rule (the inverse of [`to_rule`]).
    pub fn from_rule(r: &Tpsl2Rule) -> Self {
        Self {
            p_token_initial_buy_sol: r.p_token_initial_buy_sol,
            p_token_cu_limit: r.p_token_cu_limit,
            p_token_cu_price: r.p_token_cu_price,
            p_token_max_sol_cost: r.p_token_max_sol_cost,
            p_token_spendable_sol_in: r.p_token_spendable_sol_in,
            p_token_ix_labels: r.p_token_ix_labels.clone(),
            tolerance_pct: r.tolerance_pct,
            p_exit_take_profit: r.p_exit_take_profit,
            p_exit_stop_loss: r.p_exit_stop_loss,
            p_exit_trailing_stop_pct: r.p_exit_trailing_stop_pct,
            p_exit_time_stop_secs: r.p_exit_time_stop_secs,
            p_exit_stall_secs: r.p_exit_stall_secs,
            p_exit_liquidity_drop_pct: r.p_exit_liquidity_drop_pct,
            p_entry_min_age_secs: r.p_entry_min_age_secs,
            p_entry_max_age_secs: r.p_entry_max_age_secs,
            p_entry_min_alive_sol: r.p_entry_min_alive_sol,
            p_entry_min_organic_sol: r.p_entry_min_organic_sol,
            p_entry_pullback_pct: r.p_entry_pullback_pct,
            p_entry_higher_low_secs: r.p_entry_higher_low_secs,
            p_entry_max_cohort_held: r.p_entry_max_cohort_held,
            p_entry_min_liquidity_sol: r.p_entry_min_liquidity_sol,
            p_entry_min_organic_liq: r.p_entry_min_organic_liq,
            p_exit_cohort_ratio: r.p_exit_cohort_ratio,
        }
    }

    /// Rebuild the `Tpsl2Rule` the decision fns consume — base knobs via `new`,
    /// then the scalp gates + E5 set post-construction (mirroring the tpsl2 API).
    pub fn to_rule(&self) -> Tpsl2Rule {
        let mut r = Tpsl2Rule::new(
            String::new(),
            self.p_token_initial_buy_sol,
            self.p_token_cu_limit,
            self.p_token_cu_price,
            self.p_token_ix_labels.clone(),
            "paper".into(),
            0.0,
            self.p_exit_take_profit,
            self.p_exit_stop_loss,
            self.p_token_max_sol_cost,
            self.p_token_spendable_sol_in,
            None,
            None,
            Some(self.tolerance_pct),
            self.p_exit_trailing_stop_pct,
            self.p_exit_time_stop_secs,
            self.p_exit_stall_secs,
            self.p_exit_liquidity_drop_pct,
        );
        r.p_entry_min_age_secs = self.p_entry_min_age_secs;
        r.p_entry_max_age_secs = self.p_entry_max_age_secs;
        r.p_entry_min_alive_sol = self.p_entry_min_alive_sol;
        r.p_entry_min_organic_sol = self.p_entry_min_organic_sol;
        r.p_entry_pullback_pct = self.p_entry_pullback_pct;
        r.p_entry_higher_low_secs = self.p_entry_higher_low_secs;
        r.p_entry_max_cohort_held = self.p_entry_max_cohort_held;
        r.p_entry_min_liquidity_sol = self.p_entry_min_liquidity_sol;
        r.p_entry_min_organic_liq = self.p_entry_min_organic_liq;
        r.p_exit_cohort_ratio = self.p_exit_cohort_ratio;
        r.is_active = true;
        r
    }
}

/// The parsed, typed params for a rule — one variant per [`StrategyImpl`].
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyParams {
    Tpsl1(Tpsl1Params),
    Tpsl2(Tpsl2Params),
}

impl StrategyParams {
    /// The strategy this params payload belongs to.
    pub fn strategy(&self) -> StrategyImpl {
        match self {
            Self::Tpsl1(_) => StrategyImpl::Tpsl1,
            Self::Tpsl2(_) => StrategyImpl::Tpsl2,
        }
    }
}

/// The resolved entry fill — strategy-agnostic (price/time/tx only).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEntry {
    pub price: f64,
    pub tx_signature: String,
    pub block_time: DateTime<Utc>,
}

/// The resolved exit fill plus the (stable, persisted) reason string.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedExit {
    pub price: f64,
    pub tx_signature: String,
    pub block_time: DateTime<Utc>,
    pub reason: &'static str,
}

impl StrategyImpl {
    /// Token-creation entry gate: does this token satisfy the rule's buy
    /// criteria? Used by the live token-created path. A params/impl mismatch
    /// returns `false`.
    pub fn matches_entry(&self, token: &Token, params: &StrategyParams) -> bool {
        match (self, params) {
            (Self::Tpsl1, StrategyParams::Tpsl1(p)) => {
                t1::entry::token_matches_buy_rule(token, &p.to_rule())
            }
            (Self::Tpsl2, StrategyParams::Tpsl2(p)) => {
                t2::entry::token_matches_buy_rule(token, &p.to_rule())
            }
            _ => false,
        }
    }

    /// Resolve the entry fill from a token's trade history. tpsl1 takes the
    /// fixed first-block fill (`find_entry_fill_in_trades`, cap 1, matching the
    /// sweep); tpsl2 finds the first scalp-gated trade then its worst-case paper
    /// fill (the exact sequence the sweep and live paths use). A params/impl
    /// mismatch returns `None`.
    pub fn resolve_entry<T: TradeRow>(
        &self,
        trades: &[T],
        params: &StrategyParams,
    ) -> Option<ResolvedEntry> {
        match (self, params) {
            (Self::Tpsl1, StrategyParams::Tpsl1(_)) => {
                let f = t1::entry::find_entry_fill_in_trades(trades, 1)?;
                Some(ResolvedEntry {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                })
            }
            (Self::Tpsl2, StrategyParams::Tpsl2(p)) => {
                let rule = p.to_rule();
                let cohort = t2::entry::scalp_cohort(trades);
                let (trigger_idx, _) =
                    t2::entry::find_scalp_entry_with_cohort_indexed(trades, &rule, &cohort)?;
                let f = t2::entry::find_worst_case_paper_entry_at(trades, trigger_idx)?;
                if f.price <= 0.0 {
                    return None;
                }
                Some(ResolvedEntry {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                })
            }
            _ => None,
        }
    }

    /// Walk the post-entry trades and resolve the exit fill (full re-walk, the
    /// live trade-gate / sweep semantics). tpsl2 builds its launch cohort
    /// internally for the E5 gate. A params/impl mismatch returns `None`.
    pub fn resolve_exit<T: TradeRow>(
        &self,
        trades: &[T],
        entry_time: DateTime<Utc>,
        entry_price: f64,
        params: &StrategyParams,
    ) -> Option<ResolvedExit> {
        match (self, params) {
            (Self::Tpsl1, StrategyParams::Tpsl1(p)) => {
                let f = t1::exit::find_trade_driven_exit(
                    trades,
                    entry_time,
                    entry_price,
                    &p.to_rule(),
                )?;
                Some(ResolvedExit {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                    reason: f.reason.as_str(),
                })
            }
            (Self::Tpsl2, StrategyParams::Tpsl2(p)) => {
                let f = t2::exit::find_trade_driven_exit(
                    trades,
                    entry_time,
                    entry_price,
                    &p.to_rule(),
                )?;
                Some(ResolvedExit {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                    reason: f.reason.as_str(),
                })
            }
            _ => None,
        }
    }

    /// Resolve the paper exit fill for the **live fill-poll** — the trade-driven
    /// exit plus an optional **firing slot** that tells the poll when to record it.
    ///
    /// - `None` slot ⇒ record on first find (tpsl1: the resolver already returns the
    ///   final fill, so there is no worst-case window to wait out).
    /// - `Some(S)` ⇒ a slot-windowed worst-case fill (tpsl2) that can only *improve*
    ///   (drop) until slot `S + MAX_FILL_WAIT_SLOTS` indexes, so the poll keeps the
    ///   freshest fill and records once a trade past the window lands.
    ///
    /// A params/impl mismatch returns `None`.
    pub fn resolve_paper_exit<T: TradeRow>(
        &self,
        trades: &[T],
        entry_time: DateTime<Utc>,
        entry_price: f64,
        params: &StrategyParams,
    ) -> Option<(ResolvedExit, Option<u64>)> {
        match (self, params) {
            (Self::Tpsl1, StrategyParams::Tpsl1(p)) => {
                let f = t1::exit::find_trade_driven_exit(
                    trades,
                    entry_time,
                    entry_price,
                    &p.to_rule(),
                )?;
                let exit = ResolvedExit {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                    reason: f.reason.as_str(),
                };
                Some((exit, None))
            }
            (Self::Tpsl2, StrategyParams::Tpsl2(p)) => {
                let (f, fire_slot) = t2::exit::find_trade_driven_exit_with_slot(
                    trades,
                    entry_time,
                    entry_price,
                    &p.to_rule(),
                )?;
                let exit = ResolvedExit {
                    price: f.price,
                    tx_signature: f.tx_signature,
                    block_time: f.block_time,
                    reason: f.reason.as_str(),
                };
                Some((exit, Some(fire_slot)))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Duration;
    use serde_json::json;

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn buy(price: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            price,
            1,
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + Duration::seconds(secs),
        )
    }

    // ── id ⇄ enum ────────────────────────────────────────────────────────────

    #[test]
    fn from_id_round_trips_canonical_and_aliases() {
        assert_eq!(StrategyImpl::from_id("tpsl_sniper_1"), Some(StrategyImpl::Tpsl1));
        assert_eq!(StrategyImpl::from_id("tpsl1"), Some(StrategyImpl::Tpsl1));
        assert_eq!(StrategyImpl::from_id("tpsl_sniper_2"), Some(StrategyImpl::Tpsl2));
        assert_eq!(StrategyImpl::from_id("tpsl2"), Some(StrategyImpl::Tpsl2));
        assert_eq!(StrategyImpl::from_id("nope"), None);
        assert_eq!(StrategyImpl::Tpsl1.id(), "tpsl_sniper_1");
        assert_eq!(StrategyImpl::Tpsl2.id(), "tpsl_sniper_2");
    }

    // ── params parse + JSONB round-trip ──────────────────────────────────────

    #[test]
    fn tpsl1_params_parse_from_jsonb() {
        let v = json!({
            "p_token_initial_buy_sol": 1.0,
            "tolerance_pct": 10.0,
            "p_exit_take_profit": 50.0,
            "p_exit_stop_loss": 20.0,
            "p_exit_trailing_stop_pct": 30.0
        });
        let parsed = StrategyImpl::Tpsl1.parse_params(&v).expect("parse");
        let StrategyParams::Tpsl1(p) = parsed else { panic!("wrong variant") };
        assert_eq!(p.p_token_initial_buy_sol, Some(1.0));
        assert_eq!(p.p_exit_trailing_stop_pct, Some(30.0));
        // Omitted optionals default to None; labels default to empty array.
        assert_eq!(p.p_exit_stall_secs, None);
        assert_eq!(p.p_token_ix_labels, json!([]));
    }

    #[test]
    fn params_serde_round_trip_preserves_fields() {
        let p = Tpsl1Params {
            p_token_initial_buy_sol: Some(1.0),
            p_token_cu_limit: Some(100_000),
            p_token_cu_price: None,
            p_token_max_sol_cost: None,
            p_token_spendable_sol_in: None,
            p_token_ix_labels: json!(["A", "B"]),
            tolerance_pct: 5.0,
            p_exit_take_profit: 40.0,
            p_exit_stop_loss: 15.0,
            p_exit_trailing_stop_pct: Some(25.0),
            p_exit_time_stop_secs: Some(300),
            p_exit_stall_secs: None,
            p_exit_liquidity_drop_pct: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        let back: Tpsl1Params = serde_json::from_value(v).unwrap();
        assert_eq!(p, back);
    }

    // ── decision parity vs the direct tpsl1/2 fns ────────────────────────────

    #[test]
    fn tpsl1_resolve_exit_matches_direct_fn() {
        // Moonshot then reversal → trailing stop fires; fill in slot F+1.
        let trades = vec![
            buy(2.0, 2, 1),
            buy(3.0, 3, 2),
            buy(2.5, 4, 3),
            buy(2.0, 5, 4),
            buy(2.0, 6, 5),
        ];
        let mut rule = Tpsl1Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            1000.0, 90.0, None, None, None, None, Some(0.0),
            Some(30.0), None, None, None,
        );
        rule.is_active = true;

        let direct = t1::exit::find_trade_driven_exit(&trades, base_time(), 1.0, &rule)
            .expect("direct exit");
        let params = StrategyParams::Tpsl1(Tpsl1Params::from_rule(&rule));
        let via = StrategyImpl::Tpsl1
            .resolve_exit(&trades, base_time(), 1.0, &params)
            .expect("registry exit");

        assert_eq!(via.reason, direct.reason.as_str());
        assert!((via.price - direct.price).abs() < 1e-12);
        assert_eq!(via.block_time, direct.block_time);
        assert_eq!(via.tx_signature, direct.tx_signature);
        assert_eq!(via.reason, "TrailingStop");
    }

    #[test]
    fn tpsl1_resolve_entry_matches_direct_fn() {
        let trades = vec![buy(1.0, 5, 1), buy(2.0, 5, 1), buy(9.0, 6, 2)];
        let direct = t1::entry::find_entry_fill_in_trades(&trades, 1).expect("direct entry");
        let params = StrategyParams::Tpsl1(Tpsl1Params {
            p_token_initial_buy_sol: Some(1.0),
            p_token_cu_limit: None,
            p_token_cu_price: None,
            p_token_max_sol_cost: None,
            p_token_spendable_sol_in: None,
            p_token_ix_labels: json!([]),
            tolerance_pct: 0.0,
            p_exit_take_profit: 50.0,
            p_exit_stop_loss: 20.0,
            p_exit_trailing_stop_pct: None,
            p_exit_time_stop_secs: None,
            p_exit_stall_secs: None,
            p_exit_liquidity_drop_pct: None,
        });
        let via = StrategyImpl::Tpsl1.resolve_entry(&trades, &params).expect("registry entry");
        assert!((via.price - direct.price).abs() < 1e-12);
        assert_eq!(via.tx_signature, direct.tx_signature);
        assert_eq!(via.block_time, direct.block_time);
    }

    #[test]
    fn tpsl2_resolve_exit_matches_direct_fn() {
        // Price-only exit (E5 off) → stop loss; registry's self-contained path
        // must equal the direct tpsl2 fn.
        let trades = vec![buy(3.0, 2, 1), buy(0.05, 3, 2), buy(0.05, 4, 3)];
        let mut rule = Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            1000.0, 90.0, None, None, None, None, Some(0.0),
            Some(30.0), None, None, None,
        );
        rule.is_active = true;

        let direct = t2::exit::find_trade_driven_exit(&trades, base_time(), 1.0, &rule)
            .expect("direct exit");
        let params = StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule));
        let via = StrategyImpl::Tpsl2
            .resolve_exit(&trades, base_time(), 1.0, &params)
            .expect("registry exit");

        assert_eq!(via.reason, direct.reason.as_str());
        assert!((via.price - direct.price).abs() < 1e-12);
        assert_eq!(via.block_time, direct.block_time);
        assert_eq!(via.reason, "StopLoss");
    }

    #[test]
    fn mismatched_impl_and_params_resolve_to_none() {
        let trades = vec![buy(1.0, 2, 1)];
        let params = StrategyParams::Tpsl2(Tpsl2Params::from_rule(&Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.0), None, None, None, None,
        )));
        // Tpsl1 impl fed Tpsl2 params → None, never a panic.
        assert!(StrategyImpl::Tpsl1.resolve_entry(&trades, &params).is_none());
        assert!(StrategyImpl::Tpsl1.resolve_exit(&trades, base_time(), 1.0, &params).is_none());
    }
}
