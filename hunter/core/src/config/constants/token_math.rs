use super::protocol::LAMPORTS_PER_SOL;

// ---------------------------------------------------------------------------
// Static initial reserve values for Pump.fun tokens
// On-chain defaults observed for newly-created mints; used to compute
// circulating supply = initial_virtual_token_reserves - current_reserve_token.
// Values are raw token units (no decimal scaling applied here).
// ---------------------------------------------------------------------------

pub const INITIAL_VIRTUAL_TOKEN_RESERVES: f64 = 1073000000000000.0;
pub const INITIAL_VIRTUAL_SOL_RESERVES: f64 = 30000000000.0;
pub const TOKEN_TOTAL_SUPPLY: f64 = 1000000000000000.0;

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

/// Market cap (FDV) in whole SOL = spot price × total token supply. Uses the token's
/// actual on-chain `initial_supply_token` when known — the same quantity the
/// SQL/enrichment canonical (`MARKET_CAP_SQL` = `current_price × initial_supply_token`)
/// uses — so the live in-RAM market cap and the DB-computed value agree instead of
/// silently diverging. Falls back to the mayhem-aware constant supply only when the
/// per-token value is unavailable, so a live cell is never blank (the SQL side yields
/// NULL there). THE single in-RAM market-cap definition (`token_cache` + `seed`).
pub fn market_cap_sol(price: f64, initial_supply_token: Option<u64>, is_mayhem_mode: bool) -> f64 {
    let supply = initial_supply_token
        .map(|s| s as f64)
        .unwrap_or_else(|| total_supply_for(is_mayhem_mode));
    supply * price
}

/// SOL (human `f64`) → lamports (exact `i64`), **rounded**. THE `trading_core`
/// DB-boundary conversion for every `_sol` model field persisted into a `_lamports`
/// BIGINT column (SOL/lamports naming rule). Rounds rather than truncates so a value
/// survives a SOL→lamports→SOL round-trip exactly. Every repo (`trade`/`token`/
/// `token_info`/`strategy`) shares this one definition instead of a private copy.
/// The `pump-trader` crate keeps its own `u64` truncating variant by design
/// (standalone-crate decoupling) — do not route trade-execution through this.
pub fn sol_to_lamports(sol: f64) -> i64 {
    (sol * LAMPORTS_PER_SOL as f64).round() as i64
}

/// Lamports (exact `i64`) → SOL (human `f64`). Inverse of [`sol_to_lamports`]; the
/// shared repo-boundary read conversion.
pub fn lamports_to_sol(lamports: i64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL as f64
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn market_cap_uses_per_token_supply_then_falls_back() {
        // Per-token supply present → price × initial_supply_token (matches MARKET_CAP_SQL).
        assert_eq!(market_cap_sol(2.0, Some(1_000), false), 2_000.0);
        // Absent → mayhem-aware constant supply fallback (never blank).
        assert_eq!(market_cap_sol(1.0, None, false), TOKEN_TOTAL_SUPPLY);
        assert_eq!(market_cap_sol(1.0, None, true), TOKEN_TOTAL_SUPPLY * 2.0);
    }

    #[test]
    fn sol_lamports_round_trips_and_rounds() {
        assert_eq!(sol_to_lamports(1.5), 1_500_000_000);
        // Rounds, not truncates: 0.0000000015 SOL = 1.5 lamports → 2.
        assert_eq!(sol_to_lamports(0.0000000015), 2);
        assert!((lamports_to_sol(1_500_000_000) - 1.5).abs() < 1e-12);
        assert_eq!(sol_to_lamports(lamports_to_sol(42)), 42);
    }
}
