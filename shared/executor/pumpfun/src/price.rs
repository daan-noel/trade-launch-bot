//! Curve pricing + slippage — the ONE source of the pump.fun bonding-curve
//! `min_out` math, shared by the live buy/sell paths, the simulate path, and the
//! launch dev-buy leg. Keeping a single copy is load-bearing: a drift between the
//! buy floor the live sender uses and the one the sim/dev-buy compute would let a
//! dry-run "pass" a fill the live tx then reverts (or under-protects it).
//!
//! Both functions are pure (no RPC) so every caller derives the floor identically.
//! `None` slippage — or missing/zero reserves — yields `1` (no protection) rather
//! than blocking on a reserve read; `None` stays distinct from `Some(0)`.

use crate::protocol;

/// Curve-buy slippage floor: the minimum tokens a `buy` (exact-quote-in) must
/// return or revert. `reserves` is the curve's virtual `(token, quote=lamports)`
/// reserves; the math mirrors the on-chain constant-product fill, net of the
/// curve fee buffer. `Some(slip)` + `Some((vt, vq))` → the protected floor;
/// anything else → `1`.
pub(crate) fn curve_buy_min_out(
    buy_lamports: u64,
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
    curve_fee_buffer_bps: u128,
) -> u64 {
    match (slippage_bps, reserves) {
        (Some(slip), Some((vt, vq))) => {
            let net = (buy_lamports as u128)
                .saturating_mul(10_000u128.saturating_sub(curve_fee_buffer_bps))
                / 10_000;
            // Saturating throughout on untrusted reserve reads; a zero denominator
            // (both reserves read as 0) falls back to the unprotected min_out=1
            // rather than panicking.
            let denom = vq.saturating_add(net);
            if denom == 0 {
                1
            } else {
                let expected = vt.saturating_mul(net) / denom;
                ((expected.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000) as u64)
                    .max(1)
            }
        }
        _ => 1,
    }
}

/// Curve-sell slippage floor: the minimum lamports a `sell` (exact-base-in) must
/// return or revert. `reserves` is the curve's virtual `(token, quote=lamports)`
/// reserves. Same protection contract as [`curve_buy_min_out`].
pub(crate) fn curve_sell_min_out(
    token_amount: u64,
    slippage_bps: Option<u64>,
    reserves: Option<(u128, u128)>,
    curve_fee_buffer_bps: u128,
) -> u64 {
    match (slippage_bps, reserves) {
        (Some(slip), Some((vt, vq))) => {
            // Saturating throughout on untrusted reserve reads; a zero denominator
            // falls back to the unprotected min_out=1.
            let denom = vt.saturating_add(token_amount as u128);
            if denom == 0 {
                1
            } else {
                let gross = vq.saturating_mul(token_amount as u128) / denom;
                let net =
                    gross.saturating_mul(10_000u128.saturating_sub(curve_fee_buffer_bps)) / 10_000;
                ((net.saturating_mul(10_000u128.saturating_sub(slip as u128)) / 10_000) as u64)
                    .max(1)
            }
        }
        _ => 1,
    }
}

/// The `(virtual_token, virtual_quote=lamports)` reserves of a bonding curve at
/// the instant it is created. Used ONLY by the atomic single-tx `create` +
/// dev-buy leg, where the curve is created in the SAME transaction and therefore
/// has no on-chain state to read yet — these protocol-constant initial reserves
/// ARE the curve's live reserves for that first buy, so the dev-buy floor is
/// computed through the same [`curve_buy_min_out`] as every other buy (no
/// hand-inlined reserve tuple that could drift from the protocol constants). Any
/// buy against an *already-created* curve reads live reserves instead.
pub(crate) fn fresh_curve_reserves() -> (u128, u128) {
    (
        protocol::INITIAL_VIRTUAL_TOKEN_RESERVES,
        protocol::INITIAL_VIRTUAL_SOL_RESERVES,
    )
}

/// Advance a bonding curve's `(virtual_token, virtual_quote=lamports)` reserves by
/// ONE constant-product buy of `buy_lamports` (gross), returning the post-buy
/// reserves. Used ONLY to **simulate** the curve state each co-buy leg of an atomic
/// launch Jito bundle will face: the curve is created inside the same bundle, so no
/// leg after the dev-buy can read live reserves — they're derived by replaying the
/// preceding legs (create+dev-buy, then each prior co-buy) through this step. Mirrors
/// [`curve_buy_min_out`]'s net-of-fee-buffer input so the simulated evolution and the
/// per-leg floor never drift; the small residual (true protocol fee vs the buffer,
/// sub-0.1 % at launch buy sizes) is absorbed by the leg's own `slippage_bps`.
pub(crate) fn apply_curve_buy(
    reserves: (u128, u128),
    buy_lamports: u64,
    curve_fee_buffer_bps: u128,
) -> (u128, u128) {
    let (vt, vq) = reserves;
    let net = (buy_lamports as u128)
        .saturating_mul(10_000u128.saturating_sub(curve_fee_buffer_bps))
        / 10_000;
    let denom = vq.saturating_add(net);
    if denom == 0 {
        return reserves;
    }
    let tokens_out = vt.saturating_mul(net) / denom;
    (vt.saturating_sub(tokens_out), vq.saturating_add(net))
}

#[cfg(test)]
mod tests {
    use super::{curve_buy_min_out, curve_sell_min_out, fresh_curve_reserves};

    /// Default `config.slippage.curve_fee_buffer_bps`.
    const FEE_BUF: u128 = 200;

    #[test]
    fn buy_min_out_is_unprotected_without_slippage_or_reserves() {
        assert_eq!(curve_buy_min_out(1_000_000, None, Some((1_000, 2_000)), FEE_BUF), 1);
        assert_eq!(curve_buy_min_out(1_000_000, Some(500), None, FEE_BUF), 1);
        assert_eq!(curve_buy_min_out(1_000_000, Some(500), Some((0, 0)), FEE_BUF), 1);
    }

    #[test]
    fn buy_tighter_slippage_raises_the_floor() {
        let reserves = Some((1_000_000_000u128, 30_000_000u128));
        let loose = curve_buy_min_out(1_000_000, Some(5_000), reserves, FEE_BUF); // 50%
        let tight = curve_buy_min_out(1_000_000, Some(100), reserves, FEE_BUF); // 1%
        assert!(tight >= loose, "tighter slippage must demand at least as many tokens");
        assert!(loose >= 1 && tight >= 1, "floor is always >= 1");
    }

    #[test]
    fn sell_min_out_is_unprotected_without_slippage_or_reserves() {
        assert_eq!(curve_sell_min_out(1_000_000, None, Some((1_000, 2_000)), FEE_BUF), 1);
        assert_eq!(curve_sell_min_out(1_000_000, Some(500), None, FEE_BUF), 1);
        assert_eq!(curve_sell_min_out(1_000_000, Some(500), Some((0, 0)), FEE_BUF), 1);
    }

    /// The dev-buy leg derives its floor from the SAME `curve_buy_min_out` as
    /// every other buy, seeded with the fresh curve's protocol-constant reserves
    /// (the curve is created in the same tx, so these ARE its live reserves).
    /// A tighter slippage still raises the floor — proving it's the real
    /// protected calc, not a hand-inlined constant.
    #[test]
    fn dev_buy_floor_uses_the_shared_calc_over_fresh_reserves() {
        let reserves = Some(fresh_curve_reserves());
        let unprotected = curve_buy_min_out(1_000_000_000, None, reserves, FEE_BUF);
        let loose = curve_buy_min_out(1_000_000_000, Some(5_000), reserves, FEE_BUF);
        let tight = curve_buy_min_out(1_000_000_000, Some(100), reserves, FEE_BUF);
        assert_eq!(unprotected, 1, "no slippage → no floor even on a fresh curve");
        assert!(tight >= loose && loose >= 1, "protected floor rises with tighter slippage");
    }

    /// A single `apply_curve_buy` moves the curve the right direction: quote reserve
    /// UP by the net-of-fee-buffer lamports, token reserve DOWN by the tokens the
    /// buyer received (constant-product), and it hands out a positive token amount.
    #[test]
    fn apply_curve_buy_moves_reserves_the_right_way() {
        use super::apply_curve_buy;
        let (vt0, vq0) = fresh_curve_reserves();
        let (vt1, vq1) = apply_curve_buy((vt0, vq0), 20_000_000, FEE_BUF);
        assert!(vq1 > vq0, "quote reserve must rise on a buy");
        assert!(vt1 < vt0, "token reserve must fall on a buy");
        // Net added to the quote reserve is buy_lamports minus the fee buffer.
        let net = 20_000_000u128 * (10_000 - FEE_BUF) / 10_000;
        assert_eq!(vq1 - vq0, net, "quote reserve grows by exactly the net-of-fee input");
        assert_eq!(vt0 - vt1, vt0 * net / (vq0 + net), "tokens out follow constant product");
    }

    /// Replaying create → dev-buy → co-buy1 → co-buy2 produces a MONOTONE curve: each
    /// successive leg faces a higher quote reserve and a lower token reserve, so its
    /// `min_out` floor for the same spend is no greater than the prior leg's. This is
    /// exactly why the atomic launch's co-buy legs don't revert — each is priced
    /// against the state its predecessors leave behind, not the empty curve.
    #[test]
    fn sequential_buys_are_monotone_and_lower_each_floor() {
        use super::apply_curve_buy;
        let mut r = fresh_curve_reserves();
        let leg_spend = 10_000_000u64;
        let mut prev_vq = 0u128;
        let mut prev_floor = u64::MAX;
        for _ in 0..4 {
            assert!(r.1 > prev_vq, "each leg faces a strictly higher quote reserve");
            let floor = curve_buy_min_out(leg_spend, Some(500), Some(r), FEE_BUF);
            assert!(
                floor <= prev_floor,
                "a later leg's min_out floor must not exceed an earlier one's ({floor} > {prev_floor})"
            );
            prev_vq = r.1;
            prev_floor = floor;
            r = apply_curve_buy(r, leg_spend, FEE_BUF);
        }
    }
}
