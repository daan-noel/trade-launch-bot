//! Bundle leg composer — per-leg structure descriptors (§3e).
//!
//! Draws audited buy variants + randomized budget/tip from a template pool.
//! Jito bundle **submission** is a later step; this module plans legs and persists
//! them to `bundles.legs`.

use anyhow::{Context, Result};
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

/// One composed bundle leg — also the exact persisted shape of a `bundles.legs`
/// JSONB row (write via [`legs_to_json`], read back via [`legs_from_json`]), so
/// the plan and its stored form are ONE type that can't drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundledLegPlan {
    /// FK → `managed_wallets.id`. `alias` keeps deserialization backward-compatible
    /// with `bundles.legs` rows written under the old `wallet_id` key.
    #[serde(alias = "wallet_id")]
    pub managed_wallet_id: Uuid,
    pub quote_amount: i64,
    pub structure: LegStructure,
}

/// Read the persisted `bundles.legs` JSONB back into the per-leg descriptors (the
/// confirm watcher + positions reconcile read the bundler wallet ids from here).
pub fn legs_from_json(value: &Json) -> Result<Vec<BundledLegPlan>> {
    serde_json::from_value(value.clone()).context("parse bundles.legs")
}

/// Serialize the per-leg descriptors to the `bundles.legs` JSONB shape. Phase 2.F:
/// these legs are a *projection* of the gated orchestrator `Plan` (see
/// `plan_pipeline::display_legs_json`) — the plan is the executable SSOT; `legs`
/// stays the display/confirm shape. `BundledLegPlan` derives `Serialize`, so this
/// IS what `legs_from_json` reads back — no hand-rolled field map to drift.
pub fn legs_to_json(legs: &[BundledLegPlan]) -> Json {
    json!(legs)
}

/// How many bundler legs to plan: the launch request's explicit override (the
/// Wallet Management "use N bundlers" control) if set, else the template's
/// default `bundle_leg_count`. `None`/`0` means no bundle.
pub fn resolve_leg_count(
    requested: Option<u32>,
    params: &super::service::PumpfunTemplateParams,
) -> Option<u32> {
    requested.or(params.bundle_leg_count).filter(|&n| n > 0)
}

/// Validate the template's static per-leg SOL amount + tip. Bundler *wallets* are
/// no longer template-configured — they come from `ManagedWalletRepo::claim_funded`
/// (wallet-pool Phase 3), a random atomic claim from the `funded` bundler pool
/// rather than a client-side pick.
pub fn resolve_bundle_quote(
    params: &super::service::PumpfunTemplateParams,
) -> Result<(i64, Option<i64>)> {
    let quote = params
        .bundle_quote_per_leg
        .filter(|&q| q > 0)
        .context("bundle requested but template bundle_quote_per_leg is missing")?;
    Ok((quote, params.bundle_tip_quote))
}

