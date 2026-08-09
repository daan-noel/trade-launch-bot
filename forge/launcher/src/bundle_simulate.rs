//! `bundle-simulate <bundle_id>` — read-only pre-flight for an atomic launch
//! bundle. Rebuilds the EXACT transactions `execute_bundle` would submit (via the
//! shared [`crate::bundle_execute::build_atomic_bundle_txs`] SSOT) and runs Jito's
//! `simulateBundle` RPC against them — executing `tx0` (create + dev-buy) then every
//! co-buy leg in sequence against one simulation bank, so the co-buys see the curve
//! `tx0` creates. Nothing is submitted; NO real SOL is spent.
//!
//! Purpose: a Jito bundle that is accepted into the queue but never lands gives no
//! reason back. `sendBundle` returns a bundle id even for a bundle whose txs will
//! revert (Jito only lands a bundle if EVERY tx succeeds). This surfaces the actual
//! on-chain error + program logs so a launch that "creates-then-drops" (really:
//! never lands because a leg reverts) can be diagnosed without burning a launch.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::{BundleRepo, LaunchRepo, ManagedWalletRepo};
use pump_trader::types::TokenProgram;
use pump_trader::PumpFunTrader;
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use std::str::FromStr;
use tracing::info;

use crate::bundle_execute::{build_atomic_bundle_txs, CreateLegArgs};
use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::plan_pipeline::{self, is_bundler_leg};
use crate::trader_config::build_launch_trader_config;

/// `bundle-simulate <bundle_id>` — rebuild + simulate an atomic launch bundle.
pub async fn run_bundle_simulate(settings: &Settings, args: &[String]) -> Result<()> {
    let bundle_id = uuid::Uuid::parse_str(
        args.first()
            .context("usage: forge-live bundle-simulate <bundle_id>")?,
    )
    .context("parse bundle_id")?;

    let launcher = LauncherSettings::from_env()?;
    let pools = connect(settings).await?;

    // --- Load the persisted bundle exactly as the submit path would ------------
    let bundle = BundleRepo::get(&pools.hot, bundle_id)
        .await?
        .context("bundle not found")?;
    let launch = LaunchRepo::get(&pools.hot, bundle.launch_id)
        .await?
        .context("launch not found for bundle")?;

    let create_args: CreateLegArgs = serde_json::from_value(
        bundle
            .create_args
            .clone()
            .context("bundle has no create_args (predates the atomic-launch cutover)")?,
    )
    .context("deserialize persisted create_args")?;

    let plan: orchestrator::Plan = serde_json::from_value(
        bundle
            .plan
            .clone()
            .context("bundle has no persisted plan")?,
    )
    .context("deserialize persisted bundle plan")?;
    let gated = plan_pipeline::gate(plan, launcher.allow_fingerprint)
        .context("persisted bundle plan failed the mandatory plan/audit gate")?;
    let bundler_ops: Vec<&orchestrator::Operation> =
        gated.plan.ops.iter().filter(|op| is_bundler_leg(op)).collect();
    if bundler_ops.is_empty() {
        bail!("bundle plan has no bundler legs");
    }

    // --- Stand up the dev trader (same as execute_bundle's None-trader branch) --
    let kek = EnvKek::from_passphrase(&launcher.kek_passphrase);
    let dev_wallet_id = launch
        .dev_wallet_id
        .context("launch has no dev wallet id (cannot rebuild create leg)")?;
    let dev_wallet = ManagedWalletRepo::get(&pools.hot, dev_wallet_id)
        .await?
        .context("dev wallet not found")?;
    let dev_signer = keystore::resolve_signer(&launcher.keystore_dir, &dev_wallet.key_ref, &kek)?;
    let nonce_accounts: Vec<Pubkey> = launcher
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;
    let trader_config = build_launch_trader_config(&launcher, dev_signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    // Simulate against a FRESH ephemeral mint rather than the launch's persisted one:
    // (a) a terminal (dropped/failed) bundle's mint key is deleted on its outcome,
    // and (b) a brand-new mint guarantees the create leg simulates against a clean
    // curve, exactly like a real first launch. Mint identity is irrelevant to whether
    // any leg reverts — this tests the create+co-buy SEQUENCE, not the exact address.
    let sim_mint = Keypair::new();
    let mint = sim_mint.pubkey();
    let creator = Pubkey::from_str(&create_args.creator).context("parse creator")?;
    let token_program_id = keystore::token_program_for_variant(&create_args.variant);
    let token_program = TokenProgram::from_id(token_program_id);
    let cashback_enabled = create_args.cashback_enabled;
    let blockhash = trader.fresh_blockhash().await?;

    // Tip level 0: the tip is just a transfer and never affects whether a leg
    // reverts, so simulate a clean first attempt regardless of `submit_attempts`.
    let built = build_atomic_bundle_txs(
        &pools.hot,
        &launcher,
        &trader,
        &kek,
        &gated,
        &bundler_ops,
        &create_args,
        &sim_mint,
        &mint,
        &creator,
        token_program,
        cashback_enabled,
        blockhash,
        0,
        bundle.launch_id,
        false, // don't require an active reservation — a terminal bundle's wallets are released
    )
    .await?;

    println!("── bundle-simulate {bundle_id} ─────────────────────────────");
    println!("launch   : {}", bundle.launch_id);
    println!("orig mint: {}", launch.mint_address);
    println!("sim mint : {mint} (ephemeral — identity is irrelevant to reverts)");
    println!("variant  : {}", create_args.variant);
    println!("legs     : tx0 create(+dev-buy) + {} co-buy(s)", built.leg_signatures.len());
    println!("create   : {}", built.create_signature);
    for (i, sig) in built.leg_signatures.iter().enumerate() {
        println!("co-buy {i} : {sig}");
    }
    println!("──────────────────────────────────────────────────────────");

    let result = simulate_jito_bundle(&launcher.rpc_url, &built.txs, &vec![None; built.txs.len()]).await?;
    report_simulation(&result);
    Ok(())
}

/// POST Jito `simulateBundle` to the configured RPC. Executes the txs in order
/// against one simulation bank (so co-buys see the curve `tx0` creates). Signatures
/// are real (we sign with the keystore keys), but `skipSigVerify` is set so a
/// slightly-stale blockhash can't mask the actual execution error. `track` is a
/// parallel slice — `Some(pubkey)` at index `i` requests post-execution account data
/// for `txs[i]` (e.g. to prove tokens actually landed in a buyer's ATA); `None`
/// skips tracking for that leg. SSOT for the `simulateBundle` call — used by both
/// this bundle's real-persisted-bundle pre-flight and `launch_sim_matrix`'s
/// create+dev-buy+co-buy-leg matrix.
pub(crate) async fn simulate_jito_bundle(
    rpc_url: &str,
    txs: &[solana_sdk::transaction::VersionedTransaction],
    track: &[Option<Pubkey>],
) -> Result<Value> {
    let encoded: Vec<String> = txs
        .iter()
        .map(|tx| bincode::serialize(tx).map(|b| STANDARD.encode(b)))
        .collect::<std::result::Result<_, _>>()
        .context("serialize bundle tx")?;

    let pre_null: Vec<Value> = vec![Value::Null; encoded.len()];
    let post_configs: Vec<Value> = track
        .iter()
        .map(|t| match t {
            Some(pk) => serde_json::json!({ "encoding": "base64", "addresses": [pk.to_string()] }),
            None => Value::Null,
        })
        .collect();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateBundle",
        "params": [
            { "encodedTransactions": encoded },
            {
                "skipSigVerify": true,
                "transactionEncoding": "base64",
                "preExecutionAccountsConfigs": pre_null,
                "postExecutionAccountsConfigs": post_configs,
            }
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("simulateBundle HTTP")?;
    let status = resp.status();
    let text = resp.text().await.context("simulateBundle body")?;
    if !status.is_success() {
        bail!("simulateBundle HTTP {status}: {text}");
    }
    serde_json::from_str(&text).context("parse simulateBundle response")
}

/// Print a human-readable verdict from the `simulateBundle` response: the summary
/// (succeeded / which tx failed + why) and every tx's error + program logs.
fn report_simulation(resp: &Value) {
    if let Some(err) = resp.get("error") {
        println!("❌ RPC ERROR (simulateBundle rejected the request, not the bundle):");
        println!("{}", serde_json::to_string_pretty(err).unwrap_or_default());
        println!(
            "\nNote: if this says the method is unknown, point HELIUS_RPC_URL at an RPC \
             that exposes Jito's simulateBundle."
        );
        return;
    }

    let value = resp.get("result").and_then(|r| r.get("value"));
    let Some(value) = value else {
        println!("⚠️  Unexpected response shape — raw:");
        println!("{}", serde_json::to_string_pretty(resp).unwrap_or_default());
        return;
    };

    let summary = value.get("summary");
    let succeeded = summary.and_then(Value::as_str) == Some("succeeded");

    if succeeded {
        println!("✅ BUNDLE SIMULATION SUCCEEDED — every tx executes cleanly.");
        println!("   ⇒ the bundle is VALID; non-landing is an AUCTION/TIP problem, not a revert.");
    } else {
        println!("❌ BUNDLE SIMULATION FAILED — a transaction reverts, so Jito can never land it");
        println!("   (no tip will help). Failure summary:");
        if let Some(summary) = summary {
            println!("{}", serde_json::to_string_pretty(summary).unwrap_or_default());
        }
    }

    // Per-tx detail: err + program logs (the create leg is index 0).
    if let Some(results) = value.get("transactionResults").and_then(Value::as_array) {
        for (i, tx) in results.iter().enumerate() {
            let label = if i == 0 { "tx0 create(+dev-buy)".to_string() } else { format!("co-buy leg {}", i - 1) };
            let err = tx.get("err");
            let ok = matches!(err, None | Some(Value::Null));
            println!("\n── {label} — {} ──", if ok { "ok" } else { "REVERTED" });
            if let Some(units) = tx.get("unitsConsumed") {
                println!("   unitsConsumed: {units}");
            }
            if !ok {
                println!("   err: {}", err.map(|e| e.to_string()).unwrap_or_default());
            }
            if let Some(logs) = tx.get("logs").and_then(Value::as_array) {
                println!("   logs:");
                for line in logs {
                    if let Some(s) = line.as_str() {
                        println!("     {s}");
                    }
                }
            }
        }
    } else {
        println!("\n(no transactionResults in response — raw value:)");
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    }

    info!(succeeded, "bundle-simulate complete");
}
