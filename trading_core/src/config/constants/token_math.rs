use super::protocol::LAMPORTS_PER_SOL;

// ---------------------------------------------------------------------------
// Static initial reserve values for Pump.fun tokens
// On-chain defaults observed for newly-created mints; used to compute
// circulating supply = initial_virtual_token_reserves - current_virtual_token_reserves.
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
