//! PDA (Program-Derived Address) helpers shared across backend crates.

use std::str::FromStr;

use anyhow::Context;
use solana_sdk::pubkey::Pubkey;

use crate::config::constants::{PUMP_SWAP_PROGRAM_ID, WSOL_MINT};

/// Derive the canonical PumpSwap pool address for a migrated pump.fun token.
///
/// Migrated coins use a fixed pool layout: the pool's creator is the pump
/// program PDA `["pool-authority", mint]`, and the pool itself is the PumpSwap
/// PDA `["pool", 0u16, pool_authority, base_mint, WSOL]` (canonical index 0,
/// WSOL quote mint).
pub fn derive_pump_swap_pool(mint: &str, pump_program_id: &str) -> anyhow::Result<String> {
    let mint_pk = Pubkey::from_str(mint).context("invalid mint pubkey")?;
    let pump = Pubkey::from_str(pump_program_id).context("invalid pump program id")?;
    let swap = Pubkey::from_str(PUMP_SWAP_PROGRAM_ID).context("invalid pump swap program id")?;
    let wsol = Pubkey::from_str(WSOL_MINT).context("invalid wsol mint")?;

    let (authority, _) =
        Pubkey::find_program_address(&[b"pool-authority", mint_pk.as_ref()], &pump);
    let index: u16 = 0;
    let (pool, _) = Pubkey::find_program_address(
        &[
            b"pool",
            &index.to_le_bytes(),
            authority.as_ref(),
            mint_pk.as_ref(),
            wsol.as_ref(),
        ],
        &swap,
    );
    Ok(pool.to_string())
}
