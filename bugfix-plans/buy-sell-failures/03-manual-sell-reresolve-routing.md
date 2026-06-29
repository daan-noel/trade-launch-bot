# B3 — Manual-sell: re-resolve routing inside the clear loop (Fix 2) — P0

> ✅ **DONE** (2026-06-29, branch `split`). `resolve_buy_routing` (retried, B6) now runs at the top of
> each clear-loop pass in [live/src/api/handlers/trading/solana.rs](../../live/src/api/handlers/trading/solana.rs);
> `is_cashback`/`is_migrated` are re-read per pass, so a mid-loop migration re-routes to AMM instead of
> looping `Custom(6005)`. Landed together with B1 + B6. `cargo check -p live` clean.
>
> Workstream B (buy-sell-failures). Builds on [Fix 01](01-manual-sell-6024-cashback.md) — it
> relocates the same `routing` resolution. Land 01 first, then this.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Problem (A4 — `BondingCurveComplete(6005)`, manual path)

In `manual_sell`, `routing` (venue + creator + cashback) is resolved **once** at
[solana.rs:170](../../backend/src/api/handlers/trading/solana.rs#L170), before the clear loop. The
loop ([solana.rs:199-253](../../backend/src/api/handlers/trading/solana.rs#L199-L253)) re-reads
**balance** but not **routing**. A token that migrates **mid-loop** keeps routing every pass to the
curve → repeated `Custom(6005)` → 500.

## Fix

Move `resolve_buy_routing` (and the `is_cashback = routing.cashback_enabled` from
[Fix 01](01-manual-sell-6024-cashback.md)) to the **top of each pass**, before the balance read.

- At most 3 extra `getMultipleAccounts` (3 passes), all off the hot path.
- On a resolve error mid-loop, break with `last_err` as today.

**Bonus:** re-routes correctly if migration happens between passes, and keeps cashback fresh per pass
(defense-in-depth with Fix 01).

## Verification

- Force a migrated token through `manual_sell` with a stale cache and confirm it routes AMM (no 6005
  loop).
- `cargo check -p backend-deploy` clean; clippy on touched file.

## Related

- Bot-path equivalent of the 6005 recovery is [Fix 04](04-bot-curve-sell-revert-recovery.md).
