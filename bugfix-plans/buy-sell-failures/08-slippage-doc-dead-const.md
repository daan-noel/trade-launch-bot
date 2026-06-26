# B8 — Slippage doc correction + dead constant cleanup (Fix 8) — P2 (operational/docs)

> Workstream B (buy-sell-failures). Docs + dead-code cleanup; no behavior change.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Problem (D5)

- `slippage None` → `min_out = 1` on **both** curve and AMM builders (value-loss to MEV, not a
  revert). The AMM builders also treat `None` as 1, so `AMM_DEFAULT_SLIPPAGE_BPS` is **dead code** —
  declared at [constants.rs:238](../../pump-trader/src/constants.rs#L238), used only in a doc comment.
- The "AMM None = 5%" claim in
  [@plans/trade-execution/slippage-logic-buy-sell.md](../../plans/trade-execution/slippage-logic-buy-sell.md)
  is therefore **wrong**.

## Fix

1. **Remove the dead `AMM_DEFAULT_SLIPPAGE_BPS`** ([constants.rs:238](../../pump-trader/src/constants.rs#L238))
   **or** wire it into the AMM builders' `None` arm if a default AMM-buy floor is actually wanted
   (manual buys already pass `Some(500)`, so wiring is largely moot).
2. **Correct the doc** `@plans/trade-execution/slippage-logic-buy-sell.md`: AMM `None` = `min_out 1`
   (no floor), same as the curve — it does **not** apply a 5% default. Note that bot/manual **sells**
   intentionally pass `None` (clear at any price) via `resolve_sell_slippage_bps`
   ([tuning.rs:53-58](../../backend/src/config/constants/tuning.rs#L53-L58)).

## Optional refinements (low value, note only)

- Manual sell returns `200` if any pass sold, even with a nonzero remainder; consider returning the
  leftover balance so the UI knows it didn't fully clear.
- Manual sell collapses landed-revert vs never-landed into a string; the distinction exists in
  `sell_token` (`OnChainRevert`) but isn't surfaced to the API.

## Verification

- `cargo check -p pump-trader` clean after removing/wiring the constant; clippy shows no dead-code
  warning.
- Doc reads correctly against the actual `None`-arm behavior.
