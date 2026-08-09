//! Pure portfolio math — the single unrealized-PnL compute site.
//!
//! CLAUDE.md SSOT: a formula lives in exactly ONE place. Unrealized PnL is derived
//! **here and nowhere else** — the Holdings / Home / Live-Trading surfaces all read
//! it through the `/api/portfolio/*` endpoints; the JS side renders the numbers, it
//! never re-derives them. Mirrors the realized-PnL convention of
//! [`crate::models::strategy::StrategyPosition::realized_pnl_sol`].

use serde::Serialize;
use uuid::Uuid;

use crate::strategies::kernel::weighted_return_pct;

/// "Who manages this mint" — one open (unsettled) strategy position, tagged with
/// its rule's human name. The cross-strategy bot-correlation read backing the
/// Holdings bot badge and (later) the Trade-page interlock: a manual sell must not
/// race a live strategy's own exit (the double-sell hard constraint). Produced by
/// `StrategyRepo::managed_mints`; a mint with no open position is simply absent.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedMint {
    pub mint_address: String,
    /// Owning rule (`None` only for a malformed/legacy row with no `rule_id`).
    pub rule_id: Option<Uuid>,
    /// Human rule label (`None` if the rule was deleted out from under the position).
    pub rule_name: Option<String>,
    /// Open-partition status (never `End`/`EntryFailed`).
    pub status: String,
    /// Execution mode: `real` | `paper`.
    pub mode: String,
}

/// Unrealized PnL of an open bag, in human SOL. Price is SOL per token unit and
/// amount is that same token unit, so `price × amount` is human SOL (same
/// convention as [`crate::models::strategy::StrategyPosition::realized_pnl_sol`]).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct UnrealizedPnl {
    /// Remaining bag's cost basis: `avg_entry_price × held_amount`.
    pub cost_basis_sol: f64,
    /// Mark-to-market gain/loss: `(current_mark − avg_entry_price) × held_amount`.
    pub unrealized_pnl_sol: f64,
    /// `unrealized_pnl_sol / cost_basis_sol × 100` — the mark over the capital
    /// still deployed in the bag (0 when there is no basis). Same
    /// [`weighted_return_pct`](crate::strategies::kernel::weighted_return_pct)
    /// definition every other percent in the codebase uses, so it is sign-locked
    /// to `unrealized_pnl_sol`. It is a **gross** mark: an open bag has not paid
    /// an exit leg yet, unlike the realized
    /// [`StrategyPosition::pnl_pct`](crate::models::strategy::StrategyPosition::pnl_pct).
    pub unrealized_pnl_pct: f64,
}

/// Compute unrealized PnL from an average entry, a current mark, and the held
/// amount.
///
/// **All three share one unit basis**: `avg_entry_price` and `current_mark` are SOL
/// per the SAME token quantity `held_amount` is counted in (raw token units
/// throughout this codebase — see
/// [`crate::models::strategy::StrategyPosition::realized_pnl_sol`]), so the SOL
/// outputs come out in human SOL. No basis (`avg_entry_price ≤ 0`, or nothing
/// held) ⇒ `unrealized_pnl_pct` falls back to 0 (the SOL fields still reflect the
/// mark).
///
/// The percent goes through the shared [`weighted_return_pct`] rather than the
/// equivalent price ratio `(current_mark − avg_entry_price) / avg_entry_price`.
/// The two are algebraically identical here — `held_amount` cancels — but routing
/// it through the one formula makes the sign-lock to `unrealized_pnl_sol`
/// structural instead of a coincidence that a later edit could quietly break.
pub fn unrealized_pnl(avg_entry_price: f64, current_mark: f64, held_amount: f64) -> UnrealizedPnl {
    let cost_basis_sol = avg_entry_price * held_amount;
    let current_value_sol = current_mark * held_amount;
    let unrealized_pnl_sol = current_value_sol - cost_basis_sol;
    let unrealized_pnl_pct = weighted_return_pct(unrealized_pnl_sol, cost_basis_sol);
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
