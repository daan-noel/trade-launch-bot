//! Dry-run a PumpSwap AMM buy/sell via `simulateTransaction` — no funds move.
//!
//! Validates the account layout / derivations against a real migrated token
//! before any live AMM send is enabled.
//!
//! Reads `HELIUS_RPC_URL` and `WALLET_PRIVATE_KEY` (base58) from the environment
//! or the repo `.env`. Usage:
//!
//!   cargo run -p pump-trader --example amm_simulate -- <mint> <buy|sell> <amount> [base_token_program_id] [pool]
//!
//! `buy` amount is SOL (f64); `sell` amount is raw base-token units (u64).

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use pump_trader::constants::TOKEN_PROGRAM_ID;
use pump_trader::{PumpFunTrader, TraderConfig};
use solana_sdk::signature::Keypair;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: amm_simulate <mint> <buy|sell> <amount> [base_token_program_id] [pool]\n\
             \n  buy  amount = SOL (f64)\n  sell amount = raw base-token units (u64)"
        );
        std::process::exit(2);
    }
    let mint = &args[1];
    let side = args[2].as_str();
    let amount = &args[3];
    let base_tp = args.get(4).map(String::as_str).unwrap_or(TOKEN_PROGRAM_ID);
    let pool = args.get(5).filter(|s| !s.is_empty() && s.as_str() != "-").map(String::as_str);

    let rpc = std::env::var("HELIUS_RPC_URL").context("HELIUS_RPC_URL not set")?;
    let wallet = std::env::var("WALLET_PRIVATE_KEY").context("WALLET_PRIVATE_KEY not set")?;
    let bytes = bs58::decode(&wallet)
        .into_vec()
        .context("decode WALLET_PRIVATE_KEY as base58")?;
    let keypair = Keypair::from_bytes(&bytes).context("Keypair from WALLET_PRIVATE_KEY bytes")?;

    let cfg = Arc::new(TraderConfig {
        rpc_url: rpc.clone(),
        helius_sender_url: rpc, // unused for simulate
        keypair,
        nonce_accounts: vec![], // unused for simulate
    });
    let trader = PumpFunTrader::new(cfg);

    let sim = match side {
        "buy" => {
            let sol: f64 = amount.parse().context("buy amount must be a SOL float")?;
            trader.amm_simulate_buy(mint, base_tp, sol, pool, None).await?
        }
        "sell" => {
            let amt: u64 = amount.parse().context("sell amount must be raw u64 units")?;
            trader
                .amm_simulate_sell(mint, amt, base_tp, pool, None, None)
                .await?
        }
        other => bail!("side must be 'buy' or 'sell', got '{other}'"),
    };

    println!("err            : {:?}", sim.err);
    println!("units_consumed : {:?}", sim.units_consumed);
    println!("--- logs ---");
    for l in &sim.logs {
        println!("{l}");
    }
    if sim.err.is_some() {
        std::process::exit(1);
    }
    Ok(())
}
