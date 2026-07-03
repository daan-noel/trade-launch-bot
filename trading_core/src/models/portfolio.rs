//! Pure portfolio math — the single unrealized-PnL compute site.
//!
//! CLAUDE.md SSOT: a formula lives in exactly ONE place. Unrealized PnL is derived
//! **here and nowhere else** — the Holdings / Home / Live-Trading surfaces all read
//! it through the `/api/portfolio/*` endpoints; the JS side renders the numbers, it
//! never re-derives them. Mirrors the realized-PnL conventions of
//! [`crate::models::position::Position::pnl_sol`] and
//! [`crate::models::strategy::StrategyPosition::realized_pnl_sol`].

use serde::Serialize;

/// Unrealized PnL of an open bag, in human SOL. Price is SOL per token unit and
/// amount is that same token unit, so `price × amount` is human SOL (same
/// convention as [`crate::models::position::Position::pnl_sol`]).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct UnrealizedPnl {
    /// Remaining bag's cost basis: `avg_entry_price × held_amount`.
    pub cost_basis_sol: f64,
    /// Mark-to-market gain/loss: `(current_mark − avg_entry_price) × held_amount`.
    pub unrealized_pnl_sol: f64,
    /// Percent off entry: `(current_mark − avg_entry_price) / avg_entry_price × 100`
    /// (0 when there is no basis).
    pub unrealized_pnl_pct: f64,
}

/// Compute unrealized PnL from an average entry, a current mark, and the held
/// amount.
///
/// **All three share one unit basis**: `avg_entry_price` and `current_mark` are SOL
/// per the SAME token quantity `held_amount` is counted in (raw token units
/// throughout this codebase — see [`crate::models::position::Position::pnl_sol`]),
/// so the SOL outputs come out in human SOL. `avg_entry_price ≤ 0` ⇒ no basis, so
/// `unrealized_pnl_pct` falls back to 0 (the SOL fields still reflect the mark).
pub fn unrealized_pnl(avg_entry_price: f64, current_mark: f64, held_amount: f64) -> UnrealizedPnl {
    let cost_basis_sol = avg_entry_price * held_amount;
    let current_value_sol = current_mark * held_amount;
    let unrealized_pnl_sol = current_value_sol - cost_basis_sol;
    let unrealized_pnl_pct = if avg_entry_price > 0.0 {
        (current_mark - avg_entry_price) / avg_entry_price * 100.0
    } else {
        0.0
    };
    UnrealizedPnl { cost_basis_sol, unrealized_pnl_sol, unrealized_pnl_pct }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prices are SOL-per-raw-unit and held is raw units, so the math is identical
    /// to `Position::pnl_sol` on the same numbers — a 2× mark on a 1.0-entry bag of
    /// 100 raw units books +100 SOL (100% up).
    #[test]
    fn doubling_mark_books_100pct_gain() {
        let p = unrealized_pnl(1.0, 2.0, 100.0);
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, 100.0);
        assert_eq!(p.unrealized_pnl_pct, 100.0);
    }

    /// A mark below entry is a loss in both SOL and %.
    #[test]
    fn mark_below_entry_is_a_loss() {
        let p = unrealized_pnl(1.0, 0.5, 100.0);
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, -50.0);
        assert_eq!(p.unrealized_pnl_pct, -50.0);
    }

    /// No basis (zero entry) ⇒ pct is 0, not NaN/inf; SOL still tracks the mark.
    #[test]
    fn zero_entry_has_no_pct_but_marks_value() {
        let p = unrealized_pnl(0.0, 2.0, 100.0);
        assert_eq!(p.cost_basis_sol, 0.0);
        assert_eq!(p.unrealized_pnl_sol, 200.0);
        assert_eq!(p.unrealized_pnl_pct, 0.0);
    }
}
