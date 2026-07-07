//! Bundle leg composer — per-leg structure descriptors (§3e).
//!
//! Draws audited buy variants + randomized budget/tip from a template pool.
//! Jito bundle **submission** is a later step; this module plans legs and persists
//! them to `bundles.legs`.

use anyhow::{bail, Context, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use uuid::Uuid;

/// Audited pump.fun buy discriminator surface (never an arbitrary account list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuyVariant {
    Buy,
    BuyExactSolIn,
    BuyV2,
    BuyExactQuoteIn,
}

impl BuyVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::BuyExactSolIn => "buy_exact_sol_in",
            Self::BuyV2 => "buy_v2",
            Self::BuyExactQuoteIn => "buy_exact_quote_in",
        }
    }
}

/// One entry in `launch_templates.params.leg_structures` — ranges the composer
/// randomizes within per leg.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegStructureRecipe {
    pub variant: BuyVariant,
    #[serde(default)]
    pub slippage_bps_min: Option<u64>,
    #[serde(default)]
    pub slippage_bps_max: Option<u64>,
    #[serde(default)]
    pub cu_limit_min: Option<u32>,
    #[serde(default)]
    pub cu_limit_max: Option<u32>,
    #[serde(default)]
    pub cu_price_min: Option<u64>,
    #[serde(default)]
    pub cu_price_max: Option<u64>,
    #[serde(default)]
    pub tip_quote_min: Option<i64>,
    #[serde(default)]
    pub tip_quote_max: Option<i64>,
}

/// Persisted per-leg descriptor (matches `bundles.legs` JSON schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegStructure {
    pub variant: String,
    pub slippage_bps: u64,
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_quote: i64,
    pub ix_order: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundledLegPlan {
    pub wallet_id: Uuid,
    pub quote_amount: i64,
    pub structure: LegStructure,
}

/// Compose `leg_count` legs by drawing recipes from `pool` and pairing each with a
/// bundler wallet + quote amount. Pure (RNG for variation).
pub fn compose_bundle_legs(
    pool: &[LegStructureRecipe],
    leg_count: usize,
    wallets: &[Uuid],
    quote_per_leg: i64,
) -> Result<Vec<BundledLegPlan>> {
    if pool.is_empty() {
        bail!("leg_structures pool is empty");
    }
    if leg_count == 0 {
        return Ok(Vec::new());
    }
    if wallets.is_empty() {
        bail!("bundle needs at least one bundler wallet_id");
    }
    if quote_per_leg <= 0 {
        bail!("bundle quote_per_leg must be positive");
    }

    let mut rng = rand::thread_rng();
    let mut legs = Vec::with_capacity(leg_count);
    for i in 0..leg_count {
        let recipe = &pool[i % pool.len()];
        let wallet_id = wallets[i % wallets.len()];
        legs.push(BundledLegPlan {
            wallet_id,
            quote_amount: quote_per_leg,
            structure: materialize_leg(recipe, i as u8, &mut rng),
        });
    }
    Ok(legs)
}

/// Persisted leg row in `bundles.legs` JSONB.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoredBundleLeg {
    pub wallet_id: Uuid,
    pub quote_amount: i64,
    pub structure: LegStructure,
}

pub fn legs_from_json(value: &Json) -> Result<Vec<StoredBundleLeg>> {
    serde_json::from_value(value.clone()).context("parse bundles.legs")
}

pub fn leg_params(structure: &LegStructure) -> pump_trader::BundleLegParams {
    pump_trader::BundleLegParams {
        slippage_bps: structure.slippage_bps,
        cu_limit: structure.cu_limit,
        cu_price: structure.cu_price,
        tip_lamports: structure.tip_quote.max(0) as u64,
    }
}

pub fn legs_to_json(legs: &[BundledLegPlan]) -> Json {
    json!(legs
        .iter()
        .map(|leg| {
            json!({
                "wallet_id": leg.wallet_id,
                "quote_amount": leg.quote_amount,
                "structure": leg.structure,
            })
        })
        .collect::<Vec<_>>())
}

fn materialize_leg(recipe: &LegStructureRecipe, ix_order: u8, rng: &mut impl Rng) -> LegStructure {
    LegStructure {
        variant: recipe.variant.as_str().to_string(),
        slippage_bps: pick_u64(
            rng,
            recipe.slippage_bps_min.unwrap_or(300),
            recipe.slippage_bps_max.unwrap_or(800),
        ),
        cu_limit: pick_u32(
            rng,
            recipe.cu_limit_min.unwrap_or(120_000),
            recipe.cu_limit_max.unwrap_or(180_000),
        ),
        cu_price: pick_u64(
            rng,
            recipe.cu_price_min.unwrap_or(150_000),
            recipe.cu_price_max.unwrap_or(400_000),
        ),
        tip_quote: pick_i64(
            rng,
            recipe.tip_quote_min.unwrap_or(10_000),
            recipe.tip_quote_max.unwrap_or(100_000),
        ),
        ix_order,
    }
}

fn pick_u64(rng: &mut impl Rng, min: u64, max: u64) -> u64 {
    if max <= min {
        min
    } else {
        rng.gen_range(min..=max)
    }
}

fn pick_u32(rng: &mut impl Rng, min: u32, max: u32) -> u32 {
    if max <= min {
        min
    } else {
        rng.gen_range(min..=max)
    }
}

fn pick_i64(rng: &mut impl Rng, min: i64, max: i64) -> i64 {
    if max <= min {
        min
    } else {
        rng.gen_range(min..=max)
    }
}

/// Validate template bundle fields and return `(leg_count, wallets, quote_per_leg, tip)`.
pub fn parse_bundle_plan(params: &super::service::PumpfunTemplateParams) -> Result<Option<(usize, Vec<Uuid>, i64, Option<i64>)>> {
    let Some(leg_count) = params.bundle_leg_count.filter(|&n| n > 0) else {
        return Ok(None);
    };
    let wallets = params
        .bundle_wallet_ids
        .clone()
        .filter(|w| !w.is_empty())
        .context("bundle_leg_count set but bundle_wallet_ids is empty")?;
    let quote = params
        .bundle_quote_per_leg
        .filter(|&q| q > 0)
        .context("bundle_leg_count set but bundle_quote_per_leg missing")?;
    Ok(Some((leg_count as usize, wallets, quote, params.bundle_tip_quote)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pool() -> Vec<LegStructureRecipe> {
        vec![
            LegStructureRecipe {
                variant: BuyVariant::BuyExactSolIn,
                slippage_bps_min: Some(100),
                slippage_bps_max: Some(200),
                cu_limit_min: Some(100_000),
                cu_limit_max: Some(110_000),
                cu_price_min: Some(50_000),
                cu_price_max: Some(60_000),
                tip_quote_min: Some(1_000),
                tip_quote_max: Some(2_000),
            },
            LegStructureRecipe {
                variant: BuyVariant::BuyV2,
                slippage_bps_min: Some(300),
                slippage_bps_max: Some(400),
                cu_limit_min: None,
                cu_limit_max: None,
                cu_price_min: None,
                cu_price_max: None,
                tip_quote_min: None,
                tip_quote_max: None,
            },
        ]
    }

    #[test]
    fn compose_respects_ranges_and_cycles_wallets() {
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        let legs = compose_bundle_legs(&sample_pool(), 3, &[w1, w2], 50_000_000).unwrap();
        assert_eq!(legs.len(), 3);
        assert_eq!(legs[0].wallet_id, w1);
        assert_eq!(legs[1].wallet_id, w2);
        assert_eq!(legs[2].wallet_id, w1);
        assert!(legs[0].structure.slippage_bps >= 100 && legs[0].structure.slippage_bps <= 200);
        assert_eq!(legs[1].structure.variant, "buy_v2");
    }
}
