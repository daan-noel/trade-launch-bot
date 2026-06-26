# Fix Plan: Custom(6024) — Missing UVA in Cashback Token Sell

## Summary

Manual sells fail with `InstructionError(3, Custom(6024))` on cashback-enabled
bonding-curve tokens. The pump.fun program rejects the transaction because the
`user_volume_accumulator` (UVA) account is missing from the sell instruction.

The root cause is a **cashback-status sourcing bug** in the manual-sell handler:
it reads the flag from `token_cache` (which can be empty/stale) instead of the
live `resolve_buy_routing` value it already fetched a few lines earlier.

---

## Investigated Transaction

- **Mint:** `7feWADwudSfNyfw9yek834EL5KpKKkPdi9NN74P2pump`
- **Failed sell tx:** `31W8JBbLGfQwgbcoCFrHFowBwaRWJvnSWdqjuX64AcYtF27REeQA1YkPMzHozzGQpEcWUZ2PcDdhpCHnPimQ1EzP`
- **Timestamp:** 2026-06-26T15:21:18 UTC
- **Wallet:** `xxXgBgHE2S16gfe2CmcQ1cs2UwsFUqzMJaioovdZXxx`
- **Bonding curve account:** `Faqkh4YTW2oAcmbwTB8tsMRuVPsWegf6RV5jVba9C71S`

---

## On-Chain Evidence

### Bonding curve account (151 bytes, owner = pump.fun program)

| Offset | Value | Meaning |
|--------|-------|---------|
| 48 | `0x00` | `complete = false` → still on bonding curve (routing is correct) |
| 82 | `0x01` | **`cashback_enabled = true`** ← key fact |

### Instruction layout comparison

A successful **buy** for the same token ran 2 minutes earlier (`5ujxk...`).
It has **18 accounts** in the pump.fun instruction. The failed **sell** has
**16 accounts**.

#### Correct sell layout (17 accounts, cashback token)

| Index | Account | Notes |
|-------|---------|-------|
| 0 | `global_pda` | |
| 1 | `fee_recipient` | |
| 2 | `mint` | |
| 3 | `bonding_curve` | |
| 4 | `associated_bonding_curve` | |
| 5 | `user_token_account` | |
| 6 | `user wallet` (signer) | |
| 7 | `system_program` | |
| 8 | `creator_vault` | |
| 9 | `token_program` | Token-2022 for this token |
| 10 | `event_authority` | |
| 11 | `pump_program` | |
| 12 | `fee_config` | |
| 13 | `fee_program` (`pfeeUxB6...`) | |
| 14 | **`user_volume_accumulator`** | **cashback — MISSING in failed tx** |
| 15 | `bonding_curve_v2` | PDA: seeds `["bonding-curve-v2", mint]` |
| 16 | `curve_fee_recipient` (`A7hAgCz...`) | |

#### What the failed tx actually sent (16 accounts)

| Index | Account | Problem |
|-------|---------|---------|
| 0–13 | (correct) | |
| 14 | `bonding_curve_v2` (`AevG7...`) | **Wrong slot — UVA expected here** |
| 15 | `curve_fee_recipient` (`A7hAgCz...`) | Shifted one position early |
| — | UVA | **Absent** |

The pump.fun program read position 14 expecting the `user_volume_accumulator`
and got `bonding_curve_v2` instead → **Custom(6024)**.

---

## Root Cause

### The guard in `pump-trader/src/trader/sell.rs`

```rust
if is_cashback || pdas.cashback_enabled {
    accounts.push(AccountMeta::new(global.user_volume_accumulator, false));
}
accounts.push(AccountMeta::new_readonly(pdas.bonding_curve_v2, false));
accounts.push(AccountMeta::new(self.curve_fee_recipient, false));
```

The guard is an **OR**: the UVA is added if *either* the caller-passed
`is_cashback` flag *or* the cached `pdas.cashback_enabled` is true. For the
failed sell, both were `false`:

1. **`is_cashback = false`** — the manual-sell handler read it from `token_cache`,
   which had no entry for this mint (or a stale `false`), so it defaulted to
   `false`.
2. **`pdas.cashback_enabled = false`** — the buy path caches `TokenPDAs` with a
   hardcoded `false` (`buy.rs:163`); the buy instruction includes the UVA
   unconditionally, so it never needs the real flag. That stale `false` carries
   into the sell.

Because the guard is an OR, **passing the correct `is_cashback` flag from the
caller is sufficient** — the stale `pdas.cashback_enabled` becomes irrelevant.

---

## Design: cashback status has two independent, non-overlapping worlds

`manual_buy` creates **no position**; positions are only opened inside the TPSL
snipe paths. A bot strategy therefore only ever sells tokens it sniped itself,
and a snipe is always triggered by a create event that already populated
`token_cache`. The manual and automated paths never touch the same token's
cashback decision.

| Path | Cashback source | Reliable? | Why |
|------|-----------------|-----------|-----|
| **Bot buy/sell** (TPSL snipe + exit) | `token_cache` (in-memory, DB-seeded on restart) | **Always** | Create event always precedes the snipe → cache is populated. No RPC on the hot path. |
| **Manual buy/sell** (API handler) | `routing.cashback_enabled` from a **live** `resolve_buy_routing` | **Always** | Read live from the bonding curve at the moment of each manual action. Off the hot path, so one `getMultipleAccounts` per human action is free. |

Key points:

- The bot path is **already correct** and needs no change.
- Manual buy and manual sell each **independently fetch routing live**. There is
  **no buy→sell handoff** — a manual sell often happens with no manual buy in the
  session (after a restart, or selling dust), so it must resolve its own routing.
  `manual_sell` already does this (`solana.rs:170`); it just discards
  `routing.cashback_enabled` and wrongly falls back to `token_cache`.

---

## Fix

### Fix 1 — `manual_sell` handler (the whole fix, 1 line)

**File:** `backend/src/api/handlers/trading/solana.rs`

Use the already-fetched live routing value instead of the `token_cache` lookup:

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

`routing` is already in scope from the `resolve_buy_routing` call a few lines
earlier — **no extra RPC**. `routing.cashback_enabled` is read live from the
bonding curve, so it is always correct regardless of cache state.

### Fix 2 (optional hardening) — stop caching a wrong `cashback_enabled`

**File:** `pump-trader/src/trader/buy.rs` (~line 163)

The buy path caches `TokenPDAs` with `cashback_enabled: false` hardcoded into
`derive_token_pdas`. This is **harmless today** only because the OR guard relies
on every sell caller passing the flag explicitly. It is a latent landmine: a
future path that reads `pdas.cashback_enabled` alone would reintroduce 6024.

Both buyers already know the true value at buy time (manual: `routing`;
snipe: `token_cache`). Thread it into `derive_token_pdas` instead of `false` so
`token_pdas` is self-consistent. Not required for the fix — pure defense.

---

## Dropped from the earlier plan

- **TPSL strategy audit** — the bot path reads `token_cache`, which is always
  populated for sniped tokens. Not affected.
- **Defensive re-read in `sell_token_once_inner`** — was patching the
  ingest-gap case, which is **manual-only** (a missed create event means no
  snipe ever fired). Fix 1 covers the manual path directly, so the extra RPC is
  unnecessary.

---

## Verification

After deploying Fix 1, retry the manual sell for
`7feWADwudSfNyfw9yek834EL5KpKKkPdi9NN74P2pump`.

The sell instruction in the new transaction should have **17 accounts** with:

- Position 14: `user_volume_accumulator` (cashback UVA)
- Position 15: `bonding_curve_v2`
- Position 16: `curve_fee_recipient` (`A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW`)

---

## Related Bugs / Context

- **`sell-stale-creator-vault-bug` (FIXED):** Anchor error 2006 on creator_vault
  — same class of problem (a creation-time fact going stale in a cache). If the
  `token_pdas` hardening (Fix 2) is done, fold `creator` into the same
  "captured once at buy, stored where the sell reads it" model so the two
  staleness-prone fields share one strategy.
- **`tpsl-clones-intentional`:** the bot path needs no change here, but any
  future edit to `tpsl_sniper_1` sell logic must be mirrored to `tpsl_sniper_2`.
