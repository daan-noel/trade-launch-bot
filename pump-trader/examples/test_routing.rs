//! Verify `resolve_buy_routing` against real on-chain accounts — no funds move.
//!
//! For each fixture mint it reads the bonding-curve PDA (creator + `complete`
//! flag) and the mint account owner (token program), then checks the resolved
//! routing against the expected values pulled from the running backend's DB.
//! Finally it dry-runs the AMM buy path for a migrated token via
//! `simulateTransaction` to prove routing → amm_buy builds a valid swap.
//!
//! Reads `HELIUS_RPC_URL` and `WALLET_PRIVATE_KEY` from env / repo `.env`.
//! The wallet is only used to construct the trader — nothing is signed or sent.
//!
//!   cargo run -p pump-trader --example test_routing

use std::sync::Arc;

use anyhow::{Context, Result};
use pump_trader::constants::TOKEN_2022_PROGRAM_ID;
use pump_trader::{PumpFunTrader, TraderConfig};
use solana_sdk::signature::Keypair;

struct Fixture {
    mint: &'static str,
    symbol: &'static str,
    want_migrated: bool,
    want_token_program: &'static str,
    want_creator: &'static str,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let rpc = std::env::var("HELIUS_RPC_URL").context("HELIUS_RPC_URL not set")?;
    let wallet = std::env::var("WALLET_PRIVATE_KEY").context("WALLET_PRIVATE_KEY not set")?;
    let bytes = bs58::decode(&wallet)
        .into_vec()
        .context("decode WALLET_PRIVATE_KEY as base58")?;
    let keypair = Keypair::from_bytes(&bytes).context("Keypair from WALLET_PRIVATE_KEY bytes")?;

    let cfg = Arc::new(TraderConfig {
        rpc_url: rpc.clone(),
        helius_sender_url: rpc,
        keypair,
        nonce_accounts: vec![],
    });
    let trader = PumpFunTrader::new(cfg);

    // Fixtures: is_migrated + creator come from the live backend's /api/tokens
    // DB; token program is the on-chain mint owner (verified via getAccountInfo).
    // All current pump.fun create_v2 tokens are Token-2022, mayhem or not.
    let fixtures = [
        Fixture {
            mint: "BFxRxMGQVc4dWJAzVaUhB6L1ab8FAvaMN5B7V67Fpump",
            symbol: "STORY",
            want_migrated: true,
            want_token_program: TOKEN_2022_PROGRAM_ID,
            want_creator: "DxQ1iNidsGap2kr14VdEzA8NheqXabfh6fRkvZQbnZVY",
        },
        Fixture {
            mint: "8jFCg5jQs3WUJBmnvFyVrnkJ3fccTNJRiyy45dXmpump",
            symbol: "ban",
            want_migrated: true,
            want_token_program: TOKEN_2022_PROGRAM_ID,
            want_creator: "DhSELHEnjs3rFyamjbfW5yxV1mm5kyDCJju2p7E3wgmy",
        },
        Fixture {
            mint: "2ZXWPDVaG19W3P9D9gQcEwsyG9VzYQkxt4m8FPV1pump",
            symbol: "IPO",
            want_migrated: false,
            want_token_program: TOKEN_2022_PROGRAM_ID,
            want_creator: "2cqWjKsG8ugDRJ69yzuTMKqVPeYYGkFJM67BYogvTXyQ",
        },
        Fixture {
            mint: "6NJDKZBnF3B9asFsPr8hGpsfdc6DWK1Lc7yPd29Gpump",
            symbol: "Puffins",
            want_migrated: false,
            want_token_program: TOKEN_2022_PROGRAM_ID,
            want_creator: "5mnfX7wucq4bCgXWssC5mmBBTVtotKS2z9z5sy5uHRWm",
        },
    ];

    let mut failures = 0;
    for f in &fixtures {
        print!("[{:>7}] {} ... ", f.symbol, f.mint);
        match trader.resolve_buy_routing(f.mint).await {
            Ok(r) => {
                let ok_mig = r.is_migrated == f.want_migrated;
                let ok_tp = r.token_program_id == f.want_token_program;
                let ok_creator = r.creator == f.want_creator;
                if ok_mig && ok_tp && ok_creator {
                    println!(
                        "PASS  migrated={} program={} creator={}",
                        r.is_migrated,
                        short(&r.token_program_id),
                        short(&r.creator)
                    );
                } else {
                    failures += 1;
                    println!("FAIL");
                    if !ok_mig {
                        println!("    is_migrated: got {} want {}", r.is_migrated, f.want_migrated);
                    }
                    if !ok_tp {
                        println!(
                            "    token_program: got {} want {}",
                            r.token_program_id, f.want_token_program
                        );
                    }
                    if !ok_creator {
                        println!("    creator: got {} want {}", r.creator, f.want_creator);
                    }
                }
            }
            Err(e) => {
                failures += 1;
                println!("ERROR  {e}");
            }
        }
    }

    // Dry-run the migrated buy path (routing → amm_buy) for STORY using the
    // on-chain-resolved token program — exactly what the handler feeds amm_buy.
    // Builds the real swap instructions and simulates them. No send.
    println!("\n--- AMM buy dry-run (STORY, 0.01 SOL) ---");
    let story_tp = trader
        .resolve_buy_routing(fixtures[0].mint)
        .await?
        .token_program_id;
    match trader
        .amm_simulate_buy(fixtures[0].mint, &story_tp, 0.01, None, None)
        .await
    {
        Ok(sim) => match sim.err {
            None => println!(
                "simulate OK — {} CU consumed (swap instructions are valid)",
                sim.units_consumed.unwrap_or(0)
            ),
            Some(e) => {
                failures += 1;
                println!("simulate returned program error: {e}");
                for l in sim.logs.iter().rev().take(6).rev() {
                    println!("    {l}");
                }
            }
        },
        Err(e) => {
            failures += 1;
            println!("simulate call failed: {e}");
        }
    }

    println!();
    if failures == 0 {
        println!("ALL CHECKS PASSED");
        Ok(())
    } else {
        anyhow::bail!("{failures} check(s) failed")
    }
}

fn short(s: &str) -> String {
    if s.len() > 10 {
        format!("{}…{}", &s[..4], &s[s.len() - 4..])
    } else {
        s.to_string()
    }
}
