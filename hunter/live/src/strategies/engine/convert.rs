//! DB model ↔ engine type converters (plan 4.x — no conversion existed before).
//!
//! The engine consumes *its own* pure types ([`hunter_engine::event::LoadedRule`],
//! [`hunter_engine::fingerprint::Fingerprint`]) with `params` **parsed once** into
//! [`RuleParams`]. The DB gives us `models::StrategyRule` / `models::Fingerprint`
//! (raw `params: Value`, lamports `i64`). This module bridges the two and builds
//! the observed [`TokenFingerprint`] axes from a live [`Token`] (the arming input).

use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
use hunter_engine::fingerprint::{Fingerprint as EngineFingerprint, FingerprintId};
use hunter_engine::grouping::{extract_lamports, normalize_labels, TokenFingerprint};
use hunter_engine::rule_params::RuleParams;

use trading_core::models::{Fingerprint as ModelFingerprint, StrategyRule, Token};

/// Convert a `fingerprints` row into the engine's pure matcher fingerprint (drops
/// the label/timestamps; wraps the id). Lamports axes carry over verbatim.
pub fn fp_to_engine(fp: &ModelFingerprint) -> EngineFingerprint {
    EngineFingerprint {
        id: FingerprintId(fp.id),
        cu_limit: fp.cu_limit,
        cu_price: fp.cu_price,
        ix_labels: fp.ix_labels.clone(),
        init_buy_lamports: fp.init_buy_lamports,
        max_cost_lamports: fp.max_cost_lamports,
        spendable_lamports_in: fp.spendable_lamports_in,
        first_slot_buy_lamports: fp.first_slot_buy_lamports,
        first_slot_sell_lamports: fp.first_slot_sell_lamports,
        bucket_size_amount: fp.bucket_size_amount,
    }
}

/// Convert an active `strategy_rules` row into an engine [`LoadedRule`], parsing
/// its `params` once (plan §5). Returns the parse error string on invalid params
/// so the caller can skip + log that one rule rather than fail the whole reload.
pub fn rule_to_loaded(rule: &StrategyRule) -> Result<LoadedRule, String> {
    let params = RuleParams::parse(&rule.params)?;
    Ok(LoadedRule {
        id: RuleId(rule.id),
        fingerprint_id: FingerprintId(rule.fingerprint_id),
        trade_mode: match rule.trade_mode.as_str() {
            "real" => TradeMode::Real,
            _ => TradeMode::Paper,
        },
        // Lamports/caps are non-negative by construction; clamp defensively so a
        // bad row can't wrap into a huge u32/u64.
        buy_amount_lamports: rule.buy_amount_lamports.max(0) as u64,
        max_concurrent_tokens: rule.max_concurrent_tokens.clamp(0, i64::from(u32::MAX)) as u32,
        max_total_tokens: rule.max_total_tokens.clamp(0, i64::from(u32::MAX)) as u32,
        params,
    })
}

/// Build the observed creation axes for a live token — the SSOT the legacy
/// `check_*` entry set read (`tpsl_sniper_1/entry/mod.rs`), reproduced here so the
/// engine matcher sees identical inputs. `first_slot_*` are `None` at
/// `TokenCreated` (unknown until the slot settles) and filled in at
/// `FirstSlotSettled`.
///
/// * `cu_limit`/`cu_price` — exact, `u64` → `i64`.
/// * `initial_buy_sol` — the dev-buy SOL.
/// * `max_cost_lamports`/`spendable_lamports_in` — read from the creation
///   instruction args (number or numeric string), lamports at rest.
/// * `ix_labels` — the creation instruction-label sequence, exact order.
pub fn observed_axes(
    token: &Token,
    first_slot_buy_sol: Option<f64>,
    first_slot_sell_sol: Option<f64>,
) -> TokenFingerprint {
    TokenFingerprint {
        token_program_id: token.token_program_id.clone(),
        initial_buy_sol: token.initial_buy_sol,
        cu_limit: token.cu_limit.map(|v| v as i64),
        cu_price: token.cu_price.map(|v| v as i64),
        is_cashback_enabled: false,
        max_cost_lamports: extract_lamports(
            token.initial_buy_instruction.as_ref(),
            "max_cost_lamports",
        ),
        spendable_lamports_in: extract_lamports(
            token.initial_buy_instruction.as_ref(),
            "spendable_lamports_in",
        ),
        first_slot_buy_sol,
        first_slot_sell_sol,
        ix_labels: normalize_labels(&token.instruction_labels),
    }
}
