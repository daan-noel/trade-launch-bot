//! `launch-sim-matrix` — Helius-simulate EVERY create × cashback × buy-variant
//! combination with ZERO real SOL, to establish the actual on-chain working-pairs.
//!
//! For each combo it builds the REAL create(+dev-buy) `tx0` the launch path would
//! submit (via the same `build_create{,_v2}_leg_tx` builders) against a fresh
//! ephemeral mint, then runs a standard `simulateTransaction` against live chain
//! state. Because create + dev-buy execute in ONE tx, the simulated buy sees the
//! curve the create just made — including whether `create_v2`'s cashback CPI
//! initialized the per-mint `sharing_config` a `buy_v2`/`buy_exact_quote_in_v2`
//! needs. And because a dev-buy of variant X is byte-identical to a CO-BUY of X
//! (both draw the shared `build_curve_buy_core` SSOT), this also verifies the
//! co-buy-leg account layouts.
//!
//! Nothing is submitted; the payer only needs a real on-chain balance so a valid
//! pair simulates as SUCCESS (not insufficient-funds). Read-only diagnostic.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use platform_core::config::Settings;
use platform_core::storage::connect;
use platform_core::storage::repositories::ManagedWalletRepo;
use pump_trader::types::{CreateTokenArgs, CreateTokenV2Args};
use pump_trader::{BundleBuyVariant, DevBuy, PumpFunTrader};
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::VersionedTransaction;
use std::str::FromStr;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};
use crate::trader_config::build_launch_trader_config;

/// Minimum payer balance to simulate a small dev-buy without a false
/// insufficient-funds revert (create rent + 0.01 dev-buy + headroom).
const MIN_PAYER_LAMPORTS: i64 = 30_000_000; // ~0.03 SOL
/// Small dev-buy so any modestly-funded managed wallet can be the sim payer.
const DEV_BUY_LAMPORTS: u64 = 10_000_000; // 0.01 SOL

pub async fn run_launch_sim_matrix(settings: &Settings, _args: &[String]) -> Result<()> {
    let launcher = LauncherSettings::from_env()?;
    let pools = connect(settings).await?;
    let kek = EnvKek::from_passphrase(&launcher.kek_passphrase);

    // Pick the highest-balance managed wallet we hold a key for as the sim
    // creator/payer. Simulation spends nothing, but the payer must hold real SOL
    // so a VALID pair simulates as success rather than an insufficient-funds
    // revert that would mask the account-layout result we're actually testing.
    let mut wallets = ManagedWalletRepo::list_all(&pools.hot, None).await?;
    wallets.sort_by_key(|w| std::cmp::Reverse(w.balance_lamports.unwrap_or(0)));
    let payer = wallets
        .into_iter()
        .find(|w| w.balance_lamports.unwrap_or(0) > MIN_PAYER_LAMPORTS)
        .context("no managed wallet with > ~0.03 SOL to act as the sim payer")?;
    let dev_signer = keystore::resolve_signer(&launcher.keystore_dir, &payer.key_ref, &kek)?;
    let creator = dev_signer.pubkey();

    let nonce_accounts: Vec<Pubkey> = launcher
        .nonce_accounts
        .iter()
        .map(|s| Pubkey::from_str(s).with_context(|| format!("parse nonce pubkey {s}")))
        .collect::<Result<_>>()?;
    let trader_config = build_launch_trader_config(&launcher, dev_signer, nonce_accounts);
    let mut trader = PumpFunTrader::new(trader_config);
    trader.initialize().await.context("initialize pump-trader")?;

    println!("── launch-sim-matrix (Helius simulateTransaction, zero SOL) ──────────");
    println!(
        "payer/creator : {creator}  ({:.4} SOL)",
        payer.balance_lamports.unwrap_or(0) as f64 / 1e9
    );
    println!("dev-buy       : {:.4} SOL per combo (slippage 500bps)", DEV_BUY_LAMPORTS as f64 / 1e9);
    println!("launch ALT    : {:?}", launcher.launch_alt);
    println!("note          : dev-buy of variant X ≡ co-buy of X (shared SSOT), so this");
    println!("                verifies both dev-buy AND co-buy-leg account layouts.");
    println!("─────────────────────────────────────────────────────────────────────");
    println!(
        "{:<20} {:<9} {:<24} {:<12} {}",
        "create", "cashback", "buy_variant", "result", "detail"
    );
    println!("{}", "-".repeat(100));

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

            let built = match create_variant {
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

            // The buyer's base token ATA — track it so a "success" is proven by
            // real tokens landing in it (not a silent no-op). Token program follows
            // the create variant (Legacy SPL for v1, Token-2022 for v2). ATA =
            // PDA([owner, token_program, mint], ATA_PROGRAM).
            let base_token_program = if create_variant.ends_with("v1") {
                pump_trader::protocol::TOKEN
            } else {
                pump_trader::protocol::TOKEN_2022
            };
            let base_ata = Pubkey::find_program_address(
                &[
                    creator.as_ref(),
                    base_token_program.as_ref(),
                    mint.pubkey().as_ref(),
                ],
                &pump_trader::protocol::ASSOCIATED_TOKEN_PROGRAM,
            )
            .0;

            let (result, detail) = match built {
                Ok(tx) => match simulate_tx(&launcher.rpc_url, &tx, &base_ata).await {
                    Ok(sim) if sim.success => (
                        "OK".to_string(),
                        format!(
                            "cu={} tokens_recv={}",
                            sim.units.map(|u| u.to_string()).unwrap_or_else(|| "?".into()),
                            sim.token_after.map(|t| t.to_string()).unwrap_or_else(|| "0".into()),
                        ),
                    ),
                    Ok(sim) => {
                        let code = extract_custom(&sim.err);
                        let hint = sim
                            .logs
                            .iter()
                            .rev()
                            .find(|l| {
                                l.contains("failed") || l.contains("Error") || l.contains("Constraint")
                            })
                            .cloned()
                            .unwrap_or_default();
                        (
                            format!("REVERT{}", code.map(|c| format!(" ({c})")).unwrap_or_default()),
                            format!("{} | {hint}", sim.err),
                        )
                    }
                    Err(e) => ("SIM_ERR".to_string(), e.to_string()),
                },
                Err(e) => ("BUILD_ERR".to_string(), e.to_string()),
            };

            let cb = if create_variant.ends_with("v1") {
                "n/a"
            } else if cashback {
                "on"
            } else {
                "off"
            };
            println!(
                "{:<20} {:<9} {:<24} {:<12} {}",
                create_variant, cb, bv_name, result, detail
            );
        }
        println!();
    }

    println!("─────────────────────────────────────────────────────────────────────");
    println!("OK = simulates cleanly (valid working-pair). REVERT (N) = Anchor custom N");
    println!("(2006 = ConstraintSeeds; a v2 buy against a curve with no sharing_config).");
    Ok(())
}

/// Parsed outcome of one `simulateTransaction`.
struct SimResult {
    success: bool,
    err: String,
    logs: Vec<String>,
    units: Option<u64>,
    /// Post-execution raw token balance of the tracked base ATA — proves the buy
    /// actually filled (tokens landed), not a silent no-op.
    token_after: Option<u64>,
}

/// POST a standard `simulateTransaction` for one signed v0 tx, tracking `track`'s
/// post-execution state. `sigVerify:false` + `replaceRecentBlockhash:true` so
/// neither a stale blockhash nor the (real) signatures gate the result — only the
/// on-chain execution does.
async fn simulate_tx(rpc_url: &str, tx: &VersionedTransaction, track: &Pubkey) -> Result<SimResult> {
    let wire = bincode::serialize(tx).context("serialize tx")?;
    let b64 = STANDARD.encode(wire);
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": [
            b64,
            {
                "sigVerify": false,
                "replaceRecentBlockhash": true,
                "encoding": "base64",
                "commitment": "processed",
                "accounts": { "encoding": "base64", "addresses": [track.to_string()] }
            }
        ]
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .context("simulateTransaction HTTP")?;
    let status = resp.status();
    let text = resp.text().await.context("simulateTransaction body")?;
    if !status.is_success() {
        bail!("simulateTransaction HTTP {status}: {text}");
    }
    let v: Value = serde_json::from_str(&text).context("parse simulateTransaction response")?;
    if let Some(err) = v.get("error") {
        bail!("RPC error: {err}");
    }
    let val = v
        .get("result")
        .and_then(|r| r.get("value"))
        .context("simulateTransaction: no result.value")?;
    let err = val.get("err");
    let success = matches!(err, None | Some(Value::Null));
    let err_str = err.map(|e| e.to_string()).unwrap_or_default();
    let logs = val
        .get("logs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|l| l.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let units = val.get("unitsConsumed").and_then(Value::as_u64);
    // The tracked account's post-exec data (base64 SPL/Token-2022) → raw amount at
    // offset 64 (u64 LE), present in both token layouts. None if the ATA wasn't
    // created / no data returned.
    let token_after = val
        .get("accounts")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|acct| acct.as_object())
        .and_then(|acct| acct.get("data"))
        .and_then(|d| d.as_array())
        .and_then(|d| d.first())
        .and_then(Value::as_str)
        .and_then(|b64| STANDARD.decode(b64).ok())
        .filter(|bytes| bytes.len() >= 72)
        .map(|bytes| u64::from_le_bytes(bytes[64..72].try_into().unwrap()));
    Ok(SimResult { success, err: err_str, logs, units, token_after })
}

/// Pull the Anchor custom error code out of a `simulateTransaction` `err` JSON,
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
