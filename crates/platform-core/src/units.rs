//! Unit conversions — the single source of truth for base-unit ↔ human-display math.
//!
//! Lifted by copy from meme-trading's `config::constants::token_math` (the plan's
//! sanctioned SSOT copy), then **generalized**: the "store integer base units,
//! display float, unit-in-name" discipline is preserved, but the unit is now the
//! *referenced quote/base asset's decimals*, not a hard-coded lamport.
//!
//! Naming rule (locked): a column/field denoting an amount names its unit as a
//! **suffix** — `amount_quote` / `amount_base` (exact integer base units, `i64`),
//! human values derived here. Ratios (price/pct) stay `f64` and keep `_price` /
//! `_pct`. Native SOL is just the QuoteAsset whose base unit is the lamport
//! (decimals 9), so the lamport pair below is the `decimals == 9` special case of
//! the generic converters.

/// Lamports per whole SOL. Native-SOL quote asset has `decimals = 9`, i.e.
/// `10_i64.pow(9)` base units per display unit.
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Base units per one display unit of an asset with `decimals` decimals
/// (e.g. 1e9 for SOL, 1e6 for USDC). The generic form of `LAMPORTS_PER_SOL`.
#[inline]
pub fn base_units_per_display(decimals: u8) -> f64 {
    10_f64.powi(decimals as i32)
}

/// Human display amount (`f64`) → exact integer base units (`i64`), **rounded**.
/// THE DB-boundary write conversion for any `_*` model field persisted into an
/// `amount_*` BIGINT column. Rounds (not truncates) so a value survives a
/// display→base→display round-trip exactly. `decimals` comes from the row's
/// `quote_assets` / token dimension — never hard-coded.
#[inline]
pub fn to_base_units(display: f64, decimals: u8) -> i64 {
    (display * base_units_per_display(decimals)).round() as i64
}

/// Exact integer base units (`i64`) → human display amount (`f64`). Inverse of
/// [`to_base_units`]; the shared repo-boundary read conversion.
#[inline]
pub fn to_display(base_units: i64, decimals: u8) -> f64 {
    base_units as f64 / base_units_per_display(decimals)
}

/// Native-SOL convenience: whole SOL (`f64`) → lamports (`i64`), rounded. The
/// `decimals == 9` case of [`to_base_units`]; kept as a named pair because native
/// SOL is pervasive and self-documenting call sites read better than `_, 9`.
#[inline]
pub fn sol_to_lamports(sol: f64) -> i64 {
    to_base_units(sol, 9)
}

/// Native-SOL convenience: lamports (`i64`) → whole SOL (`f64`). Inverse of
/// [`sol_to_lamports`]; the `decimals == 9` case of [`to_display`].
#[inline]
pub fn lamports_to_sol(lamports: i64) -> f64 {
    to_display(lamports, 9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sol_lamports_round_trips_and_rounds() {
        assert_eq!(sol_to_lamports(1.5), 1_500_000_000);
        // Rounds, not truncates: 0.0000000015 SOL = 1.5 lamports → 2.
        assert_eq!(sol_to_lamports(0.0000000015), 2);
        assert!((lamports_to_sol(1_500_000_000) - 1.5).abs() < 1e-12);
        assert_eq!(sol_to_lamports(lamports_to_sol(42)), 42);
    }

    #[test]
    fn generic_decimals_match_native_and_usdc() {
        // Native SOL (9) matches the named pair.
        assert_eq!(to_base_units(1.5, 9), sol_to_lamports(1.5));
        assert_eq!(to_display(1_500_000_000, 9), lamports_to_sol(1_500_000_000));
        // USDC (6): 2.5 USDC = 2_500_000 base units.
        assert_eq!(to_base_units(2.5, 6), 2_500_000);
        assert!((to_display(2_500_000, 6) - 2.5).abs() < 1e-12);
    }
}
