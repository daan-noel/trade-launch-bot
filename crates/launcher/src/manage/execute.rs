//! Execute a management action: recompute the plan fresh, insert the audit row,
//! run each leg via pump-trader (sell/buy) or a plain RPC transfer (consolidate),
//! then reconcile positions from chain + feed.
//!
//! Legs run **sequentially** (a manual, cold path): each leg is signed by its own
//! wallet, so a per-wallet trader is stood up per leg — and sequential execution
//! keeps the shared durable-nonce accounts uncontended. Sells/buys are
//! RPC-confirmed; the "no new RPC on confirm" budget is a live-bot hot-path rule,
//! not this operator-triggered path.

use anyhow::{bail, Context, Result};
use platform_core::models::{ManageAction, ManageStatus, WalletRole};
use platform_core::storage::repositories::{ManageActionRepo, ManagedWalletRepo, TokenRepo};
use pump_trader::{protocol::LAMPORTS_PER_SOL, PumpFunTrader, TokenProgram};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::{info, warn};

use super::model::{ManageRequest, PlanLeg};
use super::plan::build_plan;
use super::positions::{load_positions, reconcile_positions};
use crate::config::{LauncherSettings, ManageConfig};
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

/// Max sell/buy attempts per leg (escalating Jito tip each try on sell).
const MAX_LEG_SELL_ATTEMPTS: u8 = 3;
/// Don't sweep a wallet holding less than this (not worth a signed tx + fee).
const CONSOLIDATE_MIN_LAMPORTS: u64 = 100_000; // 0.0001 SOL

/// Execute `req` against `mint`. Requires management to be enabled
/// (`settings.manage`); the HTTP handler also gates on it for a clean 503, but
/// this re-checks so the library can't fire trades with it off.
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

    // Seed + reconcile first so a sell plan sizes off FRESH balances.
    if let Err(e) = load_positions(pool, Some(settings), mint).await {
        warn!(%mint, ?e, "pre-plan position reconcile failed — planning off recorded balances");
    }

    let plan = build_plan(pool, mint, req).await?;
    if plan.legs.is_empty() {
        bail!("nothing to do: the plan produced no legs (no matching wallets)");
    }

    // Resolve any per-kind execution context ONCE (token creator/program for buy,
    // treasury + RPC for consolidate) so it isn't re-fetched per leg.
    let ctx = ExecContext::resolve(pool, settings, mint, &plan.kind).await?;

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

    let mut legs = plan.legs;
    let mut confirmed = 0i32;
    for leg in &mut legs {
        let outcome = match leg.side.as_str() {
            "sell" => sell_leg(pool, settings, manage_cfg, mint, leg).await,
            "buy" => buy_leg(pool, settings, manage_cfg, mint, leg, &ctx).await,
            "consolidate" => consolidate_leg(pool, settings, manage_cfg, leg, &ctx).await,
            other => Err(anyhow::anyhow!("unknown leg side '{other}'")),
        };
        match outcome {
            Ok(sig) => {
                leg.status = Some("confirmed".to_string());
                leg.signature = sig;
                confirmed += 1;
            }
            Err(e) => {
                leg.status = Some("failed".to_string());
                leg.error = Some(e.to_string());
                warn!(%mint, wallet_id = %leg.wallet_id, side = %leg.side, error = %e, "manage leg failed");
            }
        }
    }

    // Refresh positions from chain + feed (drained balances / new buyer holdings).
    if let Err(e) = reconcile_positions(pool, settings, mint).await {
        warn!(%mint, ?e, "post-action position reconcile failed");
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
        %mint, action_id = %action.id, kind = %plan.kind, status = status.as_str(),
        confirmed, total = legs_total, "manage action executed"
    );

    Ok(ManageAction {
        status: status.as_str().to_string(),
        legs_confirmed: confirmed,
        plan: serde_json::to_value(&legs)?,
        error,
        ..action
    })
}

/// Per-kind execution context resolved once before the leg loop.
struct ExecContext {
    /// Buy: the token's on-chain creator + token program (for `buy_token`).
    buy: Option<(Pubkey, TokenProgram)>,
    /// Consolidate: the treasury sink + an RPC client for plain SOL transfers.
    treasury: Option<Pubkey>,
    rpc: Option<RpcClient>,
}

impl ExecContext {
    async fn resolve(
        pool: &PgPool,
        settings: &LauncherSettings,
        mint: &str,
        kind: &str,
    ) -> Result<Self> {
        let mut ctx = ExecContext { buy: None, treasury: None, rpc: None };
        match kind {
            "buy" => {
                let token = TokenRepo::get(pool, mint)
                    .await?
                    .context("buy: token not found (not ingested / not our launch)")?;
                let creator = Pubkey::from_str(&token.creator_wallet)
                    .context("buy: parse token creator_wallet")?;
                let program = token
                    .token_program_id
                    .as_deref()
                    .map(TokenProgram::from_id)
                    .unwrap_or(TokenProgram::Legacy);
                ctx.buy = Some((creator, program));
            }
            "consolidate" => {
                let treasury = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
                    .await?
                    .into_iter()
                    .next()
                    .context("consolidate: no treasury wallet (role=treasury) configured")?;
                ctx.treasury =
                    Some(Pubkey::from_str(&treasury.address).context("parse treasury address")?);
                ctx.rpc = Some(RpcClient::new_with_commitment(
                    settings.rpc_url.clone(),
                    CommitmentConfig::confirmed(),
                ));
            }
            _ => {}
        }
        Ok(ctx)
    }
}

/// Build a pump-trader signed by `wallet_id`'s wallet (initialize included).
async fn build_wallet_trader(
    pool: &PgPool,
    settings: &LauncherSettings,
    wallet_id: uuid::Uuid,
) -> Result<(platform_core::models::ManagedWallet, PumpFunTrader)> {
    let wallet = ManagedWalletRepo::get(pool, wallet_id)
        .await?
        .context("leg wallet not found")?;
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
        .context("initialize pump-trader for manage action")?;
    Ok((wallet, trader))
}

/// SELL: sell `amount_base` tokens from the holder wallet, RPC-confirmed with an
/// escalating-tip retry. Returns the confirmed signature (`None` in dry-run).
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
    if manage_cfg.dry_run {
        info!(%mint, wallet_id = %leg.wallet_id, amount, "MANAGE_DRY_RUN: would sell (no trade placed)");
        return Ok(None);
    }

    let (_wallet, trader) = build_wallet_trader(pool, settings, leg.wallet_id).await?;

    // Own the retry loop so we capture the confirmed signature for the audit —
    // `sell_token` only returns a bool.
    let mut last_err = None;
    for attempt in 0..MAX_LEG_SELL_ATTEMPTS {
        match trader
            .sell_token_once(
                mint,
                amount,
                None,  // creator_override — self-heal reads it on revert
                false, // is_cashback — chain PDA flag covers it
                leg.token_account.as_deref(),
                Some(manage_cfg.sell_slippage_bps),
                attempt, // tip_level — escalate on retry
                true,    // RPC-confirm (cold manual path)
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

/// BUY: spend `spend_quote` lamports of SOL to buy the token from `wallet_id`.
/// RPC-confirmed; returns the signature (`None` in dry-run).
async fn buy_leg(
    pool: &PgPool,
    settings: &LauncherSettings,
    manage_cfg: &ManageConfig,
    mint: &str,
    leg: &PlanLeg,
    ctx: &ExecContext,
) -> Result<Option<String>> {
    let (creator, token_program) = ctx.buy.context("buy context not resolved")?;
    let spend = leg.spend_quote.max(0);
    if spend == 0 {
        bail!("buy spend is zero");
    }
    let sol_amount = spend as f64 / LAMPORTS_PER_SOL as f64;
    if manage_cfg.dry_run {
        info!(%mint, wallet_id = %leg.wallet_id, sol_amount, "MANAGE_DRY_RUN: would buy (no trade placed)");
        return Ok(None);
    }

    let mint_pk = Pubkey::from_str(mint).context("parse mint for buy")?;
    let (_wallet, trader) = build_wallet_trader(pool, settings, leg.wallet_id).await?;
    let sig = trader
        .buy_token(
            &mint_pk,
            &creator,
            token_program,
            sol_amount,
            Some(manage_cfg.sell_slippage_bps),
            false, // cashback — chain flag covers it
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Some(sig))
}

/// CONSOLIDATE: sweep the wallet's SOL (balance − fee) to treasury via a plain
/// RPC transfer (no Jito — no landing urgency), leaving the source at 0. Does NOT
/// retire the wallet (unlike the end-of-life dust sweep) — it stays a live
/// management wallet.
async fn consolidate_leg(
    pool: &PgPool,
    settings: &LauncherSettings,
    manage_cfg: &ManageConfig,
    leg: &PlanLeg,
    ctx: &ExecContext,
) -> Result<Option<String>> {
    let treasury = ctx.treasury.context("consolidate context not resolved")?;
    let rpc = ctx.rpc.as_ref().context("consolidate RPC not resolved")?;

    let wallet = ManagedWalletRepo::get(pool, leg.wallet_id)
        .await?
        .context("leg wallet not found")?;
    let address = Pubkey::from_str(&wallet.address).context("parse wallet address")?;
    let balance = rpc.get_balance(&address).await.context("fetch wallet balance")?;
    if balance <= CONSOLIDATE_MIN_LAMPORTS {
        bail!("balance {balance} below sweep floor — nothing to consolidate");
    }

    if manage_cfg.dry_run {
        info!(wallet = %wallet.address, balance, "MANAGE_DRY_RUN: would sweep to treasury (no transfer)");
        return Ok(None);
    }

    let blockhash = rpc.get_latest_blockhash().await.context("fetch blockhash")?;
    // Fixed-width amount ⇒ probe fee with any amount, then send balance − fee so
    // the source lands at exactly 0 (a sub-rent remainder would be rejected).
    let probe_ix = system_instruction::transfer(&address, &treasury, balance);
    let probe_msg = Message::new_with_blockhash(&[probe_ix], Some(&address), &blockhash);
    let fee = rpc
        .get_fee_for_message(&probe_msg)
        .await
        .context("fetch consolidate transfer fee")?;
    if balance <= fee {
        bail!("balance {balance} below tx fee {fee} — cannot consolidate");
    }
    let send_lamports = balance - fee;

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, &kek)?;
    let ix = system_instruction::transfer(&address, &treasury, send_lamports);
    let msg = Message::new_with_blockhash(&[ix], Some(&address), &blockhash);
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[signer.as_ref() as &dyn Signer], blockhash)
        .context("sign consolidate transfer")?;
    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .await
        .context("send consolidate transfer")?;
    info!(wallet = %wallet.address, swept_lamports = send_lamports, sig = %sig, "consolidated SOL to treasury");
    Ok(Some(sig.to_string()))
}
