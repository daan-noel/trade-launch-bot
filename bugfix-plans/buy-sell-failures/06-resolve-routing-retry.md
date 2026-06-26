# B6 — `resolve_buy_routing` small retry (Fix 6) — P1

> Workstream B (buy-sell-failures). Independent; no dependencies.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Problem (B1)

`resolve_buy_routing` does a `getMultipleAccounts`; a flaky read → 400, **no retry**. Every manual
trade (buy + sell) gates on it, so one transient RPC blip fails the whole manual trade.

## Fix

- **File:** the `resolve_buy_routing` calls in `manual_buy` / `manual_sell`
  ([solana.rs:99-106, 170-177](../../backend/src/api/handlers/trading/solana.rs#L99-L106)).
- **Change:** a 2-attempt retry with a short backoff before returning the 400, so a single flaky
  `getMultipleAccounts` doesn't fail a manual trade. Off the hot path; bounded.

## Verification

- `cargo check -p backend-deploy` clean; clippy on touched file.
- Optional: inject a one-shot RPC failure and confirm the retry succeeds rather than 400-ing.
