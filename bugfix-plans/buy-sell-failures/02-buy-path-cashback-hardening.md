# B2 — Buy-path cashback hardening: stop caching a wrong `cashback_enabled` (Fix 4) — P0

> Workstream B (buy-sell-failures). Defense-in-depth for [Fix 01](01-manual-sell-6024-cashback.md).
> This file replaces the old standalone `fix-6024-cashback-sell-bug.md` (its Fix 2) — removed.
> Together, 01 + 02 are the full "6024 plan". Mirror the snipe-side thread-through in TPSL1 + TPSL2.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Problem

The buy path caches `TokenPDAs` with `cashback_enabled: false` hardcoded into `derive_token_pdas`:

**File:** [buy.rs:163](../../pump-trader/src/trader/buy.rs#L163) —
`derive_token_pdas(mint, creator, &tp, false)`.

This is **harmless today** only because the OR guard in the curve sell
(`if is_cashback || pdas.cashback_enabled`) relies on every sell caller passing the flag explicitly.
It is a **latent landmine**: a future path that reads `pdas.cashback_enabled` alone would reintroduce
6024. (`query.rs:522/544` are the only `derive_token_pdas` callers that already pass the real
`routing.cashback_enabled`; the buy path is the one that lies.)

## Fix — thread the true flag into the buy path

Both buyers already know the true value at buy time. Thread it so the cached PDAs are
self-consistent:

1. Add a `cashback_enabled: bool` param to `buy_token_inner` (and the public `buy_token` /
   `buy_token_snipe[_write_ahead]`), and pass it to `derive_token_pdas` instead of `false`.
2. **Manual buy** already has it: pass `routing.cashback_enabled` from
   [solana.rs](../../backend/src/api/handlers/trading/solana.rs).
3. **Snipe buy** has it in `token_cache` (`token.is_cashback_enabled`) at the call site in
   `tpsl_sniper_*/execution/real.rs` — thread it through. **Mirror in TPSL1 + TPSL2.**

The curve buy ix itself does **not** change (it always includes the accumulators); this only fixes
the cached `pdas` so a later sell relying on `pdas.cashback_enabled` is correct even if the caller
flag is wrong.

## Verification

- `cargo check -p pump-trader` + `cargo check -p backend-deploy` clean; clippy on touched files.
- `cargo test -p pump-trader` — curve/AMM tx-size tests must still pass (account list unchanged).
- Probe a cashback curve token: `cargo run -p backend-deploy -- probe simulate-sell <mint>` → 17
  accounts incl. UVA at slot 14, even with `is_cashback` deliberately not passed (proves the cached
  `pdas.cashback_enabled` now carries the truth).

## Related

- **`sell-stale-creator-vault-bug` (FIXED):** if doing this hardening, consider folding `creator`
  into the same "captured once at buy, stored where the sell reads it" model so the two
  staleness-prone fields (`creator`, `cashback_enabled`) share one strategy.
- **`tpsl-clones-intentional`:** mirror the snipe call-site change in `tpsl_sniper_2`.
