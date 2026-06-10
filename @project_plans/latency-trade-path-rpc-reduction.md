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

---

# Phase 2 — WS-fed live reserve cache + AMM pool pre-warm

**Date:** 2026-06-10
**Goal:** Remove the remaining per-trade reserve RPC by serving reserves from the
existing Helius WS feed, and remove the cold AMM pool/marker fetch from the bot's
exit path. Targets the last RPCs left after Phase 1 — chiefly **TPSL migrated
(AMM) sells** and **manual curve/AMM slippage reads**.

## Key realization

The ingest already decodes a **post-trade reserve snapshot on every tracked
token's trade** — bonding-curve `virtual_*_reserves`, and (rolled PRE→POST) the
AMM `pool_base/quote_token_reserves` — and stores them on the `Trade`
([decoder/trade.rs](../backend/src/ingest/decoder/trade.rs)). So the reserves the
trade path was reading via RPC are *already arriving over the WS*; they just
weren't reaching the trader. Phase 2 bridges that gap. No new subscriptions, no
Geyser — it rides the existing pipeline.

## What was implemented

### 1. `ReserveCache` (new) — `pump-trader/src/trader/reserves.rs`
A small, freshness-bounded, **venue-tagged** cache: `mint → {token, quote_lamports,
is_amm, at}` behind a `std::sync::Mutex` (tiny sync critical sections, never held
across `.await`).
- `update(mint, token_f64, sol_f64, is_amm)` — converts to the trader's exact
  on-chain units (token raw; quote `sol × 1e9` lamports); ignores zero/non-finite.
- `get_fresh(mint, max_age, want_amm)` — returns only a **fresh** snapshot of the
  **matching venue**; miss/stale/wrong-venue → `None` (→ caller reads on-chain).

The **venue tag is the key safety property**: it closes the migration-window gap
where the last bonding-curve snapshot would otherwise be served to the first AMM
sell (curve and AMM reserves are different accounts on different scales). After
migration, AMM reads (`want_amm = true`) miss the curve entry and fall back to RPC
until the first AMM trade caches an AMM-tagged snapshot.

### 2. Trader integration — `mod.rs` / `query.rs` / `amm.rs`
- `PumpFunTrader` holds `reserve_cache: Arc<ReserveCache>` and exposes
  `update_live_reserves(mint, token, sol, is_amm)` (pub, called by the ingest).
- `curve_reserves(mint, bonding_curve)` — cache-first wrapper over
  `curve_virtual_reserves`; used by curve buy/sell slippage.
- `amm_reserves_cached(mint, pool)` — cache-first wrapper over `amm_reserves`;
  used by `build_amm_buy_ixs` / `build_amm_sell_ixs`.
- Freshness bound: `RESERVE_CACHE_MAX_AGE_MS = 3000` (constants.rs).

### 3. `prewarm_amm_pool(mint, base_token_program_id)` — `amm.rs`
Public, idempotent, best-effort: warms `amm_pool_cache` (pool facts + fee-share
marker) + `amm_global_config`. Moves the cold pool read + up-to-15-`getTransaction`
marker scan **off the exit hot path**.

### 4. Pipeline wiring — `ingest/pipeline.rs` (+ `main.rs`)
`IngestPipeline` now holds an `Arc<PumpFunTrader>`. In `on_trade_executed` (runs
only for tracked tokens):
- feeds the trade's post-trade reserves into `update_live_reserves` (venue from
  `trade.venue`);
- on a token's **first AMM trade**, spawns `prewarm_amm_pool` in the background
  (guarded by a `prewarmed_pools` set, once per mint; retried if it fails). Only
  when the token program is known, so a guessed pool layout is never cached.

## Why this is safe (freshness reasoning)

A cached reserve is only "stale" if **no trade has happened** since it was written
— in which case the reserves *haven't changed*, so the cached value is still
accurate. The 3 s bound therefore really guards against a **WS gap/disconnect**:
if updates stop arriving, the trade path silently reverts to on-chain reads. And
because reserves feed a slippage **floor** with built-in tolerance, any residual
lag is absorbed. Every read is cache-first **with RPC fallback** — a miss, a stale
entry, a wrong-venue entry, or a zero snapshot all transparently fall back to the
proven on-chain path. Nothing about what gets built/signed changed.

## Impact (per-trade reserve RPC)

| Path | Phase 1 | Phase 2 (active token) |
|---|---|---|
| **TPSL AMM sell** — reserves | 1 RTT (concurrent) | **0** (cache hit) |
| **TPSL AMM sell** — first cold pool+marker fetch | on exit path | **pre-warmed** off-path |
| **Manual curve buy/sell** — slippage reserves | 1 RTT | **0** (cache hit) |
| **Manual AMM buy/sell** — reserves | 1 RTT | **0** (cache hit) |
| Quiet token / post–WS-gap | 1 RTT | 1 RTT (RPC fallback) |

For an active migrated token, a TPSL exit now reaches the signer with **no reserve
RPC and a warm pool** — the reserve that *triggered* the exit is the same snapshot
used for the sell's slippage. (TPSL curve buy/sell already pass `slippage=None`, so
they never read reserves; Phase 2 mainly benefits AMM sells and all manual trades.)

## Pros / cons

**Pros**
- Removes the last recurring per-trade reserve RPC for actively-traded tokens, and
  the worst cold fetch (marker scan) from the exit path.
- Reuses the existing WS ingest — no new subscriptions, no Geyser, no new deps
  (std Mutex/HashMap only).
- Cache-first + RPC-fallback + venue-tag + freshness-bound ⇒ no new failure mode:
  any doubt falls back to the Phase-1 behavior.

**Cons / trade-offs**
- Couples `IngestPipeline` to `Arc<PumpFunTrader>` (cache feed + prewarm trigger).
- Reserve cache is unbounded in principle (one small entry per traded mint), but
  bounded in practice by the tracked-token set; entries are cheap (~48 B + key).
- `prewarm` fires on the first AMM trade seen, not at the migration instant (the
  marker scan needs a prior swap to read) — a sell in the tiny window before that
  still takes the cold path (then it's cached).

## Verification

- `cargo check -p pump-trader` / `-p backend` → clean (only pre-existing
  dead-code/import warnings).
- **Unit tests** (`pump-trader/src/trader/reserves.rs`, 6 tests, all pass) cover
  the riskiest new logic: SOL→lamports unit round-trip, **venue isolation** (a
  curve snapshot never serves an AMM read and vice-versa), the freshness bound,
  and the zero/negative/NaN/±inf guard. `cargo test -p pump-trader --lib` → 6/6.
- **Regression**: `cargo test -p backend --bins` → 19/19 pass (1 ignored —
  network-gated AMM integration test), confirming the pipeline + Phase-1 changes
  break nothing.

### Files touched (Phase 2)
- `pump-trader/src/trader/reserves.rs` — **new** `ReserveCache`.
- `pump-trader/src/trader/mod.rs` — cache field + `update_live_reserves`.
- `pump-trader/src/trader/query.rs` — `curve_reserves` wrapper.
- `pump-trader/src/trader/amm.rs` — `amm_reserves_cached`, `prewarm_amm_pool`,
  build paths use the cached reader.
- `pump-trader/src/trader/buy.rs`, `sell.rs` — curve slippage uses `curve_reserves`.
- `pump-trader/src/constants.rs` — `RESERVE_CACHE_MAX_AGE_MS`.
- `backend/src/ingest/pipeline.rs` — trader handle; reserve feed + prewarm in
  `on_trade_executed`.
- `backend/src/main.rs` — pass `trader` into `IngestPipeline::new`.

## Still open (future)

- ~~**Bonding-curve migration self-heal**~~ — done in Phase 3 below.
- ~~**Background blockhash cache** for the AMM buy path's `get_latest_blockhash`
  (the last per-trade RPC on AMM buys; manual AMM buys only — TPSL AMM sells use a
  durable nonce).~~ — done in Phase 5 below.
- **Account-stream (Geyser) reserves** for tokens we *don't* trade through the WS
  trade feed (not needed for the current TPSL/manual flows) — still open by design.

---

# Phase 3 — TPSL exit migration self-heal

**Date:** 2026-06-10
**Goal:** Stop a held position from getting stuck unsellable when it migrates
*during* its own exit.

## The bug

In both TPSL services' `on_trade_executed` (real mode), the exit ran a 10×
retry loop but read the routing flags **once, before the loop**:

```rust
let (is_cashback, is_migrated) = match cache.get(&mint) { ... };  // read once
while retries < max_retries {
    execute_sell_for_position(.., is_cashback, is_migrated).await; // stale forever
    ...
}
```

If a token migrated *during* the exit window, the WS-fed cache flips
`is_migrated` true within ~a slot — but the loop kept its stale `false`, so **all
10 retries routed to the bonding curve** and the on-chain program rejected each
with `BondingCurveComplete (6005)`. The position couldn't sell until a *later*
trade event happened to re-enter `on_trade_executed` and re-snapshot the flag.

## The fix

Move the `cache.get(&mint)` routing read **inside** the retry loop, so each
attempt re-reads `is_migrated` / `is_cashback` from the WS cache. The moment the
migration event lands, the next retry re-routes to the PumpSwap AMM path — and,
thanks to Phase 2, that AMM retry reads cached reserves and hits a pre-warmed
pool. Net effect: a mid-exit migration self-heals within one retry instead of
hanging the position.

- Cost: one extra `DashMap` read per retry (≤ 10, 1 s apart) — negligible.
- Safety: the `Ref` is dropped before the `await` (values copied out in the
  `match`), so no cross-await lock is held; behavior is identical when no
  migration occurs.

## Verification
- `cargo test -p backend --bins` → 19/19 pass (1 ignored, network-gated).

### Files touched (Phase 3)
- `backend/src/strategies/tpsl/service_tpsl.rs` — routing re-read inside the exit
  retry loop.
- `backend/src/strategies/tpsl_sniper_1/service_tpsl.rs` — same.

---

# Phase 4 — true `getMultipleAccounts` batching

**Date:** 2026-06-10
**Goal:** Collapse the paired reads that Phase 1 only *parallelized* into a single
RPC request.

## Context (what Phase 1 actually did)

Phase 1 used `tokio::try_join!` on the independent read pairs — that runs them
**concurrently** (≈ 1 RTT) but still sends **two** requests. Phase 4 upgrades
those pairs to one `getMultipleAccounts` call: same ~1 RTT, but a single request
(half the request count / RPC-quota, one fewer connection's overhead).

## Changes

- **`resolve_buy_routing`** (query.rs) — `[bonding_curve, mint]` →
  `get_multiple_accounts` (one request). Gates every manual buy/sell.
- **`get_creator_from_mint_pda`** (query.rs) — same `[bonding_curve, mint]` pair.
- **`amm_reserves`** (amm.rs) — `[base_vault, quote_vault]` →
  `get_multiple_accounts`, reading the raw SPL `amount` (u64 LE @ **offset 64**,
  after `mint[32] + owner[32]`) via the existing `read_u64` helper. Replaces the
  two `get_token_account_balance` calls. Layout is identical for Token and
  Token-2022, so it covers both vault kinds.

Result count is validated (`Vec → [Option<Account>; 2]`), and a missing account
errors explicitly (same outcome as the old per-account read failing).

## Why it's safe

- Routing/creator: `getMultipleAccounts` returns the same `Account` objects the
  old `get_account` calls did — byte-identical parsing, no decode change.
- `amm_reserves`: the only new assumption is the SPL `amount` offset (64). It's
  the well-known token-account layout and is now pinned by a unit test
  (`token_account_amount_lives_at_offset_64`). `amm_reserves` is also behind the
  Phase-2 cache, so it's a cold-path read, not the common case.

## Verification
- `cargo test -p pump-trader --lib` → **8/8** pass (6 reserve-cache + 2 new
  token-account-offset tests).
- `cargo check -p backend` → clean (public signatures unchanged).

### Files touched (Phase 4)
- `pump-trader/src/trader/query.rs` — `resolve_buy_routing`,
  `get_creator_from_mint_pda` → `getMultipleAccounts`.
- `pump-trader/src/trader/amm.rs` — `amm_reserves` → `getMultipleAccounts` +
  offset-64 read; offset unit tests.

---

## Status of the original future-work list

1. WS-fed reserve cache (curve + AMM) — **done (Phase 2)**.
2. AMM pool/config pre-warm on migration — **done (Phase 2)**.
3. `getMultipleAccounts` batching — **done (Phase 4)**.
4. Background blockhash cache — **done (Phase 5)**.

---

# Phase 5 — nonce-vs-blockhash investigation + background blockhash cache

**Date:** 2026-06-10

## The question

Why does `amm_buy` use a recent blockhash while `amm_sell` uses a durable nonce —
and is the nonce size limit (claimed earlier) actually real? (Note: *both* manual
and TPSL AMM **sells** use the nonce path; only the **buy** uses a blockhash, and
TPSL never AMM-buys — it buys on the curve, sells on AMM.)

## Measured answer (regression-guarded)

Built the **worst-case** AMM swaps (cashback coin + Token-2022 base — the largest
account list, no token-program dedup) from the **real** `amm_swap_accounts` + real
wrapper instructions, and measured the on-wire size (`amm::tests`):

| Tx | Size | vs 1232-byte limit |
|---|---|---|
| AMM **sell** + durable nonce | **1179 B** | fits (53 B headroom) |
| AMM **buy** + recent blockhash | **1171 B** | fits (61 B headroom) |
| AMM **buy** + durable nonce | **1245 B** | **over by 13 B** ✗ |

So the limit is **real**: a nonce-advance (+2 accounts ≈ +74 B) pushes the largest
buy past 1232. `amm_buy` must keep the recent blockhash; `amm_sell` is correctly
on the nonce. These three measurements are now assertions (`cashback_amm_*`), so
adding an account to the swap that would break either live path fails CI.

## Decision

Don't switch the buy to a nonce. Instead remove its per-buy `getLatestBlockhash`
RPC with a **background blockhash cache** — same latency win as a nonce, zero size
risk, no expiry risk.

## What was implemented

- **`BlockhashCache`** (new, `pump-trader/src/trader/blockhash.rs`) — `Mutex<Option<(Hash,
  Instant)>>`; `store` / `get_fresh(max_age)`.
- A **background refresher** spawned in `initialize()` primes the cache once, then
  refreshes every `BLOCKHASH_REFRESH_MS` (2 s).
- **`build_recent_tx`** reads the cache when fresh (`BLOCKHASH_CACHE_MAX_AGE_MS` =
  10 s, well inside a blockhash's ~60–90 s validity) and falls back to a live
  `getLatestBlockhash` otherwise — so a tx never rides an expired hash.

Effect: **manual AMM buys** drop their last per-trade RPC (`getLatestBlockhash`) →
served from cache. (TPSL is unaffected — it never AMM-buys — but the manual side
is now ~0-RPC on ready-time too.)

## Verification
- `cargo test -p pump-trader --lib` → **11/11** pass (6 reserve-cache + 2
  token-account-offset + 3 tx-size guards).
- `cargo test -p backend --bins` → **19/19** pass (1 ignored, network-gated).

### Files touched (Phase 5)
- `pump-trader/src/trader/blockhash.rs` — **new** `BlockhashCache`.
- `pump-trader/src/trader/mod.rs` — cache field + module.
- `pump-trader/src/trader/init.rs` — prime + background refresher.
- `pump-trader/src/trader/tx.rs` — `build_recent_tx` cache-first + fallback.
- `pump-trader/src/trader/amm.rs` — tx-size guard tests.
- `pump-trader/src/constants.rs` — `BLOCKHASH_REFRESH_MS`, `BLOCKHASH_CACHE_MAX_AGE_MS`.

---

## All four items complete

Across Phases 1–5, the original ready-time RPCs are gone for actively-traded
tokens: routing/account/reserve reads are cached or batched, AMM pools pre-warm on
first AMM trade, the exit path self-heals across migration, and the AMM buy's
blockhash is cached. Remaining latency now lives in **send/landing** (Jito tip,
sender, leader targeting) — a separate area, not ready-time.
