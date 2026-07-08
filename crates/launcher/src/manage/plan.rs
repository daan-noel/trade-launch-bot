//! Turn a [`ManageRequest`] into a previewable [`ActionPlan`] — pure DB reads, no
//! chain writes. Phase 2 supports `sell` + `pct_of_holdings` only; the other
//! kinds/sizings return a clear error until their phase wires them.

use anyhow::{bail, Result};
use platform_core::models::{ManageKind, ManageSizing, PositionStatus};
use platform_core::storage::repositories::{TokenMarketStateRepo, TokenPositionRepo};
use sqlx::PgPool;

use super::model::{ActionPlan, ManageRequest, PlanLeg};

/// Compute the plan for `mint` from the request against the current positions +
/// spot price. Skips wallets with nothing to sell (0 balance, or a rounded-down 0
/// leg). Never places a trade — the executor does that from a fresh recompute.
pub async fn build_plan(pool: &PgPool, mint: &str, req: &ManageRequest) -> Result<ActionPlan> {
    let kind: ManageKind = req.kind.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let sizing: ManageSizing = req.sizing.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    // Phase 2 surface: only the sell primitive with percent-of-holdings sizing.
    if kind != ManageKind::Sell {
        bail!("manage kind '{}' is not implemented yet (Phase 2 supports sell)", req.kind);
    }
    if sizing != ManageSizing::PctOfHoldings {
        bail!(
            "manage sizing '{}' is not implemented yet (Phase 2 supports pct_of_holdings)",
            req.sizing
        );
    }

    let pct = req.size;
    if !(0.0..=100.0).contains(&pct) {
        bail!("pct_of_holdings size must be 0–100 (got {pct})");
    }

    // Current spot price (raw ratio: quote base units per token base unit) for the
    // preview's estimated proceeds. `None` (never traded) ⇒ est 0, not an error.
    let price = TokenMarketStateRepo::get(pool, mint)
        .await?
        .and_then(|s| s.current_price_quote);

    let positions = TokenPositionRepo::by_mint(pool, mint).await?;
    let mut legs = Vec::new();
    for p in positions {
        if p.status != PositionStatus::Open.as_str() || p.balance_base <= 0 {
            continue;
        }
        if !req.selection.matches(p.wallet_id, &p.role) {
            continue;
        }
        // Round the sell amount down so we never try to sell more than held.
        let amount_base = ((p.balance_base as f64) * pct / 100.0).floor() as i64;
        if amount_base <= 0 {
            continue;
        }
        let est_quote = price
            .map(|pr| (amount_base as f64 * pr).max(0.0) as i64)
            .unwrap_or(0);
        legs.push(PlanLeg {
            wallet_id: p.wallet_id,
            role: p.role,
            token_account: p.token_account,
            side: "sell".to_string(),
            amount_base,
            est_quote,
            status: None,
            signature: None,
            error: None,
        });
    }

    Ok(ActionPlan::from_legs(mint, kind.as_str(), sizing.as_str(), legs))
}
