# B1 — Manual-sell `Custom(6024)`: missing UVA on cashback token (Fix 1) — P0

> Workstream B (buy-sell-failures). **The 1-line fix** for the reported 6024. This file replaces
> the old standalone `fix-6024-cashback-sell-bug.md` (its Fix 1) — that file was a subset of the
> audit and has been removed. Bot-path equivalent is [Fix 04](04-bot-curve-sell-revert-recovery.md).
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Summary

Manual sells fail with `InstructionError(3, Custom(6024))` on cashback-enabled bonding-curve
tokens. pump.fun rejects the tx because the `user_volume_accumulator` (UVA) account is missing from
the sell instruction. Root cause: a **cashback-status sourcing bug** in the manual-sell handler — it
reads the flag from `token_cache` (empty/stale) instead of the live `resolve_buy_routing` value it
already fetched a few lines earlier.

## Investigated transaction

- **Mint:** `7feWADwudSfNyfw9yek834EL5KpKKkPdi9NN74P2pump`
- **Failed sell tx:** `31W8JBbLGfQwgbcoCFrHFowBwaRWJvnSWdqjuX64AcYtF27REeQA1YkPMzHozzGQpEcWUZ2PcDdhpCHnPimQ1EzP`
- **Timestamp:** 2026-06-26T15:21:18 UTC · **Wallet:** `xxXgBgHE2S16gfe2CmcQ1cs2UwsFUqzMJaioovdZXxx`
- **Bonding curve account:** `Faqkh4YTW2oAcmbwTB8tsMRuVPsWegf6RV5jVba9C71S`

## On-chain evidence

Bonding curve account (151 bytes, owner = pump.fun program): offset 48 = `0x00`
(`complete=false`, still on curve → routing correct); offset 82 = `0x01`
(**`cashback_enabled = true`** ← key fact).

A successful **buy** 2 min earlier had **18 accounts**; the failed **sell** had **16** (correct is
17). The failed sell put `bonding_curve_v2` at index 14 where the program expected the UVA, shifting
`curve_fee_recipient` to 15 and dropping the UVA entirely → **Custom(6024)**.

| Index | Correct sell (cashback) | Failed sell |
|---|---|---|
| 14 | **`user_volume_accumulator`** | `bonding_curve_v2` ❌ wrong slot |
| 15 | `bonding_curve_v2` | `curve_fee_recipient` (shifted) |
| 16 | `curve_fee_recipient` | — (UVA absent) |

## Root cause — the OR guard in `pump-trader/src/trader/sell.rs`

```rust
if is_cashback || pdas.cashback_enabled {
    accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
}
accounts.push(AccountMeta::new_readonly(pdas.bonding_curve_v2, false));
accounts.push(AccountMeta::new(self.curve_fee_recipient, false));
```

For the failed sell **both operands were `false`**:

1. **`is_cashback = false`** — the manual-sell handler read it from `token_cache`, which had no
   entry for this mint (or a stale `false`).
2. **`pdas.cashback_enabled = false`** — the buy path caches `TokenPDAs` with a hardcoded `false`
   (`buy.rs:163`); the buy ix includes the UVA unconditionally so it never needs the real flag. That
   stale `false` carries into the sell.

Because the guard is an OR, **passing the correct `is_cashback` from the caller is sufficient** — the
stale `pdas.cashback_enabled` becomes irrelevant.

## Why the manual path must resolve its own routing

`manual_buy` creates **no position**; positions open only inside the TPSL snipe paths, and a snipe
is always triggered by a create event that already populated `token_cache`. So manual and bot paths
never share a token's cashback decision. A manual sell often happens with **no manual buy in the
session** (after restart, or selling dust), so it must resolve routing live. `manual_sell` already
does (`solana.rs:170`) — it just discards `routing.cashback_enabled` and wrongly falls back to
`token_cache`.

## Fix (the whole fix — 1 line)

**File:** [solana.rs:182-186](../../backend/src/api/handlers/trading/solana.rs#L182-L186)

```rust
// BEFORE (stale / empty cache → false):
let is_cashback = app_state
    .token_cache
    .get(&body.mint)
    .map(|e| e.token.is_cashback_enabled)
    .unwrap_or(false);

// AFTER (live, from resolve_buy_routing already called above):
let is_cashback = routing.cashback_enabled;
```

`routing` is already in scope from the `resolve_buy_routing` call at line 170 — **no extra RPC**.
`routing.cashback_enabled` is read live from the bonding curve (offset 82), so it's always correct
regardless of cache state, and being a re-read is robust to a cashback toggle (B3) on the manual path.

> **Note:** [Fix 03](03-manual-sell-reresolve-routing.md) moves this same `routing` resolution to the
> top of each clear-loop pass. If you do both, land this line first, then 03 relocates it.

## Verification

After deploying, retry the manual sell for `7feWADwudSfNyfw9yek834EL5KpKKkPdi9NN74P2pump`. The new
tx's sell instruction should have **17 accounts**: pos 14 `user_volume_accumulator`, pos 15
`bonding_curve_v2`, pos 16 `curve_fee_recipient`
(`A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW`).

Probe: `cargo run -p backend-deploy -- probe simulate-sell <mint>` on a cashback curve token must
pass (17-account sell incl. UVA at slot 14). `simulate_curve_sell` forces a fresh `ensure_token_pdas`,
so a passing sim proves the live build is correct.

## Related

- **[Fix 02](02-buy-path-cashback-hardening.md)** — stop caching the wrong `cashback_enabled`
  (defense in depth; together they are the full "6024 plan").
- **`sell-stale-creator-vault-bug` (FIXED):** Anchor 2006 on creator_vault — same class
  (creation-time fact going stale in a cache).
- **`tpsl-clones-intentional`:** bot path needs no change here, but any future edit to
  `tpsl_sniper_1` sell logic must mirror to `tpsl_sniper_2`.
