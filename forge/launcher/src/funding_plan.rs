//! Template → funding requirement (single source of truth with the launch gate).
//!
//! JIT funding derives each wallet's amount from the launch template, not a flat
//! `FUND_AMOUNT_*` env constant. The dev requirement here is the SAME figure the
//! pre-launch balance gate in [`crate::service::execute_launch`] enforces —
//! computed once, in [`dev_launch_required_lamports`], so the funder can never
//! top a dev wallet up to less than the gate demands (CLAUDE.md SSOT rule; the
//! gate + funder used to drift). A launch's real need is template-specific:
//! `dev = create floor + dev_buy_quote`, `leg = leg buy + tip + fees`.

use anyhow::Result;

use crate::bundle::{resolve_bundle_quote, resolve_leg_count};
use crate::service::{PumpfunTemplateParams, MIN_DEV_LAUNCH_LAMPORTS};

/// Extra lamports funded on top of the strict requirement to absorb the create's
/// signature fee + transient costs, so a topped-up wallet can't land a hair under
/// its gate. Small + fixed: JIT funds the exact need with **no** ±amount jitter
/// eating into it (unlike the warm-pool `DirectJittered` path).
pub const FUNDING_HEADROOM_LAMPORTS: u64 = 2_000_000; // 0.002 SOL

/// SSOT for the dev-wallet launch requirement: the create floor
/// ([`MIN_DEV_LAUNCH_LAMPORTS`]) plus the template's dev-buy spend. The
/// pre-launch gate in `service::execute_launch` MUST call this instead of
/// re-inlining the sum, so the funder's target and the gate can't drift.
pub fn dev_launch_required_lamports(params: &PumpfunTemplateParams) -> u64 {
    MIN_DEV_LAUNCH_LAMPORTS + params.dev_buy_quote.unwrap_or(0).max(0) as u64
}

/// Per bundler leg funding target: the leg's buy quote + the bundle tip + fee/
/// rent headroom. `tip_quote` is the bundle-wide tip; a leg's actual tip is drawn
/// by the per-wallet persona disguise (Phase 2.F) within its range, so budgeting
/// the full configured tip per leg stays safe rather than under-funding a leg that
/// happens to draw the top of its tip range.
pub fn leg_required_lamports(quote_per_leg: i64, tip_quote: Option<i64>) -> u64 {
    let quote = quote_per_leg.max(0) as u64;
    let tip = tip_quote.unwrap_or(0).max(0) as u64;
    quote + tip + FUNDING_HEADROOM_LAMPORTS
}

/// The per-launch funding requirement derived from a template. `leg_count == 0`
/// (and `per_leg_lamports == 0`) means the template plans no bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundPlan {
    /// Target balance for the dev wallet (launch gate + headroom).
    pub dev_lamports: u64,
    /// Target balance for each bundler leg wallet (leg buy + tip + headroom).
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
        params: &PumpfunTemplateParams,
        requested_bundler_count: Option<u32>,
    ) -> Result<Self> {
        let dev_lamports = dev_launch_required_lamports(params) + FUNDING_HEADROOM_LAMPORTS;
        let (per_leg_lamports, leg_count) = match resolve_leg_count(requested_bundler_count, params)
        {
            Some(n) => {
                let (quote_per_leg, tip_quote) = resolve_bundle_quote(params)?;
                (leg_required_lamports(quote_per_leg, tip_quote), n)
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
        let p = params(Some(100_000_000), None, None, None);
        let gate = dev_launch_required_lamports(&p);
        assert_eq!(gate, MIN_DEV_LAUNCH_LAMPORTS + 100_000_000);
        let plan = FundPlan::from_params(&p, None).unwrap();
        assert!(plan.dev_lamports >= gate, "funded dev target must cover the gate");
        assert_eq!(plan.leg_count, 0, "no bundle configured");
    }

    #[test]
    fn plan_derives_per_leg_from_template() {
        let p = params(Some(0), Some(3), Some(50_000_000), Some(1_000_000));
        let plan = FundPlan::from_params(&p, None).unwrap();
        assert_eq!(plan.leg_count, 3);
        assert_eq!(plan.per_leg_lamports, 50_000_000 + 1_000_000 + FUNDING_HEADROOM_LAMPORTS);
    }

    #[test]
    fn request_override_wins_over_template_leg_count() {
        let p = params(Some(0), Some(3), Some(50_000_000), None);
        let plan = FundPlan::from_params(&p, Some(5)).unwrap();
        assert_eq!(plan.leg_count, 5);
    }
}
