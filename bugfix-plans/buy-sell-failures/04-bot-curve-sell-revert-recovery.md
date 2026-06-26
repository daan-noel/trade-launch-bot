# B4 — Bot curve-sell structural-revert recovery: 6024 + 6005 (Fix 3) — P0

> Workstream B (buy-sell-failures). The **bot-path** counterpart to the manual fixes
> [01](01-manual-sell-6024-cashback.md)/[03](03-manual-sell-reresolve-routing.md). Also covers the
> cashback-toggle case (B3). **Mirror every edit in TPSL1 + TPSL2** — intentional clones.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Problem

On the bot curve sell, `is_cashback` and `is_migrated` come from `token_cache` (set at create, not
refreshed). Two recoverable reverts currently die as `ExitFailed`:

- **6024** (missing UVA) — cashback stale or toggled-on after the snipe (B3).
- **6005** (`BondingCurveComplete`) — a migration whose event the bot missed
  (`token_cache.is_migrated` still `false`; memory `missed-tokens-restart-replay-gap`).

Both fall through `classify_sell_revert` to `StopFeeBurn` → `ExitFailed`, **no recovery**.

## Goal

Generalize the existing `2006 → RefreshCreator` machinery so 6024 and 6005 re-read chain state and
**retry** instead of dying. One off-path RPC covers all three codes.

- **Files:** `tpsl_sniper_1/execution/real.rs` (classifier `798-830`, decision application
  `1058-1095`, error-code consts `742-756`) **and the tpsl2 clone** (classifier `943-975`,
  RefreshCreator arm ~`1216-1231`).

## Recommended unified design

1. **Add a trader method returning fresh curve facts**, reusing `ensure_token_pdas` (re-reads creator
   + cashback; a non-migrated curve is read fresh, not cache-served):
   ```rust
   // pump-trader/src/trader/query.rs — generalizes refresh_curve_creator_vault
   pub async fn refresh_curve_facts(&self, mint: &str)
       -> anyhow::Result<CurveFacts /* { creator_vault, cashback_enabled, is_migrated } */>
   ```
   (`refresh_curve_creator_vault` already does the `ensure_token_pdas` half — extend it / add a
   sibling that also returns `cashback_enabled` and `is_migrated`.)
2. **Add error-code consts:** `CURVE_MISSING_USER_VOLUME_ACCUMULATOR = 6024` and
   `BONDING_CURVE_COMPLETE = 6005` (confirm exact Anchor names against the IDLs).
3. **Extend `SellRetryDecision`** with `RefreshCashback` and `RerouteMigrated` (or a single
   `RefreshCurveFacts` the application arm interprets).
4. **In `classify_sell_revert`** curve branch (`used_migrated == now_migrated`, `!used_migrated`):
   map `6024 → RefreshCashback`, `6005 → RerouteMigrated`, keep `2006 → RefreshCreator`, slippage →
   `Retry`, else `StopFeeBurn`.
5. **In the decision-application match** (mirror the `RefreshCreator` arm at
   [real.rs:1071-1090](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L1071-L1090)):
   - **RefreshCashback:** call `refresh_curve_facts`; re-reads cashback into the cached `pdas`, so
     the OR guard `is_cashback || pdas.cashback_enabled` includes the UVA next attempt. **For full
     correctness (handles toggle-OFF too), also update the value passed to `sell_token_once`** —
     update `token_cache`'s `is_cashback_enabled` (the field the create decoder sets) and let the
     loop re-read it. Recommended: update `token_cache` so subsequent exits are correct and the
     toggle is observed.
   - **RerouteMigrated:** call `refresh_curve_facts`; if now migrated, update
     `token_cache.is_migrated` (the field `now_migrated` reads at
     [real.rs:1055-1056](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L1055-L1056)) so
     the next attempt's venue selection routes to the AMM; on failure → `Failed`.
6. **Unit-test `classify_sell_revert`** for the new codes (extend the existing `#[cfg(test)]` block).

## Minimal alternative

Treat `6024` exactly like `2006` (reuse `RefreshCreator` / `refresh_curve_creator_vault`, which also
refreshes `pdas.cashback_enabled`) — covers the common missing-UVA / toggle-ON case via the OR guard,
but **not** toggle-OFF, and does nothing for 6005. The unified design is preferred for "all cases".

## Verification

- Unit-test the classifier maps `6024 → RefreshCashback` and `6005 → RerouteMigrated`.
- Integration: stale-seed `token_cache.is_migrated = false` on an already-migrated mint, confirm the
  exit re-routes to AMM instead of `ExitFailed`; similarly for a toggled-cashback token.
- `cargo check -p backend-deploy` + `cargo check -p pump-trader` clean; `cargo test -p pump-trader`
  tx-size tests still pass.
- **Mirror all tpsl1 edits into tpsl2 and re-grep to confirm parity.**

## Docs (DoD)

Add an `@plans/trade-execution/` note for the generalized curve-fact refresh.
