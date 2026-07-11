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
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::{info, warn};

use super::model::{ManageRequest, PlanLeg};
use super::plan::build_plan;
use super::positions::{load_positions, reconcile_positions};
use crate::config::{LauncherSettings, ManageConfig};
use crate::keystore::{self, EnvKek};
use crate::plan_exec::TransferMode;
use crate::trader_config::build_launch_trader_config;

/// Max sell/buy attempts per leg (escalating Jito tip each try on sell).
const MAX_LEG_SELL_ATTEMPTS: u8 = 3;
/// Don't sweep a wallet holding less than this (not worth a signed tx + fee).
const CONSOLIDATE_MIN_LAMPORTS: u64 = 100_000; // 0.0001 SOL

/// A management sell must pay its own base fee + escalating Jito tip across
/// [`MAX_LEG_SELL_ATTEMPTS`] (each attempt is a separate tx; the tip caps at
/// 0.005 SOL). Below this floor the wallet is topped up from treasury before the
/// sell — otherwise a wallet the dust sweep already drained (gas swept while it
/// still held tokens) has 0 lamports and the sell can only ever time out.
const SELL_GAS_FLOOR_LAMPORTS: u64 = 6_000_000; // 0.006 SOL
/// Top a low sell wallet up to this from treasury — headroom for three
/// escalating-tip attempts (≈3 × 0.005 tip + fees) with margin.
const SELL_GAS_TOPUP_TARGET_LAMPORTS: u64 = 20_000_000; // 0.02 SOL

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

    // Phase 2.F: build the action as an orchestrator `Plan` (sell → Exit sell,
    // buy → Accumulate buy, consolidate → typed `TransferSol`) and run the
    // mandatory fingerprint-audit gate BEFORE anything executes. The legacy
    // `PlanLeg[]` still drives the (proven) per-leg execution below; the gated plan
    // is the audited, persisted SSOT — a hard-reject here aborts the whole action.
    let orch_plan =
        build_orchestrator_plan(pool, mint, &plan.legs, &ctx, manage_cfg.sell_slippage_bps).await?;
    let gated = crate::plan_pipeline::gate(orch_plan, settings.allow_fingerprint)
        .context("manage action failed the mandatory plan/audit gate")?;

    let selection_json = serde_json::to_value(&req.selection)?;
    let legs_total = plan.legs.len() as i32;
    let action = ManageActionRepo::insert_executing(
        pool,
        mint,
        &plan.kind,
        &plan.sizing,
        selection_json,
        serde_json::to_value(&plan.legs)?,
        gated.plan_json(),
        gated.audit_json(),
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
                warn!(%mint, managed_wallet_id = %leg.managed_wallet_id, side = %leg.side, error = %e, "manage leg failed");
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

/// Ensure a sell wallet holds enough SOL to pay the sell's fee + escalating Jito
/// tip, topping it up from treasury when it's below [`SELL_GAS_FLOOR_LAMPORTS`].
///
/// The dust sweep can sweep a token-holding wallet's SOL to treasury (now guarded
/// against, but wallets stranded before that guard shipped stay dry), leaving a
/// 0-lamport fee-payer whose sell can only time out. This tops the wallet back up
/// from treasury (net-neutral: it reclaims the gas the sweep moved there in the
/// first place). Fail-fast: if a top-up is needed but can't be sent (no treasury /
/// underfunded), return an actionable error so the leg fails immediately with a
/// clear cause instead of the opaque "confirmation timed out" after ~3 min of
/// retries. A wallet already at/above the floor is a no-op (one balance RPC).
async fn ensure_sell_gas(
    pool: &PgPool,
    settings: &LauncherSettings,
    wallet_address: &str,
) -> Result<()> {
    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());
    let addr = Pubkey::from_str(wallet_address).context("parse sell wallet address")?;
    let balance = rpc.get_balance(&addr).await.context("fetch sell wallet balance")?;
    if balance >= SELL_GAS_FLOOR_LAMPORTS {
        return Ok(());
    }

    // Source the top-up from the best-funded live treasury (`by_role` already
    // excludes retired). No treasury ⇒ we can't re-gas; say so plainly.
    let treasury = ManagedWalletRepo::by_role(pool, WalletRole::Treasury.as_str())
        .await?
        .into_iter()
        .max_by_key(|w| w.balance_lamports)
        .with_context(|| {
            format!(
                "sell wallet {wallet_address} has {balance} lamports (below the \
                 {SELL_GAS_FLOOR_LAMPORTS} gas floor) and no treasury wallet (role=treasury) \
                 is configured to top it up — fund the wallet manually and retry"
            )
        })?;
    let treasury_addr = Pubkey::from_str(&treasury.address).context("parse treasury address")?;
    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let treasury_signer = keystore::resolve_signer(&settings.keystore_dir, &treasury.key_ref, &kek)?;

    let topup = SELL_GAS_TOPUP_TARGET_LAMPORTS.saturating_sub(balance);
    crate::plan_exec::execute_transfer(
        &rpc,
        treasury_signer.as_ref(),
        treasury_addr,
        addr,
        TransferMode::Exact(topup),
        true, // confirm — the sell that immediately follows needs the SOL on-chain
    )
    .await
    .with_context(|| {
        format!(
            "gas top-up of {topup} lamports from treasury {} to sell wallet {wallet_address} \
             failed (treasury underfunded?) — fund the wallet manually and retry",
            treasury.address
        )
    })?;
    info!(%wallet_address, balance, topup, treasury = %treasury.address, "topped up sell wallet gas from treasury");
    Ok(())
}

/// Build a pump-trader signed by `managed_wallet_id`'s wallet (initialize included).
async fn build_wallet_trader(
    pool: &PgPool,
    settings: &LauncherSettings,
    managed_wallet_id: uuid::Uuid,
) -> Result<(platform_core::models::ManagedWallet, PumpFunTrader)> {
    let wallet = ManagedWalletRepo::get(pool, managed_wallet_id)
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
        info!(%mint, managed_wallet_id = %leg.managed_wallet_id, amount, "MANAGE_DRY_RUN: would sell (no trade placed)");
        return Ok(None);
    }

    // Re-gas the wallet from treasury if it's short — the dust sweep can drain a
    // token-holding wallet's SOL to treasury, and a 0-lamport fee-payer's sell just
    // times out. Fails fast with an actionable error if a top-up is needed but
    // can't be sent, rather than burning the retry budget on a doomed send.
    ensure_sell_gas(pool, settings, &leg.wallet_address).await?;

    let (_wallet, trader) = build_wallet_trader(pool, settings, leg.managed_wallet_id).await?;

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

/// BUY: spend `spend_quote` lamports of SOL to buy the token from `managed_wallet_id`.
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
        info!(%mint, managed_wallet_id = %leg.managed_wallet_id, sol_amount, "MANAGE_DRY_RUN: would buy (no trade placed)");
        return Ok(None);
    }

    let mint_pk = Pubkey::from_str(mint).context("parse mint for buy")?;
    let (_wallet, trader) = build_wallet_trader(pool, settings, leg.managed_wallet_id).await?;
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

/// CONSOLIDATE: sweep the wallet's SOL (balance − fee) to treasury as a typed
/// `TransferSol` op, executed via the SSOT [`crate::plan_exec::execute_transfer`]
/// (a plain transfer — no Jito, no landing urgency), leaving the source at 0. Does
/// NOT retire the wallet (unlike the end-of-life dust sweep) — it stays a live
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

    let wallet = ManagedWalletRepo::get(pool, leg.managed_wallet_id)
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

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, &kek)?;
    let (sig, _lamports) = crate::plan_exec::execute_transfer(
        rpc,
        signer.as_ref(),
        address,
        treasury,
        TransferMode::SweepAll { min_lamports: CONSOLIDATE_MIN_LAMPORTS },
        true, // confirm — cold manual path
    )
    .await?
    .context("consolidate: balance fell below the tx fee — nothing swept")?;
    info!(wallet = %wallet.address, sig = %sig, "consolidated SOL to treasury");
    Ok(Some(sig.to_string()))
}

/// Build the orchestrator [`Plan`](orchestrator::Plan) for a management action from
/// its computed legs — the audited + persisted SSOT. Resolves each leg wallet's
/// pubkey (the op needs it to validate) and maps by side: sell → `Sell`/`Exit`,
/// buy → `Buy`/`Accumulate` (SOL-in), consolidate → typed `TransferSol` to treasury.
async fn build_orchestrator_plan(
    pool: &PgPool,
    mint: &str,
    legs: &[PlanLeg],
    ctx: &ExecContext,
    slippage_bps: u64,
) -> Result<orchestrator::Plan> {
    use orchestrator::{Amount, IdSeq, Intent, Operation, Plan, Role, WalletRef};

    let ids: Vec<uuid::Uuid> = legs.iter().map(|l| l.managed_wallet_id).collect();
    let wallets = ManagedWalletRepo::get_many(pool, &ids).await?;
    let addr_of = |id: uuid::Uuid| wallets.iter().find(|w| w.id == id).map(|w| w.address.clone());
    let treasury = ctx.treasury.map(|p| p.to_string());

    let mut seq = IdSeq::new(0);
    let mut ops = Vec::with_capacity(legs.len());
    for leg in legs {
        let pubkey = addr_of(leg.managed_wallet_id)
            .with_context(|| format!("manage plan: wallet {} not found", leg.managed_wallet_id))?;
        let wref = WalletRef::managed(leg.managed_wallet_id, pubkey);
        let role = Role::from_str_lossy(&leg.role);
        let op = match leg.side.as_str() {
            "sell" => Operation::sell(
                seq.next(),
                "sell",
                role,
                wref,
                mint.to_string(),
                leg.amount_base.max(0) as u64,
                Some(slippage_bps),
                vec![],
            ),
            "buy" => Operation::buy(
                seq.next(),
                crate::plan_pipeline::LAUNCH_BUY_VARIANT,
                role,
                Intent::Accumulate,
                wref,
                mint.to_string(),
                Amount::ExactQuote(leg.spend_quote.max(0) as u64),
                Some(slippage_bps),
                vec![],
            ),
            "consolidate" => {
                let to = treasury
                    .clone()
                    .context("consolidate plan needs a resolved treasury target")?;
                Operation::transfer_sol_as(
                    seq.next(),
                    orchestrator::macros::DEFAULT_SOL_TRANSFER_VARIANT,
                    role,
                    Intent::Consolidate,
                    wref,
                    to,
                    leg.spend_quote.max(0) as u64,
                    vec![],
                )
            }
            other => bail!("unknown leg side '{other}'"),
        };
        ops.push(op);
    }
    Ok(Plan::for_mint(mint.to_string(), ops))
}
