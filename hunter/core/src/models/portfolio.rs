//! Pure portfolio math — the single unrealized-PnL compute site.
//!
//! CLAUDE.md SSOT: a formula lives in exactly ONE place. Unrealized PnL is derived
//! **here and nowhere else** — the Holdings / Home / Live-Trading surfaces all read
//! it through the `/api/portfolio/*` endpoints; the JS side renders the numbers, it
//! never re-derives them. Mirrors the realized-PnL convention of
//! [`crate::models::strategy::StrategyPosition::realized_pnl_sol`].
//!
//! The mark is **net of the round trip**, and that is load-bearing rather than a
//! refinement. A gross mark omits four terms, all of one sign: the entry fee and
//! the exit fee (125 bps each — `avg_entry_price` is the *curve-side* amount, so
//! neither is in it), and both legs' tip + priority. At the live clip size those
//! fixed costs are the larger half: measured 0.77% of notional per leg on a
//! 0.0296 SOL median clip, for ~4 pp of round trip in total. A bag needs roughly
//! a +4% move to be worth anything, so a gross mark renders green across the
//! entire range where the position is in fact under water — the same
//! `+%`-beside-a-red-`◎` contradiction
//! [`weighted_return_pct`](crate::strategies::kernel::weighted_return_pct)
//! exists to kill.

use serde::Serialize;
use uuid::Uuid;

use crate::strategies::kernel::{mark_open_bag, weighted_return_pct, CostModel};

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
///
/// **Net, not gross** — every field is what closing the bag right now would
/// leave, priced through [`mark_open_bag`]. See this module's header for why the
/// a gross mark reads ~4 pp high at live clip sizes.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct UnrealizedPnl {
    /// Capital the bag actually consumed: the curve-side cost
    /// `avg_entry_price × held_amount`, **plus** the entry fee and the entry
    /// leg's fixed cost, neither of which is inside `avg_entry_price`.
    pub cost_basis_sol: f64,
    /// Mark-to-market gain/loss net of the round trip: what selling the bag at
    /// `current_mark` would leave, minus [`Self::cost_basis_sol`].
    pub unrealized_pnl_sol: f64,
    /// `unrealized_pnl_sol / cost_basis_sol × 100` — the mark over the capital
    /// still deployed in the bag (0 when there is no basis). Same
    /// [`weighted_return_pct`](crate::strategies::kernel::weighted_return_pct)
    /// definition every other percent in the codebase uses, so it is sign-locked
    /// to `unrealized_pnl_sol` and comparable to the realized
    /// [`StrategyPosition::pnl_pct`](crate::models::strategy::StrategyPosition::pnl_pct).
    pub unrealized_pnl_pct: f64,
}

/// Compute unrealized PnL from an average entry, a current mark, the held amount,
/// and the pool depth the exit would sell into.
///
/// **All three price/amount inputs share one unit basis**: `avg_entry_price` and
/// `current_mark` are SOL per the SAME token quantity `held_amount` is counted in
/// (raw token units throughout this codebase — see
/// [`crate::models::strategy::StrategyPosition::realized_pnl_sol`]), so the SOL
/// outputs come out in human SOL. No basis (`avg_entry_price ≤ 0`, or nothing
/// held) ⇒ every field is 0 rather than NaN.
///
/// The arithmetic is [`mark_open_bag`] — the cost kernel that also prices the
/// sim, the sweep, and the per-rule `open_pnl_sol`, so an open position means one
/// thing on every surface. `reserve_sol` is the mint's SOL-side depth for the
/// exit's impact; `None` charges no impact rather than a guessed one, exactly as
/// `CostModel::pumpfun_with_impact` degrades everywhere else.
///
/// The percent goes through the shared [`weighted_return_pct`] rather than a
/// price ratio. Routing it through the one formula makes the sign-lock to
/// `unrealized_pnl_sol` structural instead of a coincidence a later edit could
/// quietly break.
pub fn unrealized_pnl(
    avg_entry_price: f64,
    current_mark: f64,
    held_amount: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
) -> UnrealizedPnl {
    let (cost_basis_sol, unrealized_pnl_sol) =
        mark_open_bag(avg_entry_price, current_mark, held_amount, reserve_sol, costs);
    let unrealized_pnl_pct = weighted_return_pct(unrealized_pnl_sol, cost_basis_sol);
    UnrealizedPnl { cost_basis_sol, unrealized_pnl_sol, unrealized_pnl_pct }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frictionless: the pure price math, so a cost term cannot hide in the
    /// baseline. Prices are SOL-per-raw-unit and held is raw units — a 2× mark on
    /// a 1.0-entry bag of 100 raw units books +100 SOL (100% up).
    #[test]
    fn doubling_mark_books_100pct_gain() {
        let p = unrealized_pnl(1.0, 2.0, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, 100.0);
        assert_eq!(p.unrealized_pnl_pct, 100.0);
    }

    /// A mark below entry is a loss in both SOL and %.
    #[test]
    fn mark_below_entry_is_a_loss() {
        let p = unrealized_pnl(1.0, 0.5, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, -50.0);
        assert_eq!(p.unrealized_pnl_pct, -50.0);
    }

    /// No basis (zero entry) ⇒ pct is 0, not NaN/inf.
    #[test]
    fn zero_entry_has_no_pct() {
        let p = unrealized_pnl(0.0, 2.0, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 0.0);
        assert_eq!(p.unrealized_pnl_sol, 0.0);
        assert_eq!(p.unrealized_pnl_pct, 0.0);
    }

    /// The defect this module exists to prevent: an unmoved price is NOT break-even.
    /// Both fees and both fixed legs are still owed, so a flat mark is red.
    #[test]
    fn a_flat_mark_is_a_loss_not_break_even() {
        let costs = CostModel::pumpfun_fee_only();
        let p = unrealized_pnl(1.0, 1.0, 1.0, None, &costs);
        assert!(p.unrealized_pnl_sol < 0.0, "flat mark must not read break-even");
        assert!(p.unrealized_pnl_pct < 0.0);
        // Both legs' fee (125 bps each) plus both legs' fixed cost, and nothing else.
        let expected = -(2.0 * 0.0125 + 2.0 * costs.fixed_cost_sol_per_leg);
        assert!(
            (p.unrealized_pnl_sol - expected).abs() < 1e-12,
            "got {}, want {expected}",
            p.unrealized_pnl_sol
        );
    }

    /// Break-even needs roughly a +4% move at the live clip size — the number the
    /// module header quotes, asserted so it cannot drift out of the docs silently.
    #[test]
    fn break_even_needs_about_four_percent_at_live_clip_size() {
        let costs = CostModel::pumpfun_fee_only();
        // 0.0296 SOL clip (measured median real entry), priced as 1 SOL/token.
        let held = 0.0296;
        let flat = unrealized_pnl(1.0, 1.0, held, None, &costs);
        let up_four = unrealized_pnl(1.0, 1.04, held, None, &costs);
        assert!(flat.unrealized_pnl_pct < -3.0 && flat.unrealized_pnl_pct > -5.0);
        assert!(
            up_four.unrealized_pnl_pct.abs() < 1.5,
            "+4% should land near break-even, got {}",
            up_four.unrealized_pnl_pct
        );
    }

    /// Depth charges the exit's impact; no depth charges none (never a guess).
    #[test]
    fn depth_charges_exit_impact() {
        let costs = CostModel::pumpfun_with_impact();
        let deep = unrealized_pnl(1.0, 2.0, 1.0, Some(1_000.0), &costs);
        let shallow = unrealized_pnl(1.0, 2.0, 1.0, Some(10.0), &costs);
        let none = unrealized_pnl(1.0, 2.0, 1.0, None, &costs);
        assert!(shallow.unrealized_pnl_sol < deep.unrealized_pnl_sol);
        assert!(deep.unrealized_pnl_sol < none.unrealized_pnl_sol);
    }

    /// Sign-lock: the percent never disagrees with the SOL beside it.
    #[test]
    fn pct_is_sign_locked_to_sol() {
        let costs = CostModel::pumpfun_with_impact();
        for mark in [0.0, 0.5, 1.0, 1.02, 1.04, 2.0, 10.0] {
            let p = unrealized_pnl(1.0, mark, 0.05, Some(70.0), &costs);
            assert_eq!(
                p.unrealized_pnl_sol > 0.0,
                p.unrealized_pnl_pct > 0.0,
                "sign split at mark {mark}"
            );
        }
    }
}
