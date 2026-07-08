//! Execute a management action: recompute the plan fresh, insert the audit row,
//! run each leg via pump-trader, then reconcile positions from chain + feed.
//!
//! Phase 2 = the Sell primitive. Legs run **sequentially** (a manual, cold path):
//! each sell is signed by its own holder wallet, so a per-wallet trader is stood
//! up per leg — and sequential execution keeps the shared durable-nonce accounts
//! uncontended. Each sell is RPC-confirmed (`sell_token_once(confirm=true)`); the
//! "no new RPC on sell-confirm" budget is a live-bot hot-path rule, not this
//! operator-triggered path.

use anyhow::{bail, Context, Result};
use platform_core::models::{ManageAction, ManageStatus};
use platform_core::storage::repositories::{ManageActionRepo, ManagedWalletRepo};
use pump_trader::PumpFunTrader;
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::{info, warn};

use super::model::{ManageRequest, PlanLeg};
use super::plan::build_plan;
use super::positions::{load_positions, reconcile_positions};
use crate::config::{LauncherSettings, ManageConfig};
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

/// Max sell attempts per leg (escalating Jito tip each try). Small — a manual
/// action, not the live bot's aggressive retry ladder.
const MAX_LEG_SELL_ATTEMPTS: u8 = 3;

/// Execute `req` against `mint`. Requires management to be enabled
/// (`settings.manage`); the caller (HTTP handler) also gates on it for a clean
/// 503, but this re-checks so the library can't fire trades with it off.
///
/// Recomputes the plan at execute time (positions may have moved since preview),
/// records it as an audit row, runs the legs, and returns the finalized row.
pub async fn execute_action(
    pool: &PgPool,
    settings: &LauncherSettings,
    mint: &str,
    req: &ManageRequest,
) -> Result<ManageAction> {
    let manage_cfg = settings
        .manage
        .as_ref()
        .context("token management disabled (set MANAGE_ENABLED=true)")?;

    // Seed + reconcile positions first so the plan sizes off FRESH on-chain
    // balances (critical for "sell 100%"), not a stale seed. Best-effort: a
    // reconcile failure still lets us plan off whatever's recorded.
    if let Err(e) = load_positions(pool, Some(settings), mint).await {
        warn!(%mint, ?e, "pre-plan position reconcile failed — planning off recorded balances");
    }

    let plan = build_plan(pool, mint, req).await?;
    if plan.legs.is_empty() {
        bail!("nothing to do: the plan produced no legs (no matching wallets with a balance)");
    }

    let selection_json = serde_json::to_value(&req.selection)?;
    let legs_total = plan.legs.len() as i32;
    let action = ManageActionRepo::insert_executing(
        pool,
        mint,
        &plan.kind,
        &plan.sizing,
        selection_json,
        serde_json::to_value(&plan.legs)?,
        legs_total,
    )
    .await?;

    // Run legs sequentially, recording each outcome onto its leg for the audit.
    let mut legs = plan.legs;
    let mut confirmed = 0i32;
    for leg in &mut legs {
        match sell_leg(pool, settings, manage_cfg, mint, leg).await {
            Ok(sig) => {
                leg.status = Some("confirmed".to_string());
                leg.signature = sig;
                confirmed += 1;
            }
            Err(e) => {
                leg.status = Some("failed".to_string());
                leg.error = Some(e.to_string());
                warn!(%mint, wallet_id = %leg.wallet_id, error = %e, "manage sell leg failed");
            }
        }
    }

    // Refresh positions from chain + feed (balance drained, realized proceeds).
    // Best-effort — a reconcile hiccup must not mask a completed sell.
    if let Err(e) = reconcile_positions(pool, settings, mint).await {
        warn!(%mint, ?e, "post-sell position reconcile failed");
    }

    let status = if confirmed == legs_total {
        ManageStatus::Completed
    } else if confirmed > 0 {
        ManageStatus::Partial
    } else {
        ManageStatus::Failed
    };
    let error = (status == ManageStatus::Failed).then(|| "all legs failed".to_string());

    ManageActionRepo::set_result(
        pool,
        action.id,
        status.as_str(),
        confirmed,
        serde_json::to_value(&legs)?,
        error.as_deref(),
    )
    .await?;

    info!(
        %mint, action_id = %action.id, status = status.as_str(),
        confirmed, total = legs_total, "manage action executed"
    );

    // Return the finalized row.
    Ok(ManageAction {
        status: status.as_str().to_string(),
        legs_confirmed: confirmed,
        plan: serde_json::to_value(&legs)?,
        error,
        ..action
    })
}

/// Sell one leg: build a per-wallet trader (signed by the holder) and sell
/// `amount_base` tokens, RPC-confirmed with an escalating-tip retry. Returns the
/// confirmed signature (`None` in dry-run). `MANAGE_DRY_RUN` places no trade.
async fn sell_leg(
    pool: &PgPool,
    settings: &LauncherSettings,
    manage_cfg: &ManageConfig,
    mint: &str,
    leg: &PlanLeg,
) -> Result<Option<String>> {
    let amount = leg.amount_base.max(0) as u64;
    if amount == 0 {
        bail!("leg amount is zero");
    }

    let wallet = ManagedWalletRepo::get(pool, leg.wallet_id)
        .await?
        .context("leg wallet not found")?;

    if manage_cfg.dry_run {
        info!(
            %mint, wallet = %wallet.address, amount,
            "MANAGE_DRY_RUN: would sell (no trade placed)"
        );
        return Ok(None);
    }

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, &kek)?;
    let nonce_accounts: Vec<Pubkey> = settings
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;

    let config = build_launch_trader_config(settings, signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(config);
    trader
        .initialize()
        .await
        .context("initialize pump-trader for manage sell")?;

    // Own the retry loop (escalating tip, fresh nonce each attempt) so we capture
    // the confirmed signature for the audit — `sell_token` only returns a bool.
    let mut last_err = None;
    for attempt in 0..MAX_LEG_SELL_ATTEMPTS {
        match trader
            .sell_token_once(
                mint,
                amount,
                None,                        // creator_override — self-heal reads it on revert
                false,                       // is_cashback — chain PDA flag covers it
                leg.token_account.as_deref(),
                Some(manage_cfg.sell_slippage_bps),
                attempt,                     // tip_level — escalate on retry
                true,                        // confirm via RPC (cold manual path)
            )
            .await
        {
            Ok(Some(sig)) => return Ok(Some(sig)),
            Ok(None) => last_err = Some(anyhow::anyhow!("sell did not submit (attempt {})", attempt + 1)),
            Err(e) => last_err = Some(anyhow::anyhow!("{e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("sell failed after {MAX_LEG_SELL_ATTEMPTS} attempts")))
}
