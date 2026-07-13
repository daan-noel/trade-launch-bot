//! Execute a planned bundle: build signed buy txs → Jito `sendBundle`.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::models::{BundleStatus, ManagedWallet, WalletRole, WalletStatus};
use platform_core::storage::repositories::{BundleRepo, LaunchRepo, ManagedWalletRepo};
use pump_trader::types::{CreateTokenArgs, CreateTokenV2Args, TokenProgram};
use pump_trader::PumpFunTrader;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
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

/// The create (+ dev-buy) leg's build inputs, persisted on the bundle (JSONB
/// `bundles.create_args`) so the confirm watcher's background re-bid can rebuild
/// the atomic bundle's `tx0` create leg without the original launch request. The
/// SSOT for the create leg — service.rs writes it at plan time; [`build_create_leg`]
/// reads it on every submit + re-bid so first-attempt and re-bid create legs are
/// byte-identical bar the tip level + blockhash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLegArgs {
    /// Template variant: `"pumpfun.create_v2"` | `"pumpfun.create_v1"`.
    pub variant: String,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    #[serde(default)]
    pub is_mayhem_mode: bool,
    #[serde(default)]
    pub cashback_enabled: bool,
    /// Dev-buy lamports fused into the create leg; `0` = create-only.
    #[serde(default)]
    pub dev_buy_quote: u64,
    /// Dev-buy curve encoding (catalog name: `buy` / `buy_exact_sol_in` / `buy_v2` /
    /// `buy_exact_quote_in_v2`). Defaults to `buy_exact_sol_in` for bundles persisted
    /// before dev-buy variants existed.
    #[serde(default = "default_dev_buy_variant")]
    pub dev_buy_variant: String,
    /// Dev-buy slippage tolerance (bps); `None` = unprotected (min_out 1). Required
    /// (launcher-validated) when `dev_buy_variant` is a tokens-out encoding.
    #[serde(default)]
    pub slippage_bps: Option<u64>,
    /// The dev/creator wallet pubkey (base58) — equals the create ix `creator`.
    pub creator: String,
}

/// The dev-buy encoding for `CreateLegArgs` persisted before dev-buy variants
/// existed (they were always `buy_exact_sol_in`).
fn default_dev_buy_variant() -> String {
    crate::plan_pipeline::LAUNCH_BUY_VARIANT.to_string()
}

/// Build + submit a `planned` atomic launch bundle to Jito — the create (+ dev-buy)
/// leg (`tx0`) plus every co-buy leg in ONE `sendBundle`, standing up a fresh trader
/// from the launch's DEV wallet. Standalone entrypoint (`POST /bundles/:id/execute`
/// and the confirm watcher's re-bid).
pub async fn execute_bundle(
    pool: &PgPool,
    settings: &LauncherSettings,
    bundle_id: Uuid,
) -> Result<BundleExecuteResult> {
    execute_bundle_inner(pool, settings, bundle_id, None).await
}

/// Same as [`execute_bundle`], but reuse the DEV trader the caller already built +
/// initialized (service.rs's launch trader, whose `config.signer` is the dev
/// wallet). The create leg signs + pays as that dev wallet; each co-buy leg signs
/// with its own bundler wallet (the cached global account is wallet-independent for
/// the fields the legs read — `build_bundle_leg_tx` re-derives each leg's
/// `user_volume_accumulator` from that leg's signer). Avoids a second full
/// `initialize()` for the auto-submit that follows a plan in the same request.
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

    // The create (+ dev-buy) leg is `tx0` of the atomic bundle now, NOT a
    // separately-landed tx — so a bundle without `create_args` predates this
    // cutover and can't be built on this path.
    let create_args: CreateLegArgs = serde_json::from_value(
        bundle
            .create_args
            .clone()
            .context("bundle predates the atomic-launch cutover (no create_args) — cannot execute")?,
    )
    .context("deserialize persisted create_args")?;

    // Phase 2.F: the bundle's executable SSOT is the persisted orchestrator `Plan`.
    // Deserialize it and re-run the mandatory gate (which recomputes the identical
    // deterministic disguises and re-audits).
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

    // Reuse the caller's already-initialized trader when given one (the launch's
    // DEV trader from service.rs); otherwise stand up a fresh one from the DEV
    // wallet — the create leg signs + pays as the dev/creator, so `config.signer`
    // MUST be the dev wallet (each co-buy leg still signs with its OWN bundler
    // wallet, independent of `config.signer`).
    let built_trader;
    let trader: &PumpFunTrader = match existing_trader {
        Some(t) => t,
        None => {
            let dev_wallet_id = launch
                .dev_wallet_id
                .context("launch has no dev wallet id (cannot rebuild create leg)")?;
            let dev_wallet = ManagedWalletRepo::get(pool, dev_wallet_id)
                .await?
                .context("dev wallet not found")?;
            let dev_signer =
                keystore::resolve_signer(&settings.keystore_dir, &dev_wallet.key_ref, &kek)?;
            let nonce_accounts: Vec<Pubkey> = settings
                .nonce_accounts
                .iter()
                .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
                .collect::<Result<_>>()?;
            let trader_config = build_launch_trader_config(settings, dev_signer, nonce_accounts);
            let mut t = PumpFunTrader::new(trader_config);
            t.initialize().await.context("initialize pump-trader")?;
            built_trader = t;
            &built_trader
        }
    };

    // The persisted mint keypair (envelope-encrypted at plan time). REQUIRED — the
    // create leg is inside the bundle, so a re-bid must re-sign create with the SAME
    // mint. Deterministic key_ref from the launch id (no DB column).
    let mint_ref = keystore::mint_key_ref(bundle.launch_id);
    let mint_signer = keystore::resolve_signer(&settings.keystore_dir, &mint_ref, &kek)
        .context("load persisted launch mint key (atomic bundle create leg needs it)")?;
    if mint_signer.pubkey().to_string() != launch.mint_address {
        bail!(
            "persisted mint key {} does not match launch mint {}",
            mint_signer.pubkey(),
            launch.mint_address
        );
    }

    let mint = Pubkey::from_str(&launch.mint_address).context("parse mint")?;
    let creator = Pubkey::from_str(&create_args.creator).context("parse creator")?;
    let token_program_id = keystore::token_program_for_variant(&create_args.variant);
    let token_program = TokenProgram::from_id(token_program_id);
    let cashback_enabled = create_args.cashback_enabled;

    let blockhash = trader.fresh_blockhash().await?;

    // Escalation level = `submit_attempts` (0 first attempt; a re-bid after a
    // `dropped` verdict re-enters with it incremented → bids above the tip that just
    // lost). The WHOLE-bundle Jito tip rides the create leg (`tx0`) at this level
    // (sized to the live floor, clamped to the cost guardrail); each co-buy leg
    // carries only its small persona tip jitter (`min_tip = 0`) so the total sits at
    // ~floor without double-counting a per-leg split.
    let level = bundle.submit_attempts.max(0) as u8;
    let total_tip_floor = trader.jito_tip_floor_lamports(level);

    // Atomic compare-and-swap `planned → submitting`. The read-check above is a
    // fast-fail for an obviously-wrong status, but ~8-12 RPC round trips (gate,
    // trader init, blockhash, tip floor) run between it and here — two concurrent
    // executes can both pass that check. This conditional UPDATE is the real gate:
    // exactly one caller flips the row, and the loser aborts BEFORE signing or
    // submitting, so the same wallets can't double-submit real-SOL buys.
    if !BundleRepo::claim_for_submitting(pool, bundle_id).await? {
        bail!(
            "bundle {bundle_id} was concurrently claimed for submission \
             (status is no longer planned) — aborting to avoid a double submit"
        );
    }

    let finish = async {
        // Build tx0 (create + fused dev-buy) + every co-buy leg via the shared SSOT
        // builder — byte-identical to what `bundle-simulate` pre-checks. `true` =
        // enforce that each leg wallet is still `reserved` to this launch (the real
        // spend path); the simulate path passes `false`.
        let built = build_atomic_bundle_txs(
            pool,
            settings,
            trader,
            &kek,
            &gated,
            &bundler_ops,
            &create_args,
            mint_signer.as_ref(),
            &mint,
            &creator,
            token_program,
            cashback_enabled,
            blockhash,
            level,
            bundle.launch_id,
            true,
        )
        .await?;

        let jito_bundle_id =
            submit_jito_bundle(&settings.jito_block_engine_url, &built.txs).await?;
        // `leg_signatures` is the CO-BUY set the confirm watcher checks against the
        // `trades` feed; the bundle is atomic, so all co-buys present ⟹ create landed.
        // Stamp the create leg's signature on the launch (status stays `pending`
        // until the watcher confirms landing); a re-bid overwrites it.
        BundleRepo::set_submitted(pool, bundle_id, &jito_bundle_id, &built.leg_signatures)
            .await?;
        BundleRepo::set_tip_quote(pool, bundle_id, total_tip_floor as i64).await?;
        LaunchRepo::set_create_signature(pool, bundle.launch_id, &built.create_signature)
            .await?;

        // Wallet-pool lifecycle: dev + bundler wallets stay `reserved` here — submit
        // isn't completion. `launcher::confirm` transitions them to `used` on a
        // terminal outcome.

        info!(
            bundle_id = %bundle_id,
            launch_id = %bundle.launch_id,
            legs = bundler_ops.len(),
            jito_bundle_id = %jito_bundle_id,
            create_signature = %built.create_signature,
            tip_level = level,
            total_tip_floor_lamports = total_tip_floor,
            "atomic launch bundle submitted to Jito (create + co-buys)"
        );

        Ok(BundleExecuteResult {
            bundle_id,
            jito_bundle_id,
            leg_signatures: built.leg_signatures,
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

/// The signed transactions of an atomic launch bundle — `tx0` (create + fused
/// dev-buy) followed by every co-buy leg in submit order — plus the signatures the
/// caller stamps/tracks. Returned by [`build_atomic_bundle_txs`].
pub(crate) struct BuiltBundleTxs {
    pub txs: Vec<VersionedTransaction>,
    pub create_signature: String,
    pub leg_signatures: Vec<String>,
}

/// Build (but do NOT submit) an atomic launch bundle's transactions: the create
/// (+ fused dev-buy) leg as `tx0`, then each co-buy leg, all on the shared
/// `blockhash`. This is the SSOT for what a launch bundle *is* on the wire —
/// [`execute_bundle_inner`] submits exactly this, and `bundle-simulate` simulates
/// exactly this, so a pre-submit dry-run can never diverge from the real spend.
///
/// `require_reservation` gates the per-leg wallet check: the real spend path passes
/// `true` (a leg wallet must still be `reserved` to this launch); the read-only
/// simulate passes `false` so a terminal bundle (dropped/failed, wallets already
/// released) can still be rebuilt and checked.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_atomic_bundle_txs(
    pool: &PgPool,
    settings: &LauncherSettings,
    trader: &PumpFunTrader,
    kek: &EnvKek,
    gated: &plan_pipeline::GatedPlan,
    bundler_ops: &[&orchestrator::Operation],
    create_args: &CreateLegArgs,
    mint_signer: &(dyn Signer + Send + Sync),
    mint: &Pubkey,
    creator: &Pubkey,
    token_program: TokenProgram,
    cashback_enabled: bool,
    blockhash: solana_sdk::hash::Hash,
    level: u8,
    launch_id: Uuid,
    require_reservation: bool,
) -> Result<BuiltBundleTxs> {
    // The whole-bundle Jito tip rides the create leg (`tx0`); co-buy legs carry
    // only their persona tip jitter, so their tip floor is 0.
    const COBUY_MIN_TIP: u64 = 0;

    // tx0 — the create (+ fused dev-buy) leg, signed by dev + mint on the shared
    // blockhash, carrying the whole-bundle tip at `level`.
    let dev_buy = if create_args.dev_buy_quote > 0 {
        let sol = create_args.dev_buy_quote as f64 / pump_trader::protocol::LAMPORTS_PER_SOL as f64;
        // Same catalog→executor map the co-buy legs use, so the dev buy honors its
        // authored encoding (create leg is byte-identical on every submit + re-bid).
        let variant = plan_pipeline::bundle_buy_variant(&create_args.dev_buy_variant)
            .context("persisted dev_buy_variant is not a valid buy encoding")?;
        Some(pump_trader::DevBuy {
            sol,
            lamports: create_args.dev_buy_quote,
            slippage_bps: create_args.slippage_bps,
            variant,
        })
    } else {
        None
    };
    let create_tx = match create_args.variant.as_str() {
        "pumpfun.create_v2" => {
            let args = CreateTokenV2Args {
                name: create_args.name.clone(),
                symbol: create_args.symbol.clone(),
                uri: create_args.uri.clone(),
                creator: *creator,
                is_mayhem_mode: create_args.is_mayhem_mode,
                cashback_enabled: create_args.cashback_enabled,
            };
            trader
                .build_create_v2_leg_tx(mint_signer, &args, dev_buy, blockhash, level)
                .await?
        }
        "pumpfun.create_v1" => {
            let args = CreateTokenArgs {
                name: create_args.name.clone(),
                symbol: create_args.symbol.clone(),
                uri: create_args.uri.clone(),
                creator: *creator,
            };
            trader
                .build_create_leg_tx(mint_signer, &args, dev_buy, blockhash, level)
                .await?
        }
        other => bail!("unsupported launch variant: {other}"),
    };
    let create_signature = create_tx.signatures[0].to_string();

    // The curve is created in THIS bundle, so co-buy legs can't read live reserves —
    // simulate the curve forward (create → dev-buy → prior co-buys) so each leg's
    // min_out floor is computed against the state it will fill at.
    let cobuy_lamports: Vec<u64> = bundler_ops
        .iter()
        .map(|op| crate::plan_exec::buy_lamports(op))
        .collect::<Result<_>>()?;
    let leg_reserves =
        trader.simulate_launch_leg_reserves(create_args.dev_buy_quote, &cobuy_lamports);

    // tx1..N — the co-buy legs, each signed by its own bundler wallet, all on the
    // same blockhash so the whole bundle lands atomically in one slot.
    let mut txs = Vec::with_capacity(bundler_ops.len() + 1);
    txs.push(create_tx);
    let mut leg_signatures = Vec::with_capacity(bundler_ops.len());
    for (i, op) in bundler_ops.iter().enumerate() {
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
        if require_reservation {
            check_wallet_reserved_to_bundle(&wallet, launch_id)?;
        }
        let signer = keystore::resolve_signer(&settings.keystore_dir, &wallet.key_ref, kek)?;
        let tx = crate::plan_exec::build_leg_tx(
            trader,
            signer.as_ref(),
            blockhash,
            mint,
            creator,
            token_program,
            cashback_enabled,
            op,
            disguise,
            COBUY_MIN_TIP,
            Some(leg_reserves[i]),
        )
        .await?;
        leg_signatures.push(tx.signatures[0].to_string());
        txs.push(tx);
    }

    Ok(BuiltBundleTxs {
        txs,
        create_signature,
        leg_signatures,
    })
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
