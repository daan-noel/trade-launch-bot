//! `launch-sim-matrix` — Jito-`simulateBundle` EVERY create × cashback ×
//! buy-variant combination with ZERO real SOL, to establish the actual on-chain
//! working-pairs for BOTH the fused dev-buy and a standalone co-buy leg.
//!
//! For each combo it builds the REAL create(+dev-buy) `tx0` the launch path would
//! submit (via the same `build_create{,_v2}_leg_tx` builders) against a fresh
//! ephemeral mint, PLUS a real co-buy-leg `tx1` of the same variant (via
//! `build_bundle_leg_tx`, signed by a second wallet) against the SAME curve — the
//! exact two builders an atomic launch bundle draws from
//! (`build_atomic_bundle_txs`). Both run through ONE `simulateBundle` call so `tx1`
//! executes against the simulation bank `tx0` just produced, exactly like a real
//! bundle. This covers both the shared `build_curve_buy_core` SSOT AND each
//! builder's own wrapping (idempotent ATA creation, `assemble`'s CU/tip/ATA
//! layout, launch-ALT compilation) — a leg-specific bug (e.g. in `assemble` or the
//! leg's ALT compile) would NOT be caught by the dev-buy alone.
//!
//! Nothing is submitted; the two payers only need a real on-chain balance so a
//! valid pair simulates as SUCCESS (not insufficient-funds). Read-only diagnostic.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use executor_core::IxLayout;
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::ManagedWalletRepo;
use pump_trader::types::{CreateTokenArgs, CreateTokenV2Args, TokenProgram};
use pump_trader::{BundleBuyVariant, BundleLegParams, DevBuy, PumpFunTrader};
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::VersionedTransaction;
use std::str::FromStr;

use crate::bundle_simulate::simulate_jito_bundle;
use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

/// Minimum payer balance to simulate a small dev-buy/co-buy without a false
/// insufficient-funds revert (create rent + 0.01 buy + fee/tip headroom).
const MIN_PAYER_LAMPORTS: i64 = 30_000_000; // ~0.03 SOL
/// Small dev-buy so any modestly-funded managed wallet can be the sim payer.
const DEV_BUY_LAMPORTS: u64 = 10_000_000; // 0.01 SOL
/// Small co-buy leg, same size as the dev-buy for a like-for-like comparison.
const COBUY_LAMPORTS: u64 = 10_000_000; // 0.01 SOL
/// Static leg budget for the sim — real launches jitter these per persona, but a
/// fixed mid-range value is enough to prove the account layout + ix assembly work.
fn sim_leg_params() -> BundleLegParams {
    BundleLegParams {
        slippage_bps: 500,
        cu_limit: 250_000,
        cu_price: 750_000,
        tip_lamports: 200_000,
        layout: IxLayout::canonical_buy(),
    }
}

pub async fn run_launch_sim_matrix(settings: &Settings, _args: &[String]) -> Result<()> {
    let launcher = LauncherSettings::from_env()?;
    let pools = connect(settings).await?;
    let kek = EnvKek::from_passphrase(&launcher.kek_passphrase);

    // Pick the two highest-balance managed wallets we hold keys for: one is the sim
    // creator/dev payer (tx0), the other is the sim bundler payer for the co-buy
    // leg (tx1) — mirroring a real atomic bundle, where the dev wallet and bundler
    // wallets are always distinct. Simulation spends nothing, but both payers must
    // hold real SOL so a VALID pair simulates as success rather than an
    // insufficient-funds revert that would mask the account-layout result we're
    // actually testing.
    let mut wallets = ManagedWalletRepo::list_all(&pools.hot, None).await?;
    wallets.sort_by_key(|w| std::cmp::Reverse(w.balance_lamports.unwrap_or(0)));
    let mut funded = wallets
        .into_iter()
        .filter(|w| w.balance_lamports.unwrap_or(0) > MIN_PAYER_LAMPORTS);
    let payer = funded
        .next()
        .context("no managed wallet with > ~0.03 SOL to act as the sim dev/creator payer")?;
    let bundler_payer = funded.next().context(
        "need a SECOND managed wallet with > ~0.03 SOL to act as the sim co-buy-leg payer \
         (the dev wallet and a bundler wallet are always distinct in a real launch)",
    )?;

    let dev_signer = keystore::resolve_signer(&launcher.keystore_dir, &payer.key_ref, &kek)?;
    let creator = dev_signer.pubkey();
    let bundler_signer = keystore::resolve_signer(&launcher.keystore_dir, &bundler_payer.key_ref, &kek)?;
    let bundler_pubkey = bundler_signer.pubkey();

    let nonce_accounts: Vec<Pubkey> = launcher
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;
    let trader_config = build_launch_trader_config(&launcher, dev_signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    println!("── launch-sim-matrix (Jito simulateBundle, zero SOL) ──────────────────");
    println!(
        "dev payer     : {creator}  ({:.4} SOL)",
        payer.balance_lamports.unwrap_or(0) as f64 / 1e9
    );
    println!(
        "bundler payer : {bundler_pubkey}  ({:.4} SOL)",
        bundler_payer.balance_lamports.unwrap_or(0) as f64 / 1e9
    );
    println!("dev-buy       : {:.4} SOL per combo (slippage 500bps, fused into tx0)", DEV_BUY_LAMPORTS as f64 / 1e9);
    println!("co-buy leg    : {:.4} SOL per combo (slippage 500bps, standalone tx1)", COBUY_LAMPORTS as f64 / 1e9);
    println!("launch ALT    : {:?}", launcher.launch_alt);
    println!("note          : tx0 (create+dev-buy) and tx1 (co-buy leg) run through ONE");
    println!("                simulateBundle call, so tx1 executes against the curve tx0");
    println!("                just made — exactly like a real atomic launch bundle.");
    println!("─────────────────────────────────────────────────────────────────────");
    println!(
        "{:<20} {:<9} {:<24} {:<12} {:<12} detail",
        "create", "cashback", "buy_variant", "dev_buy", "leg_buy"
    );
    println!("{}", "-".repeat(120));

    // create variant, cashback flag (create_v1 ignores it)
    let creates: [(&str, bool); 3] = [
        ("pumpfun.create_v1", false),
        ("pumpfun.create_v2", false),
        ("pumpfun.create_v2", true),
    ];
    let buy_variants: [(&str, BundleBuyVariant); 4] = [
        ("buy", BundleBuyVariant::Buy),
        ("buy_exact_sol_in", BundleBuyVariant::BuyExactSolIn),
        ("buy_v2", BundleBuyVariant::BuyV2),
        ("buy_exact_quote_in_v2", BundleBuyVariant::BuyExactQuoteIn),
    ];

    for (create_variant, cashback) in creates {
        for (bv_name, bv) in buy_variants {
            let mint = Keypair::new();
            let blockhash = trader.fresh_blockhash().await?;
            let dev_buy = Some(DevBuy {
                sol: DEV_BUY_LAMPORTS as f64 / 1e9,
                lamports: DEV_BUY_LAMPORTS,
                slippage_bps: Some(500),
                variant: bv,
            });

            let create_built = match create_variant {
                "pumpfun.create_v2" => {
                    let args = CreateTokenV2Args {
                        name: "SimMatrix".into(),
                        symbol: "SIM".into(),
                        uri: "ipfs://QmSimMatrixVerificationPlaceholder000000000000000000".into(),
                        creator,
                        is_mayhem_mode: false,
                        cashback_enabled: cashback,
                    };
                    trader
                        .build_create_v2_leg_tx(&mint, &args, dev_buy, blockhash, 0)
                        .await
                }
                _ => {
                    let args = CreateTokenArgs {
                        name: "SimMatrix".into(),
                        symbol: "SIM".into(),
                        uri: "ipfs://QmSimMatrixVerificationPlaceholder000000000000000000".into(),
                        creator,
                    };
                    trader
                        .build_create_leg_tx(&mint, &args, dev_buy, blockhash, 0)
                        .await
                }
            };

            // Each buyer's base token ATA — tracked so a "success" is proven by real
            // tokens landing in it (not a silent no-op). Token program follows the
            // create variant (Legacy SPL for v1, Token-2022 for v2). ATA =
            // PDA([owner, token_program, mint], ATA_PROGRAM).
            let token_program_pk = if create_variant.ends_with("v1") {
                pump_trader::protocol::TOKEN
            } else {
                pump_trader::protocol::TOKEN_2022
            };
            let base_ata_for = |owner: &Pubkey| {
                Pubkey::find_program_address(
                    &[owner.as_ref(), token_program_pk.as_ref(), mint.pubkey().as_ref()],
                    &pump_trader::protocol::ASSOCIATED_TOKEN_PROGRAM,
                )
                .0
            };
            let dev_base_ata = base_ata_for(&creator);
            let leg_base_ata = base_ata_for(&bundler_pubkey);

            // tx1 — the standalone co-buy leg (`build_bundle_leg_tx`, the SAME
            // builder an atomic launch bundle draws from), same variant as the
            // dev-buy above. Only attempted when tx0 built — it has no curve to buy
            // against otherwise. Pre-buy reserves are the SIMULATED post-dev-buy
            // curve (no on-chain state exists yet for this ephemeral mint), the same
            // `simulate_launch_leg_reserves` SSOT `build_atomic_bundle_txs` uses.
            let mut leg_build_err: Option<String> = None;
            let leg_built = if create_built.is_ok() {
                let token_program = if create_variant.ends_with("v1") {
                    TokenProgram::Legacy
                } else {
                    TokenProgram::Token2022
                };
                let leg_reserves =
                    trader.simulate_launch_leg_reserves(DEV_BUY_LAMPORTS, &[COBUY_LAMPORTS])[0];
                match trader
                    .build_bundle_leg_tx(
                        bundler_signer.as_ref(),
                        blockhash,
                        &mint.pubkey(),
                        &creator,
                        token_program,
                        COBUY_LAMPORTS,
                        cashback,
                        bv,
                        &sim_leg_params(),
                        Some(leg_reserves),
                    )
                    .await
                {
                    Ok(tx) => Some(tx),
                    Err(e) => {
                        leg_build_err = Some(e.to_string());
                        None
                    }
                }
            } else {
                leg_build_err = Some("skipped (tx0 create leg failed to build)".to_string());
                None
            };

            // Run whatever built through ONE simulateBundle call so the co-buy leg
            // (if present) executes against tx0's simulation bank.
            let mut txs: Vec<VersionedTransaction> = Vec::with_capacity(2);
            let mut track: Vec<Option<Pubkey>> = Vec::with_capacity(2);
            let dev_build_err = match create_built {
                Ok(tx) => {
                    txs.push(tx);
                    track.push(Some(dev_base_ata));
                    None
                }
                Err(e) => Some(e.to_string()),
            };
            if let Some(tx) = leg_built {
                txs.push(tx);
                track.push(Some(leg_base_ata));
            }

            let leg_results: Vec<(String, String)> = if txs.is_empty() {
                Vec::new()
            } else {
                match combo_leg_results(&launcher.rpc_url, &txs, &track).await {
                    Ok(r) => r,
                    Err(e) => vec![("SIM_ERR".to_string(), e.to_string()); txs.len()],
                }
            };
            let mut leg_results = leg_results.into_iter();

            let (dev_result, dev_detail) = match dev_build_err {
                Some(e) => ("BUILD_ERR".to_string(), e),
                None => leg_results
                    .next()
                    .unwrap_or_else(|| ("SIM_ERR".to_string(), "no result".to_string())),
            };
            let (leg_result, leg_detail) = match leg_build_err {
                Some(e) => ("BUILD_ERR".to_string(), e),
                None => leg_results
                    .next()
                    .unwrap_or_else(|| ("SIM_ERR".to_string(), "no result".to_string())),
            };

            let cb = if create_variant.ends_with("v1") {
                "n/a"
            } else if cashback {
                "on"
            } else {
                "off"
            };
            println!(
                "{:<20} {:<9} {:<24} {:<12} {:<12} dev[{dev_detail}] leg[{leg_detail}]",
                create_variant, cb, bv_name, dev_result, leg_result
            );
        }
        println!();
    }

    println!("─────────────────────────────────────────────────────────────────────");
    println!("OK = simulates cleanly (valid working-pair). REVERT (N) = Anchor custom N");
    println!("(2006 = ConstraintSeeds; a v2 buy against a curve with no sharing_config).");
    Ok(())
}

/// Parse a `simulateBundle` response into one `(result, detail)` pair per tx, in
/// request order — `txs[0]` is always tx0 (create+dev-buy); `txs[1]`, if present,
/// is the co-buy leg. Mirrors the single-tx `simulateTransaction` parse this
/// replaced (same OK/REVERT/custom-code/log-hint shape), plus a best-effort
/// post-execution token balance per leg from `postExecutionAccountsConfigs`.
async fn combo_leg_results(
    rpc_url: &str,
    txs: &[VersionedTransaction],
    track: &[Option<Pubkey>],
) -> Result<Vec<(String, String)>> {
    let resp = simulate_jito_bundle(rpc_url, txs, track).await?;
    if let Some(err) = resp.get("error") {
        bail!("simulateBundle RPC error: {err}");
    }
    let value = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .context("simulateBundle: no result.value")?;
    let results = value
        .get("transactionResults")
        .and_then(Value::as_array)
        .context("simulateBundle: no transactionResults")?;

    Ok(results
        .iter()
        .map(|tx| {
            let err = tx.get("err");
            let ok = matches!(err, None | Some(Value::Null));
            let units = tx.get("unitsConsumed").and_then(Value::as_u64);
            if ok {
                let token_after = extract_token_after(tx);
                (
                    "OK".to_string(),
                    format!(
                        "cu={} tokens_recv={}",
                        units.map(|u| u.to_string()).unwrap_or_else(|| "?".into()),
                        token_after.map(|t| t.to_string()).unwrap_or_else(|| "0".into()),
                    ),
                )
            } else {
                let err_str = err.map(|e| e.to_string()).unwrap_or_default();
                let code = extract_custom(&err_str);
                let hint = tx
                    .get("logs")
                    .and_then(Value::as_array)
                    .and_then(|logs| {
                        logs.iter().rev().find_map(|l| {
                            l.as_str().filter(|l| {
                                l.contains("failed") || l.contains("Error") || l.contains("Constraint")
                            })
                        })
                    })
                    .unwrap_or_default();
                (
                    format!("REVERT{}", code.map(|c| format!(" ({c})")).unwrap_or_default()),
                    format!("{err_str} | {hint}"),
                )
            }
        })
        .collect())
}

/// Best-effort post-execution raw token balance of a `simulateBundle` tx result's
/// FIRST tracked account (`postExecutionAccountsConfigs`) — offset 64 (u64 LE) is
/// the SPL/Token-2022 amount field, present in both layouts. `None` if the field is
/// absent (RPC doesn't populate it) or the ATA wasn't created.
fn extract_token_after(tx_result: &Value) -> Option<u64> {
    tx_result
        .get("postExecutionAccounts")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|acct| acct.as_object())
        .and_then(|acct| acct.get("data"))
        .and_then(|d| d.as_array())
        .and_then(|d| d.first())
        .and_then(Value::as_str)
        .and_then(|b64| STANDARD.decode(b64).ok())
        .filter(|bytes| bytes.len() >= 72)
        .map(|bytes| u64::from_le_bytes(bytes[64..72].try_into().unwrap()))
}

/// Pull the Anchor custom error code out of a `simulateTransaction`/`simulateBundle`
/// `err` JSON,
/// e.g. `{"InstructionError":[2,{"Custom":2006}]}` → `Some(2006)`.
fn extract_custom(err_json: &str) -> Option<u32> {
    let marker = "\"Custom\":";
    let start = err_json.find(marker)? + marker.len();
    let rest = &err_json[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    /// Live Helius simulation of the full create × cashback × buy-variant matrix.
    /// `#[ignore]` by default — it hits mainnet RPC, the DB, and the keystore. Run
    /// it (with the launcher env loaded from `forge/.env`) via:
    ///
    /// ```text
    /// cargo test -p forge-launcher launch_sim_matrix_live -- --ignored --nocapture
    /// ```
    ///
    /// Runs as a launcher test harness (a separate binary), so it does NOT fight the
    /// running `forge-live.exe` for the on-disk executable the CLI subcommand would.
    #[tokio::test]
    #[ignore]
    async fn launch_sim_matrix_live() {
        let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.env");
        load_dotenv(&env_path);
        // The real server runs from `forge/`, so `WALLET_KEYSTORE=./keystore` is
        // relative to it. Under a test harness the CWD differs, so absolutize any
        // relative keystore path against `forge/` (the .env's directory).
        let forge_dir = env_path.parent().expect("env parent dir");
        if let Ok(ks) = std::env::var("WALLET_KEYSTORE") {
            let ks_path = std::path::Path::new(&ks);
            if ks_path.is_relative() {
                let abs = forge_dir.join(ks.trim_start_matches("./").trim_start_matches(".\\"));
                std::env::set_var("WALLET_KEYSTORE", abs);
            }
        }
        let settings =
            platform_core::config::Settings::from_env().expect("Settings::from_env (forge/.env)");
        super::run_launch_sim_matrix(&settings, &[])
            .await
            .expect("run_launch_sim_matrix");
    }

    /// Minimal `.env` loader (no dotenvy dep): `KEY=VALUE` per line, `#` comments,
    /// optional surrounding quotes stripped. Existing process env wins (never
    /// clobbers a var already set), matching dotenvy's non-override default.
    #[cfg(test)]
    fn load_dotenv(path: &std::path::Path) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            let mut v = v.trim();
            if v.len() >= 2
                && ((v.starts_with('"') && v.ends_with('"'))
                    || (v.starts_with('\'') && v.ends_with('\'')))
            {
                v = &v[1..v.len() - 1];
            }
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v);
            }
        }
    }
}
