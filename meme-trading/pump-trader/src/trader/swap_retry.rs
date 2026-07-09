// ============================================================
// Swap-revert retry classification — SSOT.
//
// A landed-and-reverted buy/sell carries the program's Anchor custom error
// code (see `tx::custom_error_code`). This module is the single place that
// maps `(code, venue, direction)` to a retry decision, shared by:
//   - this crate's own confirm=true in-call self-heal (`sell.rs`'s
//     `execute_sell`, `buy.rs`'s `buy_token_inner`, `amm.rs`'s `amm_sell`/
//     `amm_buy`) — the sync trigger, since `confirm_transaction` surfaces the
//     revert synchronously (see `tx.rs:226`);
//   - `live`'s feed-confirmed bot loop (`strategies::execution::real`), which
//     depends on this crate and calls `classify_swap_revert` directly instead
//     of keeping its own copy — the async trigger, since a `confirm=false`
//     send only learns the outcome from the LaserStream feed / a later
//     `signature_state_detailed` poll.
//
// Two triggers, one shared decision. Codes classified here:
//   - 2006 stale-creator (both directions, both routes);
//   - the sell slippage floors (6003 curve, 6004 AMM), 6024 missing-UVA,
//     6005 already-migrated (sell-only — these keep their curve/sell behavior);
//   - the BUY slippage codes (curve 6002 `TooMuchSolRequired` / 6042
//     `BuySlippageBelowMinTokensOut`; AMM 6004 `ExceededSlippage` shared with
//     sell / 6040 `BuySlippageBelowMinBaseAmountOut`) → `Retry`, so a
//     slippage-reverted entry re-quotes and resends instead of giving up.
// Every other code falls through to `StopFeeBurn` (a blind resend would only
// re-pay fees). See CLAUDE.md SSOT rule.
// ============================================================

/// The venue a swap executed against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapRoute {
    Curve,
    Amm,
}

/// Which side of the swap reverted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapDirection {
    Buy,
    Sell,
}

/// Retry decision for a landed-and-reverted swap. Pure data — callers own the
/// action (refresh + rebuild + resend, or give up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapRetryDecision {
    /// Transient (e.g. slippage floor missed) — re-quote and resend.
    Retry,
    /// Structural/unknown revert on a route the retry would reuse — stop
    /// rather than re-pay fees on a blind resend.
    StopFeeBurn,
    /// Curve `ConstraintSeeds` (2006): `bonding_curve.creator` rotated (pump
    /// `set_creator`) after the cache was populated — refresh the creator
    /// vault, retry.
    RefreshCreator,
    /// AMM `ConstraintSeeds` (2006): the pool's `coin_creator` rotated after
    /// it was cached — evict + re-read the pool, retry.
    RefreshCoinCreator,
    /// Curve `MissingUserVolumeAccumulator` (6024): cashback UVA was missing
    /// — re-read the cashback flag, retry.
    RefreshCashback,
    /// Curve `BondingCurveComplete` (6005): token already migrated — re-read
    /// migration state, re-route to the AMM.
    RerouteMigrated,
}

/// pump.fun bonding-curve `TooLittleSolReceived` — the curve SELL slippage floor.
pub const CURVE_TOO_LITTLE_SOL_RECEIVED: u32 = 6003;
/// pump.fun bonding-curve `TooMuchSolRequired` — the curve BUY slippage ceiling
/// (the buyer's `max_sol_cost` was exceeded by the live price).
pub const CURVE_TOO_MUCH_SOL_REQUIRED: u32 = 6002;
/// pump.fun bonding-curve `BuySlippageBelowMinTokensOut` — the curve BUY
/// min-tokens-out floor.
pub const CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT: u32 = 6042;
/// PumpSwap AMM `ExceededSlippage` — the AMM-side slippage floor (shared by AMM
/// buys and sells).
pub const AMM_EXCEEDED_SLIPPAGE: u32 = 6004;
/// PumpSwap AMM `BuySlippageBelowMinBaseAmountOut` — the AMM BUY min-base-out floor.
pub const AMM_BUY_SLIPPAGE_BELOW_MIN_BASE: u32 = 6040;
/// pump.fun `BondingCurveComplete`: token already migrated to the AMM but the
/// caller's cache still had `is_migrated = false`.
pub const BONDING_CURVE_COMPLETE: u32 = 6005;
/// Anchor `ConstraintSeeds`: on a curve swap it means pump.fun rotated
/// `bonding_curve.creator` after a cache read; on an AMM swap it means the
/// pool's `coin_creator` was populated/rotated after it was cached.
pub const ANCHOR_CONSTRAINT_SEEDS: u32 = 2006;
/// pump.fun `MissingUserVolumeAccumulator`: cashback UVA account not included
/// in the tx because the cache had `is_cashback = false`.
pub const CURVE_MISSING_USER_VOLUME_ACCUMULATOR: u32 = 6024;

/// Map a landed-and-reverted swap's on-chain custom error code to a retry
/// decision. `route` is the venue the reverted tx used; `direction` is buy vs
/// sell. Pure — no RPC, no state.
pub fn classify_swap_revert(
    custom: Option<u32>,
    route: SwapRoute,
    direction: SwapDirection,
) -> SwapRetryDecision {
    use SwapDirection::{Buy, Sell};
    use SwapRoute::{Amm, Curve};

    match (custom, route, direction) {
        (Some(CURVE_TOO_LITTLE_SOL_RECEIVED), Curve, Sell) => SwapRetryDecision::Retry,
        (Some(AMM_EXCEEDED_SLIPPAGE), Amm, Sell) => SwapRetryDecision::Retry,
        // Buy slippage floors — re-quote and resend (the entry-path analogue of
        // the sell floors above). 6004 is shared buy+sell on the AMM route.
        (Some(CURVE_TOO_MUCH_SOL_REQUIRED | CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT), Curve, Buy) => {
            SwapRetryDecision::Retry
        }
        (Some(AMM_EXCEEDED_SLIPPAGE | AMM_BUY_SLIPPAGE_BELOW_MIN_BASE), Amm, Buy) => {
            SwapRetryDecision::Retry
        }
        // 2006 is the only code generalized across both directions — the
        // stale-creator cache is equally wrong for a buy or a sell.
        (Some(ANCHOR_CONSTRAINT_SEEDS), Curve, _) => SwapRetryDecision::RefreshCreator,
        (Some(ANCHOR_CONSTRAINT_SEEDS), Amm, _) => SwapRetryDecision::RefreshCoinCreator,
        (Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR), Curve, Sell) => {
            SwapRetryDecision::RefreshCashback
        }
        (Some(BONDING_CURVE_COMPLETE), Curve, Sell) => SwapRetryDecision::RerouteMigrated,
        _ => SwapRetryDecision::StopFeeBurn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_creator_2006_refreshes_by_route_regardless_of_direction() {
        for direction in [SwapDirection::Buy, SwapDirection::Sell] {
            assert_eq!(
                classify_swap_revert(Some(ANCHOR_CONSTRAINT_SEEDS), SwapRoute::Curve, direction),
                SwapRetryDecision::RefreshCreator
            );
            assert_eq!(
                classify_swap_revert(Some(ANCHOR_CONSTRAINT_SEEDS), SwapRoute::Amm, direction),
                SwapRetryDecision::RefreshCoinCreator
            );
        }
    }

    #[test]
    fn sell_only_codes_do_not_fire_on_buy() {
        assert_eq!(
            classify_swap_revert(
                Some(CURVE_MISSING_USER_VOLUME_ACCUMULATOR),
                SwapRoute::Curve,
                SwapDirection::Buy
            ),
            SwapRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_swap_revert(Some(BONDING_CURVE_COMPLETE), SwapRoute::Curve, SwapDirection::Buy),
            SwapRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_swap_revert(
                Some(CURVE_TOO_LITTLE_SOL_RECEIVED),
                SwapRoute::Curve,
                SwapDirection::Buy
            ),
            SwapRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn slippage_codes_retry_on_their_own_route_only() {
        assert_eq!(
            classify_swap_revert(
                Some(CURVE_TOO_LITTLE_SOL_RECEIVED),
                SwapRoute::Curve,
                SwapDirection::Sell
            ),
            SwapRetryDecision::Retry
        );
        assert_eq!(
            classify_swap_revert(Some(AMM_EXCEEDED_SLIPPAGE), SwapRoute::Amm, SwapDirection::Sell),
            SwapRetryDecision::Retry
        );
        // Curve's sell slippage code on the AMM route (shouldn't happen, but
        // the guard must hold) falls through to StopFeeBurn.
        assert_eq!(
            classify_swap_revert(
                Some(CURVE_TOO_LITTLE_SOL_RECEIVED),
                SwapRoute::Amm,
                SwapDirection::Sell
            ),
            SwapRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn buy_slippage_codes_retry_on_their_own_route() {
        // Curve buy floors.
        for code in [CURVE_TOO_MUCH_SOL_REQUIRED, CURVE_BUY_SLIPPAGE_BELOW_MIN_TOKENS_OUT] {
            assert_eq!(
                classify_swap_revert(Some(code), SwapRoute::Curve, SwapDirection::Buy),
                SwapRetryDecision::Retry
            );
        }
        // AMM buy floors (6004 shared with sell, 6040 buy-only).
        for code in [AMM_EXCEEDED_SLIPPAGE, AMM_BUY_SLIPPAGE_BELOW_MIN_BASE] {
            assert_eq!(
                classify_swap_revert(Some(code), SwapRoute::Amm, SwapDirection::Buy),
                SwapRetryDecision::Retry
            );
        }
        // A buy floor on the wrong route falls through to StopFeeBurn.
        assert_eq!(
            classify_swap_revert(
                Some(CURVE_TOO_MUCH_SOL_REQUIRED),
                SwapRoute::Amm,
                SwapDirection::Buy
            ),
            SwapRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_swap_revert(
                Some(AMM_BUY_SLIPPAGE_BELOW_MIN_BASE),
                SwapRoute::Curve,
                SwapDirection::Buy
            ),
            SwapRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn structural_buy_revert_stops_fee_burn() {
        // An unknown/structural code on the buy path must give up, not resend.
        assert_eq!(
            classify_swap_revert(Some(6022), SwapRoute::Curve, SwapDirection::Buy),
            SwapRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_swap_revert(None, SwapRoute::Curve, SwapDirection::Buy),
            SwapRetryDecision::StopFeeBurn
        );
    }

    #[test]
    fn unknown_code_stops_fee_burn() {
        assert_eq!(
            classify_swap_revert(Some(6022), SwapRoute::Curve, SwapDirection::Sell),
            SwapRetryDecision::StopFeeBurn
        );
        assert_eq!(
            classify_swap_revert(None, SwapRoute::Curve, SwapDirection::Sell),
            SwapRetryDecision::StopFeeBurn
        );
    }
}
