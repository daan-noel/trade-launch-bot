use super::protocol::LAMPORTS_PER_SOL;

// ---------------------------------------------------------------------------
// Static initial reserve values for Pump.fun tokens
// On-chain defaults observed for newly-created mints; used to compute
// circulating supply = initial_virtual_token_reserves - current_reserve_token.
// Values are raw token units (no decimal scaling applied here).
// ---------------------------------------------------------------------------

pub const INITIAL_VIRTUAL_TOKEN_RESERVES: f64 = 1073000000000000.0;
pub const INITIAL_VIRTUAL_SOL_RESERVES: f64 = 30000000000.0;
pub const INITIAL_REAL_TOKEN_RESERVES: f64 = 793100000000000.0;
pub const TOKEN_TOTAL_SUPPLY: f64 = 1000000000000000.0;

/// Bonding-curve genesis spot price in whole SOL per raw token unit
/// (`virtual_sol / virtual_token` at curve genesis). Constant for every
/// Pump.fun mint. Used as the dead-token launch baseline when a token has no
/// recorded dev buy, so a no-dev-buy mint is still evaluated against a real
/// floor instead of being silently immune to deadness.
pub const PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN: f64 =
    INITIAL_VIRTUAL_SOL_RESERVES / LAMPORTS_PER_SOL as f64 / INITIAL_VIRTUAL_TOKEN_RESERVES;

/// Initial virtual SOL the pump.fun curve is seeded with, in **whole SOL**
/// (`INITIAL_VIRTUAL_SOL_RESERVES` lamports ÷ `LAMPORTS_PER_SOL` = 30.0). The
/// curve's *real* deposited SOL is `virtual_sol − this`; on the AMM there is no
/// virtual offset. Mirrors the frontend chart's `PUMP_INITIAL_VIRTUAL_SOL`.
pub const PUMP_INITIAL_VIRTUAL_SOL: f64 = INITIAL_VIRTUAL_SOL_RESERVES / LAMPORTS_PER_SOL as f64;

/// Approximate the pool's **real** (non-virtual) SOL reserves from the priced
/// reserve pair, given the row's `venue`. On the AMM the pool balance *is* the
/// real reserve (`real == reserve_sol`); on the curve the real deposited SOL is
/// `reserve_sol − PUMP_INITIAL_VIRTUAL_SOL`, clamped at 0. This matches the
/// "true liquidity" the frontend chart already derives (`chartBars.ts`).
///
/// This is the **approximation** used by the sim/backtest corpus, where the
/// program-emitted `real_sol_reserves` field isn't carried (see
/// `lab::lake::duck`). The live/paper paths use the program's exact emitted
/// value instead and must NOT go through this.
pub fn approx_real_sol_reserves(reserve_sol: f64, venue: &str) -> f64 {
    if venue == "amm" {
        reserve_sol
    } else {
        (reserve_sol - PUMP_INITIAL_VIRTUAL_SOL).max(0.0)
    }
}

/// Total token supply (raw units) for a token, accounting for Mayhem-mode
/// tokens which are minted via `create_v2` with 2× the standard supply
/// (2B vs 1B). Use this anywhere FDV / market cap is computed as `supply × price`.
pub fn total_supply_for(is_mayhem_mode: bool) -> f64 {
    if is_mayhem_mode {
        TOKEN_TOTAL_SUPPLY * 2.0
    } else {
        TOKEN_TOTAL_SUPPLY
    }
}
