//! Execute a planned bundle: build signed buy txs → Jito `sendBundle`.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::models::{BundleStatus, ManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenRepo,
};
use pump_trader::types::TokenProgram;
use pump_trader::{PumpFunTrader};
use serde::Serialize;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;
use sqlx::PgPool;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::plan_pipeline::{self, is_bundler_leg};
use crate::trader_config::build_launch_trader_config;

#[derive(Debug, Clone, Serialize)]
pub struct BundleExecuteResult {
    pub bundle_id: Uuid,
    pub jito_bundle_id: String,
    pub leg_signatures: Vec<String>,
}

/// Build + submit a `planned` bundle's buy legs to Jito, standing up a fresh
/// trader from the first leg's bundler wallet. Standalone retry entrypoint
/// (`POST /bundles/:id/execute`).
pub async fn execute_bundle(
    pool: &PgPool,
    settings: &LauncherSettings,
    bundle_id: Uuid,
) -> Result<BundleExecuteResult> {
    execute_bundle_inner(pool, settings, bundle_id, None).await
}

/// Same as [`execute_bundle`], but reuse a trader the caller already built and
/// initialized (the launch's create trader). Every leg is signed by its own
/// bundler wallet, and the cached global account is wallet-independent for the
/// fields the legs read (`build_bundle_leg_tx` re-derives each leg's
/// `user_volume_accumulator` from that leg's signer), so the trader's own signer
/// identity is irrelevant here. This avoids standing up a second trader — a full
/// `initialize()` (≈8-12 RPC/HTTP round trips plus two background refresher
/// tasks) — for the auto-submit that already follows a create in the same request.
pub(crate) async fn execute_bundle_with_trader(
    pool: &PgPool,
    settings: &LauncherSettings,
    trader: &PumpFunTrader,
    bundle_id: Uuid,
) -> Result<BundleExecuteResult> {
    execute_bundle_inner(pool, settings, bundle_id, Some(trader)).await
}

async fn execute_bundle_inner(
    pool: &PgPool,
    settings: &LauncherSettings,
    bundle_id: Uuid,
    existing_trader: Option<&PumpFunTrader>,
) -> Result<BundleExecuteResult> {
    let bundle = BundleRepo::get(pool, bundle_id)
        .await?
        .context("bundle not found")?;
    if bundle.status != BundleStatus::Planned.as_str() {
        bail!(
            "bundle {} is status={}, expected planned",
            bundle_id,
            bundle.status
        );
    }

    let launch = LaunchRepo::get(pool, bundle.launch_id)
        .await?
        .context("launch not found for bundle")?;
    if launch.create_signature.is_none() {
        bail!("launch must be created on-chain before bundle execute");
    }

    let token = TokenRepo::get(pool, &launch.mint_address)
        .await?
        .context("token row not found")?;

    // Phase 2.F: the bundle's executable SSOT is the persisted orchestrator `Plan`.
    // Deserialize it and re-run the mandatory gate (which recomputes the identical
    // deterministic disguises and re-audits) — a bundle planned before the cutover
    // has no plan and can't be executed on this path.
    let plan: orchestrator::Plan = serde_json::from_value(
        bundle
            .plan
            .clone()
            .context("bundle predates the Plan cutover (no persisted plan) — cannot execute")?,
    )
    .context("deserialize persisted bundle plan")?;
    let gated = plan_pipeline::gate(plan, settings.allow_fingerprint)
        .context("persisted bundle plan failed the mandatory plan/audit gate")?;
    let bundler_ops: Vec<&orchestrator::Operation> =
        gated.plan.ops.iter().filter(|op| is_bundler_leg(op)).collect();
    if bundler_ops.is_empty() {
        bail!("bundle plan has no bundler legs");
    }

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let first_wallet_id = bundler_ops[0]
        .wallet
        .managed_wallet_id
        .context("bundler leg op has no managed wallet id")?;
    let first_wallet = ManagedWalletRepo::get(pool, first_wallet_id)
        .await?
        .context("bundler wallet not found")?;
    check_wallet_reserved_to_bundle(&first_wallet, bundle.launch_id)?;

    // Reuse the caller's already-initialized trader when given one (the launch's
    // create trader); otherwise stand up a fresh one from the first bundler
    // wallet (the standalone retry path).
    let built_trader;
    let trader: &PumpFunTrader = match existing_trader {
        Some(t) => t,
        None => {
            let primary_signer = keystore::resolve_signer(
                &settings.keystore_dir,
                &first_wallet.key_ref,
                &kek,
            )?;
            let nonce_accounts: Vec<Pubkey> = settings
                .nonce_accounts
                .iter()
                .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
                .collect::<Result<_>>()?;
            let trader_config = build_launch_trader_config(settings, primary_signer, nonce_accounts);
            let mut t = PumpFunTrader::new(trader_config);
            t.initialize().await.context("initialize pump-trader")?;
            built_trader = t;
            &built_trader
        }
    };

    let mint = Pubkey::from_str(&launch.mint_address).context("parse mint")?;
    let creator = Pubkey::from_str(&token.creator_wallet).context("parse creator")?;
    let token_program_id = token
        .token_program_id
        .as_deref()
        .unwrap_or_else(|| keystore::token_program_for_variant(&launch.variant));
    let token_program = TokenProgram::from_id(token_program_id);
    let cashback_enabled = token
        .meta
        .get("cashback_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let blockhash = trader.fresh_blockhash().await?;

    // Size the bundle's total Jito tip to the live landed-tip floor at this
    // submission's escalation level. `submit_attempts` is the persisted level:
    // 0 on the first attempt, and a re-bid after a `dropped` verdict re-enters
    // here with it incremented (see `launcher::confirm`), so it bids ABOVE the
    // tip that just lost the auction (level 1 = p95, 2 = p99, …). The total is
    // split evenly across legs; `leg_params` only ever RAISES a persona tip draw
    // to this floor, never lowers it — so persona jitter is kept when it already
    // clears the auction, and the tip is bid up to the live floor when it doesn't.
    let level = bundle.submit_attempts.max(0) as u8;
    let total_tip_floor = trader.jito_tip_floor_lamports(level);
    let per_leg_tip_floor = total_tip_floor.div_ceil(bundler_ops.len() as u64);

    // Atomic compare-and-swap `planned → submitting`. The read-check above is a
    // fast-fail for an obviously-wrong status, but ~8-12 RPC round trips (gate,
    // trader init, blockhash, tip floor) run between it and here — two concurrent
    // executes can both pass that check. This conditional UPDATE is the real gate:
    // exactly one caller flips the row, and the loser aborts BEFORE signing or
    // submitting, so the same bundler wallets can't double-submit real-SOL buys.
    if !BundleRepo::claim_for_submitting(pool, bundle_id).await? {
        bail!(
            "bundle {bundle_id} was concurrently claimed for submission \
             (status is no longer planned) — aborting to avoid a double submit"
        );
    }

    let finish = async {
        let mut txs = Vec::with_capacity(bundler_ops.len());
        let mut leg_signatures = Vec::with_capacity(bundler_ops.len());

        for op in &bundler_ops {
            let managed_wallet_id = op
                .wallet
                .managed_wallet_id
                .context("bundler leg op has no managed wallet id")?;
            let disguise = gated
                .disguise_of(op.id)
                .context("no disguise for bundler leg op")?;
            let wallet = ManagedWalletRepo::get(pool, managed_wallet_id)
                .await?
                .context("bundler wallet not found")?;
            check_wallet_reserved_to_bundle(&wallet, bundle.launch_id)?;
            let signer =
                keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, &kek)?;
            let tx = crate::plan_exec::build_leg_tx(
                trader,
                signer.as_ref(),
                blockhash,
                &mint,
                &creator,
                token_program,
                cashback_enabled,
                op,
                disguise,
                per_leg_tip_floor,
            )
            .await?;
            leg_signatures.push(tx.signatures[0].to_string());
            txs.push(tx);
        }

        let jito_bundle_id = submit_jito_bundle(&settings.jito_block_engine_url, &txs).await?;
        BundleRepo::set_submitted(pool, bundle_id, &jito_bundle_id, &leg_signatures).await?;

        // Wallet-pool lifecycle: bundler wallets stay `reserved` here — submit
        // isn't completion. `launcher::confirm` transitions them to `used` once
        // the bundle reaches a terminal landed/dropped/partial outcome.

        info!(
            bundle_id = %bundle_id,
            launch_id = %bundle.launch_id,
            legs = bundler_ops.len(),
            jito_bundle_id = %jito_bundle_id,
            tip_level = level,
            total_tip_floor_lamports = total_tip_floor,
            "bundle submitted to Jito"
        );

        Ok(BundleExecuteResult {
            bundle_id,
            jito_bundle_id,
            leg_signatures,
        })
    }
    .await;

    if finish.is_err() {
        if let Err(mark_err) =
            BundleRepo::set_status(pool, bundle_id, BundleStatus::Failed.as_str()).await
        {
            tracing::warn!(%bundle_id, %mark_err, "failed to mark bundle failed");
        }
    }

    finish
}

/// Guards against spending from a wallet whose reservation lapsed (TTL sweep)
/// or was claimed by another launch since this bundle was planned — e.g. a
/// stale `planned` bundle (process crashed before it reached `submitting`)
/// manually retried via `POST /bundles/:id/execute` long after its wallets
/// were released and re-claimed elsewhere.
fn check_wallet_reserved_to_bundle(wallet: &ManagedWallet, launch_id: Uuid) -> Result<()> {
    if wallet.role != WalletRole::Bundler.as_str() {
        bail!(
            "wallet {} is role={}, expected bundler",
            wallet.id,
            wallet.role
        );
    }
    if wallet.status != WalletStatus::Reserved.as_str()
        || wallet.reserved_by_launch_id != Some(launch_id)
    {
        bail!(
            "wallet {} is not reserved to launch {} (status={}, reserved_by_launch_id={:?})",
            wallet.id,
            launch_id,
            wallet.status,
            wallet.reserved_by_launch_id
        );
    }
    Ok(())
}

/// Solana's hard per-tx wire cap. Each Jito bundle leg must fit it independently;
/// a leg over the limit fails the whole bundle at submit, so guard locally with an
/// actionable message rather than shipping it and getting an opaque rejection.
const MAX_TX_WIRE_BYTES: usize = 1232;

async fn submit_jito_bundle(url: &str, txs: &[VersionedTransaction]) -> Result<String> {
    let mut encoded = Vec::with_capacity(txs.len());
    for (i, tx) in txs.iter().enumerate() {
        let wire = bincode::serialize(tx)?;
        if wire.len() > MAX_TX_WIRE_BYTES {
            bail!(
                "bundle leg {i} is {} B, over the {MAX_TX_WIRE_BYTES} B tx limit by {} B — \
                 provision/point PUMP_LAUNCH_ALT at a launch ALT so v2 legs compress",
                wire.len(),
                wire.len() - MAX_TX_WIRE_BYTES,
            );
        }
        encoded.push(STANDARD.encode(wire));
    }

    // Jito's `sendBundle` defaults to base58-decoding each tx; we send base64
    // (v0 ALT legs are binary-dense), so the encoding option is mandatory — without
    // it Jito rejects with "transaction #0 could not be decoded".
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [encoded, { "encoding": "base64" }]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .context("Jito sendBundle HTTP")?;
    let status = resp.status();
    let text = resp.text().await.context("Jito sendBundle body")?;
    if !status.is_success() {
        bail!("Jito sendBundle HTTP {status}: {text}");
    }
    let v: serde_json::Value = serde_json::from_str(&text).context("parse Jito response")?;
    if let Some(err) = v.get("error") {
        bail!("Jito sendBundle error: {err}");
    }
    v.get("result")
        .context("Jito response missing result")?
        .as_str()
        .context("Jito bundle id not a string")
        .map(str::to_string)
}
