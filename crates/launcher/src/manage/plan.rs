//! Turn a [`ManageRequest`] into a previewable [`ActionPlan`] — pure DB reads, no
//! chain writes.
//!
//! - `sell` + `pct_of_holdings` — per-holder, sized off the position balance.
//! - `buy` + `fixed_sol` — per selected managed wallet (may be a fresh buyer that
//!   holds nothing yet), a fixed SOL spend each.
//! - `consolidate` + `sweep` — per selected wallet, sweep its SOL to treasury.

use anyhow::{bail, Result};
use platform_core::models::{ManageKind, ManageSizing, ManagedWallet, PositionStatus};
use platform_core::storage::repositories::{
    ManagedWalletRepo, TokenMarketStateRepo, TokenPositionRepo,
};
use pump_trader::protocol::LAMPORTS_PER_SOL;
use sqlx::PgPool;

use super::model::{ActionPlan, ManageRequest, PlanLeg, WalletSelection};

/// Compute the plan for `mint` from the request against current DB state. Skips
/// wallets with nothing to do. Never places a trade — the executor recomputes.
pub async fn build_plan(pool: &PgPool, mint: &str, req: &ManageRequest) -> Result<ActionPlan> {
    let kind: ManageKind = req.kind.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let sizing: ManageSizing = req.sizing.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    let legs = match kind {
        ManageKind::Sell => build_sell_legs(pool, mint, req, sizing).await?,
        ManageKind::Buy => build_buy_legs(pool, mint, req, sizing).await?,
        ManageKind::Consolidate => build_consolidate_legs(pool, mint, req, sizing).await?,
    };

    Ok(ActionPlan::from_legs(mint, kind.as_str(), sizing.as_str(), legs))
}

/// SELL: percent of each selected holder's balance.
async fn build_sell_legs(
    pool: &PgPool,
    mint: &str,
    req: &ManageRequest,
    sizing: ManageSizing,
) -> Result<Vec<PlanLeg>> {
    if sizing != ManageSizing::PctOfHoldings {
        bail!("sell supports sizing 'pct_of_holdings' (got '{}')", req.sizing);
    }
    let pct = req.size;
    if !(0.0..=100.0).contains(&pct) {
        bail!("pct_of_holdings size must be 0–100 (got {pct})");
    }

    // Current spot price (raw ratio) for the preview's estimated proceeds.
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
            spend_quote: 0,
            est_quote,
            status: None,
            signature: None,
            error: None,
        });
    }
    Ok(legs)
}

/// BUY: a fixed SOL spend per selected managed wallet (fresh buyers welcome).
async fn build_buy_legs(
    pool: &PgPool,
    _mint: &str,
    req: &ManageRequest,
    sizing: ManageSizing,
) -> Result<Vec<PlanLeg>> {
    if sizing != ManageSizing::FixedSol {
        bail!("buy supports sizing 'fixed_sol' (got '{}')", req.sizing);
    }
    let sol = req.size;
    if sol <= 0.0 {
        bail!("fixed_sol size (SOL per wallet) must be positive (got {sol})");
    }
    let spend_quote = (sol * LAMPORTS_PER_SOL as f64).round() as i64;

    let wallets = resolve_selection_wallets(pool, &req.selection, "buy").await?;
    Ok(wallets
        .into_iter()
        .map(|w| PlanLeg {
            wallet_id: w.id,
            role: w.role,
            token_account: None,
            side: "buy".to_string(),
            amount_base: 0,
            spend_quote,
            est_quote: 0,
            status: None,
            signature: None,
            error: None,
        })
        .collect())
}

/// CONSOLIDATE: sweep each selected wallet's SOL to treasury. Preview shows the
/// cached balance as the sweep estimate; the exact amount (balance − fee) is
/// computed at execute.
async fn build_consolidate_legs(
    pool: &PgPool,
    _mint: &str,
    req: &ManageRequest,
    sizing: ManageSizing,
) -> Result<Vec<PlanLeg>> {
    if sizing != ManageSizing::Sweep {
        bail!("consolidate supports sizing 'sweep' (got '{}')", req.sizing);
    }
    let wallets = resolve_selection_wallets(pool, &req.selection, "consolidate").await?;
    Ok(wallets
        .into_iter()
        // Never sweep a treasury wallet into itself.
        .filter(|w| w.role != platform_core::models::WalletRole::Treasury.as_str())
        .map(|w| PlanLeg {
            wallet_id: w.id,
            role: w.role,
            token_account: None,
            side: "consolidate".to_string(),
            amount_base: 0,
            spend_quote: w.balance_lamports.unwrap_or(0).max(0),
            est_quote: 0,
            status: None,
            signature: None,
            error: None,
        })
        .collect())
}

/// Resolve the managed wallets a buy/consolidate targets: explicit `wallet_ids`
/// win, else a `role`. Both unset is refused (these primitives don't have a
/// natural "all wallets" — that would be dangerously broad).
async fn resolve_selection_wallets(
    pool: &PgPool,
    selection: &WalletSelection,
    action: &str,
) -> Result<Vec<ManagedWallet>> {
    if !selection.wallet_ids.is_empty() {
        return ManagedWalletRepo::get_many(pool, &selection.wallet_ids).await;
    }
    match &selection.role {
        Some(role) => ManagedWalletRepo::by_role(pool, role).await,
        None => bail!("{action} requires a wallet role or explicit wallet_ids"),
    }
}
