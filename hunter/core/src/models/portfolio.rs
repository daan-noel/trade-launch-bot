//! Pure portfolio math — the single unrealized-PnL compute site.
//!
//! CLAUDE.md SSOT: a formula lives in exactly ONE place. Unrealized PnL is derived
//! **here and nowhere else** — the Holdings / Home / Live-Trading surfaces all read
//! it through the `/api/portfolio/*` endpoints; the JS side renders the numbers, it
//! never re-derives them. Mirrors the realized-PnL convention of
//! [`crate::models::strategy::StrategyPosition::realized_pnl_sol`].
//!
//! The mark is **what the wallet would end up with**: the SOL the entry actually
//! took (the cost basis) against the SOL the exit would return — the curve's
//! output for the bag, less the venue fee, the sell's fixed cost and the close.
//! A gross mark (spot × tokens over curve-side cost) omits the entry fee, the exit
//! fee and both legs' fixed cost, all of one sign — ~4 pp of round trip at the live
//! clip size — so it renders green across the range where the position is in fact
//! under water: the `+%`-beside-a-red-`◎` contradiction
//! [`weighted_return_pct`](crate::strategies::kernel::weighted_return_pct)
//! exists to kill.

use serde::Serialize;
use uuid::Uuid;

use crate::models::MarkQuote;
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
    /// SOL the bag still held took from the wallet — fee and fixed cost included.
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

/// Compute unrealized PnL from the SOL a bag cost, a current mark, the held
/// amount, and the pool depth the exit would sell into.
///
/// `current_mark` is SOL per the SAME token quantity `held_amount` is counted in
/// (raw token units throughout this codebase — see
/// [`crate::models::strategy::StrategyPosition::realized_pnl_sol`]), so the SOL
/// outputs come out in human SOL. No basis (`cost_basis_sol ≤ 0`, or nothing
/// held) ⇒ every field is 0 rather than NaN.
///
/// The arithmetic is [`mark_open_bag`] — the cost kernel that also prices the
/// sim, the sweep, and the per-rule `open_pnl_sol`, so an open position means one
/// thing on every surface. `reserve_sol` is the mint's priced SOL depth for the
/// exit's impact; `None` charges no impact rather than a guessed one, exactly as
/// `CostModel::pumpfun_with_impact` degrades everywhere else.
///
/// The percent goes through the shared [`weighted_return_pct`] rather than a
/// price ratio. Routing it through the one formula makes the sign-lock to
/// `unrealized_pnl_sol` structural instead of a coincidence a later edit could
/// quietly break.
pub fn unrealized_pnl(
    cost_basis_sol: f64,
    current_mark: f64,
    held_amount: f64,
    reserve_sol: Option<f64>,
    costs: &CostModel,
) -> UnrealizedPnl {
    if !(cost_basis_sol > 0.0) || !(held_amount > 0.0) {
        return UnrealizedPnl { cost_basis_sol: 0.0, unrealized_pnl_sol: 0.0, unrealized_pnl_pct: 0.0 };
    }
    let unrealized_pnl_sol =
        mark_open_bag(cost_basis_sol, current_mark, held_amount, reserve_sol, costs);
    let unrealized_pnl_pct = weighted_return_pct(unrealized_pnl_sol, cost_basis_sol);
    UnrealizedPnl { cost_basis_sol, unrealized_pnl_sol, unrealized_pnl_pct }
}

/// Mark ONE open strategy position's remaining bag to the live cache quote.
///
/// **The single entry point for every open-position figure.** The per-rule
/// `open_pnl_sol` rollup and the Console's per-position PnL column both resolve a
/// position through here, so an open position's unrealized number cannot mean two
/// things depending on which surface asked. Before this existed the Console had no
/// position-scoped mark at all and joined the *wallet holding* by mint instead —
/// an aggregator USD price over a wallet-wide average-cost basis, which is a
/// different position, a different price universe, and a different denominator.
///
/// Every input comes from the position itself: `cost_basis_sol` is the SOL its
/// entry took, pro-rata to `held_amount` = `entry_token_amount - sold_token_amount`
/// (a scaled-out position marks the half it still owns —
/// `OpenPositionBag::cost_basis_sol`), and `quote` is
/// [`mark_quote`](crate::state::token_cache::mark_quote) — the one definition of
/// what the live cache says a mint is worth, curve-native in SOL per raw unit.
///
/// `None` — never a fabricated zero — when there is nothing honest to mark: no
/// executed entry, no bag left, or no cached price for the mint yet.
pub fn mark_bag(
    cost_basis_sol: f64,
    held_amount: f64,
    quote: Option<MarkQuote>,
    costs: &CostModel,
) -> Option<UnrealizedPnl> {
    if !(cost_basis_sol.is_finite() && cost_basis_sol > 0.0) || !(held_amount > 0.0) {
        return None;
    }
    let quote = quote.filter(|q| q.price.is_finite() && q.price > 0.0)?;
    Some(unrealized_pnl(cost_basis_sol, quote.price, held_amount, quote.reserve_sol, costs))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::strategies::kernel::modeled_cost_basis;

    /// Frictionless: the pure price math, so a cost term cannot hide in the
    /// baseline. Prices are SOL-per-raw-unit and held is raw units — a 2× mark on
    /// a 100-SOL bag of 100 raw units books +100 SOL (100% up).
    #[test]
    fn doubling_mark_books_100pct_gain() {
        let p = unrealized_pnl(100.0, 2.0, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, 100.0);
        assert_eq!(p.unrealized_pnl_pct, 100.0);
    }

    /// A mark below entry is a loss in both SOL and %.
    #[test]
    fn mark_below_entry_is_a_loss() {
        let p = unrealized_pnl(100.0, 0.5, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 100.0);
        assert_eq!(p.unrealized_pnl_sol, -50.0);
        assert_eq!(p.unrealized_pnl_pct, -50.0);
    }

    /// No basis ⇒ pct is 0, not NaN/inf.
    #[test]
    fn zero_basis_has_no_pct() {
        let p = unrealized_pnl(0.0, 2.0, 100.0, None, &CostModel::frictionless());
        assert_eq!(p.cost_basis_sol, 0.0);
        assert_eq!(p.unrealized_pnl_sol, 0.0);
        assert_eq!(p.unrealized_pnl_pct, 0.0);
    }

    /// The defect this module exists to prevent: an unmoved price is NOT break-even.
    /// Both fees, both fixed legs and the close are owed, so a flat mark is red.
    #[test]
    fn a_flat_mark_is_a_loss_not_break_even() {
        let costs = CostModel::pumpfun_fee_only();
        let basis = modeled_cost_basis(1.0, 1.0, &costs);
        let p = unrealized_pnl(basis, 1.0, 1.0, None, &costs);
        assert!(p.unrealized_pnl_sol < 0.0, "flat mark must not read break-even");
        assert!(p.unrealized_pnl_pct < 0.0);
        // Both legs' fee (125 bps each), both legs' fixed cost and the close.
        let expected =
            -(2.0 * 0.0125 + costs.fixed_buy_sol + costs.fixed_sell_sol + costs.close_fee_sol);
        assert!(
            (p.unrealized_pnl_sol - expected).abs() < 1e-12,
            "got {}, want {expected}",
            p.unrealized_pnl_sol
        );
    }

    /// Break-even needs a +3-4% move at the live clip size — the fee and the fixed
    /// legs, asserted so the bar cannot drift out of the docs silently.
    #[test]
    fn break_even_needs_about_four_percent_at_live_clip_size() {
        let costs = CostModel::pumpfun_fee_only();
        // 0.0296 SOL clip (measured median real entry), priced as 1 SOL/token.
        let held = 0.0296;
        let basis = modeled_cost_basis(1.0, held, &costs);
        let flat = unrealized_pnl(basis, 1.0, held, None, &costs);
        let up_four = unrealized_pnl(basis, 1.04, held, None, &costs);
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
            let p = unrealized_pnl(0.0508, mark, 0.05, Some(70.0), &costs);
            assert_eq!(
                p.unrealized_pnl_sol > 0.0,
                p.unrealized_pnl_pct > 0.0,
                "sign split at mark {mark}"
            );
        }
    }

    // ── mark_bag: the one open-position entry point ─────────────────────────────

    /// SSOT: `mark_bag` is `unrealized_pnl` with the guards in front, never a
    /// second formula. If someone reimplements it, this fails.
    #[test]
    fn mark_bag_is_unrealized_pnl() {
        let costs = CostModel::pumpfun_with_impact();
        let quote = MarkQuote { price: 2.0, reserve_sol: Some(70.0) };
        let got = mark_bag(0.0508, 0.05, Some(quote), &costs).expect("markable");
        let want = unrealized_pnl(0.0508, 2.0, 0.05, Some(70.0), &costs);
        assert_eq!(got.cost_basis_sol, want.cost_basis_sol);
        assert_eq!(got.unrealized_pnl_sol, want.unrealized_pnl_sol);
        assert_eq!(got.unrealized_pnl_pct, want.unrealized_pnl_pct);
    }

    /// Nothing honest to mark ⇒ `None`, never a fabricated 0 that would render as
    /// a break-even the position never had. Each guard checked on its own.
    #[test]
    fn mark_bag_declines_rather_than_inventing_a_zero() {
        let costs = CostModel::pumpfun_with_impact();
        let quote = || Some(MarkQuote { price: 2.0, reserve_sol: None });
        // No executed entry (BuySubmitted, fill not adopted yet).
        assert!(mark_bag(0.0, 1.0, quote(), &costs).is_none());
        assert!(mark_bag(f64::NAN, 1.0, quote(), &costs).is_none());
        // Nothing left to mark (fully scaled out, row not yet closed).
        assert!(mark_bag(1.0, 0.0, quote(), &costs).is_none());
        // Mint has no cached price yet (just entered, no post-entry trade).
        assert!(mark_bag(1.0, 1.0, None, &costs).is_none());
        assert!(mark_bag(1.0, 1.0, Some(MarkQuote { price: 0.0, reserve_sol: None }), &costs)
            .is_none());
    }

    /// A scaled-out position marks the bag it STILL holds, against the share of the
    /// entry that bought it.
    #[test]
    fn mark_bag_prices_only_the_remaining_bag() {
        let costs = CostModel::frictionless();
        let quote = MarkQuote { price: 2.0, reserve_sol: None };
        let whole = mark_bag(100.0, 100.0, Some(quote), &costs).expect("markable");
        let half = mark_bag(50.0, 50.0, Some(quote), &costs).expect("markable");
        assert_eq!(whole.unrealized_pnl_sol, 100.0);
        assert_eq!(half.unrealized_pnl_sol, 50.0);
        // Per-SOL-deployed return is unchanged by the scale-out — only the size is.
        assert_eq!(whole.unrealized_pnl_pct, half.unrealized_pnl_pct);
    }
}
