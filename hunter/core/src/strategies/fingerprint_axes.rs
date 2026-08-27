//! Observed token-creation axes — the SSOT that turns a DB [`Token`] into the
//! engine matcher's [`TokenFingerprint`] input (plan §2.1). **Both** runtime edges
//! build the arming input through this one function so the live engine and the lab
//! replay driver match identical fingerprints on identical tokens (redesign
//! parity, decision 6): `live`'s `strategies::engine::convert` re-exports it, and
//! `lab`'s replay producer calls it directly.
//!
//! The observed axes are the `cu_*`/label-sequence exact axes plus the lamports
//! axes pulled from the creation instruction args — the full set the engine
//! matcher grades a fingerprint against.
//!
//! This module also holds the DB-row → engine-type converters
//! ([`fp_to_engine`], [`rule_to_loaded`]) for the same reason: both edges feed the
//! engine identical fingerprints + parsed rules, so a rule prices the same live or
//! replayed.

use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{Fingerprint as EngineFingerprint, FingerprintId};
use hunter_engine::fingerprint::{
    extract_lamports, normalize_labels, sol_to_lamports, TokenFingerprint,
};
use hunter_engine::rule_params::RuleParams;

use crate::models::{Fingerprint as ModelFingerprint, StrategyRule, Token};

/// Build the observed creation axes for a token. `first_slot_buy_sol` /
/// `first_slot_sell_sol` are `None` at `TokenCreated` (the creation slot has not
/// settled) and filled in from a `FirstSlotSettled` event; instant axes are always
/// read here.
///
/// Every numeric axis lands here as an INTEGER — lamports for the amounts, a tally
/// for the counts. The `initial_buy_sol` / first-slot `f64` columns convert once,
/// here, through the one shared `sol_to_lamports`.
pub fn observed_axes(
    token: &Token,
    first_slot_buy_sol: Option<f64>,
    first_slot_sell_sol: Option<f64>,
) -> TokenFingerprint {
    TokenFingerprint {
        token_program_id: token.token_program_id.clone(),
        is_cashback_enabled: token.is_cashback_enabled,
        cu_limit: token.cu_limit,
        cu_price: token.cu_price,
        // The ONE float -> integer seam on this path, at the repo boundary. Identity
        // is integer from here on, so no match ever re-derives a rounding two
        // implementations would have to agree on.
        init_buy_lamports: token.initial_buy_sol.map(sol_to_lamports),
        max_cost_lamports: extract_lamports(
            token.initial_buy_instruction.as_ref(),
            "max_cost_lamports",
        ),
        spendable_lamports_in: extract_lamports(
            token.initial_buy_instruction.as_ref(),
            "spendable_lamports_in",
        ),
        first_slot_buy_lamports: first_slot_buy_sol.map(sol_to_lamports),
        first_slot_sell_lamports: first_slot_sell_sol.map(sol_to_lamports),
        ix_labels: normalize_labels(&token.instruction_labels),
        // Not a token column: the engine stamps its own running per-creator tally
        // onto the axes in `reduce` at `TokenCreated`, before the match reads them.
        // A caller reconstructing one token out of band (the readout, a replay)
        // seeds it from `TokenRepo::count_prior_launches` instead.
        prior_launches: None,
    }
}

/// Convert a `fingerprints` row into the engine's pure matcher fingerprint. Thin by
/// design — [`ModelFingerprint::to_engine`] is the one converter, so the live gate
/// and every mirror grade the same predicates.
pub fn fp_to_engine(fp: &ModelFingerprint) -> EngineFingerprint {
    fp.to_engine()
}

/// Convert an active `strategy_rules` row into an engine [`LoadedRule`], parsing
/// its `params` once (plan §5). Returns the parse error string on invalid params so
/// the caller can skip + log that one rule rather than fail a whole reload.
pub fn rule_to_loaded(rule: &StrategyRule) -> Result<LoadedRule, String> {
    let params = RuleParams::parse(&rule.params)?;
    Ok(LoadedRule {
        id: RuleId(rule.id),
        fingerprint_id: FingerprintId(rule.fingerprint_id),
        trade_mode: match rule.trade_mode.as_str() {
            "real" => TradeMode::Real,
            _ => TradeMode::Paper,
        },
        // Lamports/caps are non-negative by construction; clamp defensively so a bad
        // row can't wrap into a huge u32/u64.
        buy_amount_lamports: rule.buy_amount_lamports.max(0) as u64,
        max_concurrent_tokens: rule.max_concurrent_tokens.clamp(0, i64::from(u32::MAX)) as u32,
        max_total_tokens: rule.max_total_tokens.clamp(0, i64::from(u32::MAX)) as u32,
        params,
        entry_enabled: rule.is_active,
    })
}
