# Trade-path latency: RPC de-duplication & cache reuse

**Date:** 2026-06-09
**Scope:** `pump-trader` crate (buy/sell/AMM/query) + TPSL strategy services.
**Goal:** Reduce "ready-time" (time to build+sign a tx before send) and improve
stability by removing unnecessary/duplicated RPC round-trips and serving from
existing caches. **Primary focus: auto buy/sell for the TPSL bot.** Manual
buy/sell on the wallet page is also covered.

The guiding rule: *use cached data as much as possible; only hit RPC when it is
actually required for correctness.*

---

## Background: where the latency was

Ready-time on the trade path is almost entirely **sequential RPC round-trips**
(RTT). All CPU work (PDA derivation, signing, template build) is < 1 ms combined.
So reducing latency = reducing the number of *serial* RPCs before send.

The existing Helius WS ingest already feeds the in-memory `TokenCache`
(creator, token_program, `is_migrated`, `is_cashback`, price, reserves), and the
trader already keeps internal caches (`token_pdas`, `user_token_accounts`,
`amm_pool_cache`, `amm_global_config`). The waste was in **not fully using them**
and in **issuing independent reads sequentially**.

---

## Changes implemented

### 1. `buy_token_snipe` — skip the fresh-token ATA-existence RPC
`pump-trader/src/trader/buy.rs`

`buy_token` always did `get_account(ata)` to decide create-vs-reuse of the user
token account. For a **freshly-created token** (the TPSL snipe case) the wallet
provably holds no account yet, so that check is a guaranteed-"missing" RPC on the
most latency-critical path.

- Refactored the body into `buy_token_inner(..., skip_ata_check: bool)`.
- `buy_token(...)` → calls inner with `false` (**unchanged behavior**; all
  existing callers, incl. manual buy and examples, are untouched).
- `buy_token_snipe(...)` → calls inner with `true`, which sets
  `ata_exists = false` without the RPC and goes straight to the seed-account
  (template) path.

**Safety:** if the "fresh" assumption were ever wrong, the only consequence is one
extra create-with-seed token account (a few thousand lamports of rent) — never a
failed or misrouted trade.

### 2. `resolve_cached_token_account` — one lookup, then cache
`pump-trader/src/trader/query.rs`

New method: cache-peek (`user_token_accounts`) → at most one
`get_all_token_accounts` scan → cache the result. Returns `None` if the wallet
holds no account for the mint.

This replaces the TPSL sell loop's habit of re-scanning the **entire wallet**
(2 RPC: legacy + Token-2022 `getTokenAccountsByOwner`) on *every* attempt.

### 3. TPSL sell: resolve the token account once
`backend/src/strategies/tpsl/service_tpsl.rs`
`backend/src/strategies/tpsl_sniper_1/service_tpsl.rs`

`sell_with_retries` previously called `get_all_token_accounts()` inside the retry
loop. Because `on_trade_executed` wraps it in an outer retry (up to 10×) and the
inner loop runs up to 6×, the token-account lookup could fire **up to ~120 times**
for a single exit — for a value that never changes.

Now it resolves **once** via `resolve_cached_token_account` before the loop and
reuses the result. On a position bought in the same process the account is already
cached by the buy → **0 RPC**. Across a restart it costs **one** wallet scan, then
cached.

If resolution returns `None`, `sell_token`/`amm_sell` still fall back to their own
internal `override → cache → on-chain` chain, so **correctness is unchanged**.

### 4. TPSL buy uses `buy_token_snipe`
Both TPSL services' `buy_with_retries` now call `buy_token_snipe` (these fire from
`on_token_created`, i.e. a token just seen via the create event → provably fresh).

### 5. Parallelize independent reads (`tokio::try_join!`)
- `resolve_buy_routing` (query.rs): bonding-curve + mint accounts — were two
  sequential `get_account`s, now concurrent. Gates **every manual buy/sell**.
- `get_creator_from_mint_pda` (query.rs): same two independent reads, now
  concurrent. Hit by manual sell when PDAs aren't cached.
- `amm_reserves` (amm.rs): base + quote vault balances — were sequential, now
  concurrent. Hit by **every AMM swap** (manual + migrated TPSL sells).

No decode logic changed; only the reads were made concurrent (2 RTT → 1 RTT).
This is the conservative form of batching — equivalent latency to a
`getMultipleAccounts` call, but the parsing path is byte-for-byte identical to
before, so there is no risk of an offset/decode regression.

---

## Before / after (sequential RTT before send)

CPU is negligible; numbers are serial RPC round-trips. "warm" = token previously
traded in this process (caches populated); "cold" = fresh process / first touch.

| Path | Before | After | Notes |
|---|---|---|---|
| **TPSL buy** (snipe, curve) | 1 | **0** | ATA-existence RPC skipped |
| **TPSL sell** (curve, warm) | 2 × attempts (≤~120) | **0** | account cached from buy |
| **TPSL sell** (curve, cold) | 2 × attempts | **2 once** | one scan, then cached |
| **TPSL sell** (migrated/AMM) | 2/attempt + reserves(2 seq) + pool/cfg | **reserves(1) + pool/cfg cold-once** | account once; reserves concurrent |
| **Manual buy** (curve) | 4 | **3** | routing 2→1 |
| **Manual sell** (curve, cold) | 7 | **~5** (3 if acct cached) | routing 2→1, creator-pda 2→1 |
| **Any AMM swap** | reserves 2 seq | reserves **1** | concurrent vault reads |

The headline win is **TPSL sell**: from a per-attempt full-wallet rescan to a
single cached lookup. That removes the dominant latency *and* the dominant
stability risk on the exit path (each removed RPC is one fewer thing that can
time out or error mid-exit).

---

## Pros / cons

**Pros**
- Large latency cut on the TPSL exit path; smaller cuts everywhere else.
- **Stability:** far fewer RPCs per trade → fewer transient-error/timeout surfaces,
  and far less `getTokenAccountsByOwner` load (which grows with wallet size).
- No new subsystems, no new external dependencies, no schema changes.
- Backward compatible: `buy_token` and all existing callers are untouched; new
  behavior is opt-in via `buy_token_snipe`.
- Both crates `cargo check` clean (no new warnings/errors).

**Cons / trade-offs**
- `buy_token_snipe` trusts the caller that the token is fresh. Mitigated: only the
  TPSL create-event path uses it, and the worst case is a redundant token account,
  not a bad trade.
- `resolve_cached_token_account` trusts the `user_token_accounts` cache. It is
  populated by this trader's own buys with the exact account used, so it is
  authoritative; on a miss it falls back to a live scan.
- `try_join!` issues the two reads concurrently (still two requests to the RPC,
  just not serialized). A true `getMultipleAccounts` would be one request — left as
  a future option since the concurrent form is the zero-risk version.

---

## What was NOT done (future work)

These are the next latency levers, deliberately left out of this pass to keep it
safe and non-breaking:

1. **WS-fed reserve cache (curve + AMM).** Stream the bonding-curve PDA
   (`vt@8`, `vq@16`, `complete@48`, `creator@49`, `cashback@82`) and the AMM vault
   balances (`amount@64`) via Geyser/Laserstream account-update streams, and serve
   slippage reserves + migration status from cache with a staleness guard +
   RPC fallback. This would take manual buy/sell and AMM sells close to **0 RPC**
   too. (TPSL already uses `slippage=None`, so it skips reserve reads on the curve
   path — this mainly helps manual + AMM.)
   - Offset maps for the bonding-curve/AMM accounts and the trade-event payloads
     are already documented from the IDLs; note both programs use Anchor
     `emit_cpi!`, so events ride as inner-instruction data (transaction stream),
     while account-update streams are the simpler, emit-agnostic source.
2. **AMM pool/config pre-warm on migration.** Seed `amm_pool_cache` from the
   `CompletePumpAmmMigrationEvent` (carries the new `pool` address) so the first
   migrated sell doesn't pay the cold pool read + fee-share-marker scan.
3. **`getMultipleAccounts` batching** where multiple reads are needed at once
   (e.g. routing), upgrading the `try_join!` pairs to a single request.
4. **Background blockhash cache** for the AMM path's `get_latest_blockhash`
   (`build_recent_tx`), removing the last per-trade RPC on AMM buys.

---

## Verification

- `cargo check -p pump-trader` → clean.
- `cargo check -p backend` → clean (only pre-existing dead-code warnings).
- Behavior preserved: manual buy/sell call the unchanged `buy_token` /
  `resolve_buy_routing`; sell fallbacks intact; only the *number and ordering* of
  RPCs changed, not what gets built or signed.

### Files touched
- `pump-trader/src/trader/buy.rs` — `buy_token_snipe` / `buy_token_inner`, ATA skip.
- `pump-trader/src/trader/query.rs` — `resolve_cached_token_account`; parallel
  reads in `resolve_buy_routing` and `get_creator_from_mint_pda`.
- `pump-trader/src/trader/amm.rs` — parallel reads in `amm_reserves`.
- `backend/src/strategies/tpsl/service_tpsl.rs` — sell account resolved once; buy
  uses snipe path.
- `backend/src/strategies/tpsl_sniper_1/service_tpsl.rs` — same.
