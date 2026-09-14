//! Template → funding requirement (single source of truth with the launch gate).
//!
//! JIT funding derives each wallet's amount from the launch template, not a flat
//! `FUND_AMOUNT_*` env constant. The dev requirement here is the SAME figure the
//! pre-launch balance gate in [`crate::service::execute_launch`] enforces —
//! computed once, in [`dev_launch_required_lamports`], so the funder can never
//! top a dev wallet up to less than the gate demands (CLAUDE.md SSOT rule — a
//! second copy of that figure drifts). A launch's real need is template-specific:
//! `dev = create floor + dev_buy_quote`, `leg = leg buy + ATA rent + fees + tip +
//! the payer's rent-exempt minimum` (see [`leg_required_lamports`]).

use anyhow::Result;

use crate::bundle::{resolve_bundle_quote, resolve_leg_count};
use crate::service::{min_dev_launch_lamports, PumpfunTemplateParams};

/// Extra lamports funded on top of the strict requirement to absorb the create's
/// signature fee + transient costs, so a topped-up wallet can't land a hair under
/// its gate. Small + fixed: JIT funds the exact need with **no** ±amount jitter
/// eating into it (unlike the warm-pool `DirectJittered` path).
pub const FUNDING_HEADROOM_LAMPORTS: u64 = 2_000_000; // 0.002 SOL

/// SSOT for the dev-wallet launch requirement: the create floor
/// ([`min_dev_launch_lamports`], = variant-aware rent/fees + the live tip ceiling)
/// plus the template's dev-buy spend. The pre-launch gate in
/// `service::execute_launch` MUST call this instead of re-inlining the sum, so the
/// funder's target and the gate can't drift. `variant` is the launch template's
/// `variant` (`pumpfun.create_v1` / `_v2`) — a legacy v1 create costs ~2× the rent
/// of a v2, so the floor is variant-keyed. `tip_ceiling_lamports` is the
/// `JITO_MAX_TIP_SOL` ceiling the caller reads from settings.
pub fn dev_launch_required_lamports(
    variant: &str,
    params: &PumpfunTemplateParams,
    tip_ceiling_lamports: u64,
) -> u64 {
    min_dev_launch_lamports(variant, tip_ceiling_lamports)
        + params.dev_buy_quote.unwrap_or(0).max(0) as u64
}

/// Per bundler-leg funding target (lamports): everything one co-buy leg takes out
/// of its wallet, so a wallet funded to this can land the leg and still pass rent:
///
/// - `quote_per_leg` — the SOL-in buy (`buy_exact_sol_in` / `buy_exact_quote_in_v2`):
///   the program takes its venue fee out of this spend, so no fee rides on top;
/// - the base token ATA's rent, sized by the launch's token program (legacy SPL
///   for `create_v1`, Token-2022 otherwise), plus the WSOL quote ATA's rent the v2
///   encodings create (a persona may swap a SOL-in leg onto one);
/// - one signature fee;
/// - the worst-case priority fee + tip any persona draws for a curve buy
///   ([`crate::plan_pipeline::plan_personas`], the set the gate disguises with —
///   the co-buy tip is the persona draw, never the bundle-wide `bundle_tip_quote`,
///   which rides the create leg);
/// - the rent-exempt minimum a system account keeps once it holds any lamports.
pub fn leg_required_lamports(quote_per_leg: i64, token_program_id: &str) -> u64 {
    use pump_trader::protocol::{TOKEN_2022_ACCOUNT_SPACE, TOKEN_2022_PROGRAM_ID, TOKEN_ACCOUNT_SPACE};
    use solana_sdk::{fee::FeeStructure, rent::Rent};

    let rent = Rent::default();
    let base_ata_space = if token_program_id == TOKEN_2022_PROGRAM_ID {
        TOKEN_2022_ACCOUNT_SPACE
    } else {
        TOKEN_ACCOUNT_SPACE
    };
    let ata_rent = rent.minimum_balance(base_ata_space as usize)
        + rent.minimum_balance(TOKEN_ACCOUNT_SPACE as usize);
    let personas = crate::plan_pipeline::plan_personas();
    let fee_and_tip = FeeStructure::default().lamports_per_signature
        + personas.max_priority_fee_lamports(crate::plan_pipeline::plan_compute_cfg().curve_buy_cu)
        + personas.max_tip_lamports();
    quote_per_leg.max(0) as u64 + ata_rent + fee_and_tip + rent.minimum_balance(0)
}

/// The per-launch funding requirement derived from a template. `leg_count == 0`
/// (and `per_leg_lamports == 0`) means the template plans no bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundPlan {
    /// Target balance for the dev wallet (launch gate + headroom).
    pub dev_lamports: u64,
    /// Target balance for each bundler leg wallet ([`leg_required_lamports`]).
    pub per_leg_lamports: u64,
    /// Number of bundler legs this launch will run (0 = no bundle).
    pub leg_count: u32,
}

impl FundPlan {
    /// Derive the requirement from a launch template's parsed params + the launch
    /// request's optional "use N bundlers" override. Reuses the SAME
    /// `resolve_leg_count` / `resolve_bundle_quote` the launch executor uses, so
    /// the funded amounts match what the bundle actually spends.
    pub fn from_params(
        variant: &str,
        params: &PumpfunTemplateParams,
        requested_bundler_count: Option<u32>,
        tip_ceiling_lamports: u64,
    ) -> Result<Self> {
        let dev_lamports =
            dev_launch_required_lamports(variant, params, tip_ceiling_lamports)
                + FUNDING_HEADROOM_LAMPORTS;
        let (per_leg_lamports, leg_count) = match resolve_leg_count(requested_bundler_count, params)
        {
            Some(n) => {
                let (quote_per_leg, _bundle_tip) = resolve_bundle_quote(params)?;
                let token_program = crate::keystore::token_program_for_variant(variant);
                (leg_required_lamports(quote_per_leg, token_program), n)
            }
            None => (0, 0),
        };
        Ok(Self { dev_lamports, per_leg_lamports, leg_count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(dev_buy: Option<i64>, legs: Option<u32>, per_leg: Option<i64>, tip: Option<i64>) -> PumpfunTemplateParams {
        PumpfunTemplateParams {
            dev_buy_quote: dev_buy,
            dev_buy_variant: None,
            slippage_bps: None,
            is_mayhem_mode: false,
            cashback_enabled: false,
            bundle_leg_count: legs,
            bundle_quote_per_leg: per_leg,
            bundle_tip_quote: tip,
            leg_structures: None,
            create_layout: None,
        }
    }

    #[test]
    fn dev_required_is_the_launch_gate() {
        // The funder's dev target must never be below the launch gate the executor
        // enforces — this is the SSOT guard.
        let tip = 1_000_000; // JITO_MAX_TIP_SOL ceiling, in lamports
        let p = params(Some(100_000_000), None, None, None);
        let gate = dev_launch_required_lamports("pumpfun.create_v2", &p, tip);
        assert_eq!(gate, min_dev_launch_lamports("pumpfun.create_v2", tip) + 100_000_000);
        let plan = FundPlan::from_params("pumpfun.create_v2", &p, None, tip).unwrap();
        assert!(plan.dev_lamports >= gate, "funded dev target must cover the gate");
        assert_eq!(plan.leg_count, 0, "no bundle configured");
    }

    #[test]
    fn create_v1_floor_exceeds_v2() {
        // Regression: a legacy `create_v1` allocates a separate Metaplex metadata
        // account (~2× the create rent of a v2), so its dev-wallet floor MUST be
        // strictly higher — under-budgeting it silently under-funds the fused
        // dev-buy and reverts on-chain (System `ResultWithNegativeLamports`).
        let tip = 1_000_000;
        let p = params(Some(30_000_000), None, None, None);
        let v1 = dev_launch_required_lamports("pumpfun.create_v1", &p, tip);
        let v2 = dev_launch_required_lamports("pumpfun.create_v2", &p, tip);
        assert!(v1 > v2, "v1 floor {v1} must exceed v2 floor {v2}");
        // The delta is entirely the create-rent difference (same dev-buy + tip).
        assert_eq!(v1 - v2, 26_000_000 - 13_000_000);
    }

    #[test]
    fn plan_derives_per_leg_from_template() {
        let p = params(Some(0), Some(3), Some(50_000_000), Some(1_000_000));
        let plan = FundPlan::from_params("pumpfun.create_v2", &p, None, 0).unwrap();
        assert_eq!(plan.leg_count, 3);
        assert_eq!(
            plan.per_leg_lamports,
            leg_required_lamports(50_000_000, pump_trader::protocol::TOKEN_2022_PROGRAM_ID)
        );
    }

    /// A leg covers its buy, both ATA rents, the signature fee, the worst persona
    /// fee + tip, and the payer's rent-exempt minimum — not a flat 0.002 headroom.
    /// The bundle-wide tip is not a co-buy cost.
    #[test]
    fn leg_requirement_itemizes_every_cost() {
        use pump_trader::protocol::{TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
        let legacy = leg_required_lamports(10_000_000, TOKEN_PROGRAM_ID);
        // 10M buy + 2 x 2,039,280 ATA rent + 5,000 fee + 76,500 priority + 10,000 tip
        // + 890,880 rent-exempt minimum.
        assert_eq!(legacy, 10_000_000 + 2 * 2_039_280 + 5_000 + 76_500 + 10_000 + 890_880);
        // A Token-2022 base ATA is larger, so its rent is higher.
        assert!(leg_required_lamports(10_000_000, TOKEN_2022_PROGRAM_ID) > legacy);
        // The template's bundle tip never changes the per-leg target.
        let a = FundPlan::from_params("pumpfun.create_v1", &params(None, Some(1), Some(10_000_000), None), None, 0)
            .unwrap();
        let b = FundPlan::from_params(
            "pumpfun.create_v1",
            &params(None, Some(1), Some(10_000_000), Some(5_000_000)),
            None,
            0,
        )
        .unwrap();
        assert_eq!(a.per_leg_lamports, legacy);
        assert_eq!(a.per_leg_lamports, b.per_leg_lamports);
    }

    #[test]
    fn request_override_wins_over_template_leg_count() {
        let p = params(Some(0), Some(3), Some(50_000_000), None);
        let plan = FundPlan::from_params("pumpfun.create_v2", &p, Some(5), 0).unwrap();
        assert_eq!(plan.leg_count, 5);
    }
}
