# Helius RPC usage reduction — plan

> **Status (2026-07-17): IMPLEMENTED — Phases 1 AND 2 (2.1 + 2.2; 2.3 skipped as designed).**
> All checks/tests/clippy green (`hunter-live`/`hunter-lab`/`forge-live` + executor/ingest
> crates). Runtime paper smoke + post-deploy Helius-dashboard verification still pending.
> Deviations from the text below, chosen during implementation:
>
> - **1.3/2.1 merged:** the interim 30 s/45 s blockhash poll values were skipped — hunter goes
>   straight to the endgame: `blockhash_refresh_ms = 10_000` (watchdog tick),
>   `blockhash_max_age_ms = 30_000`, fed by the `blocks_meta` push. The executor loop itself
>   became skip-if-fresh (`BlockhashCache::is_fresher_than`), so forge's 300 s poll behavior
>   is unchanged.
> - **1.4:** poll set to **60 s** (not 120 s) — the cached balance feeds `can_commit_buy`,
>   a real buy gate (the plan's own conditional).
> - **B6 heal:** `refresh_amm_pool_info` now compares `coin_creator_vault_authority` instead
>   of `coin_creator` — a feed-harvested `AmmPoolInfo` doesn't know the raw creator (a swap's
>   account list carries only the derived vault pair); harvested entries store
>   `coin_creator = Pubkey::default()`.
> - **2.2:** executor entry point is `on_nonce_account_update(pubkey, data, slot)` (raw
>   account bytes; the nonce-state parse lives in the executor next to the RPC decode).
>   Re-arm is slot-gated per account and the fallback poll is `use_epoch`-guarded so
>   push-then-poll can never double-free a slot; the poll also early-exits (zero RPC reads)
>   once the push wins. `refresh_first_delay_ms` (new `NonceCfg` knob, default 0) is 2 s on
>   hunter only; `refresh_max_attempts` default 8 → 4.
> - **Addition — `amm_config` stale-while-revalidate:** deleting the prewarm removed the
>   accidental keep-warm of the PumpSwap GlobalConfig (fee bps, 300 s TTL). `amm_config` now
>   serves the stale value and refreshes in the background (deduped), so a real AMM exit
>   never blocks on that `getAccountInfo` once the process has fetched it once.

**Goal:** cut hunter-live's steady-state Helius HTTP RPC usage to ≈ zero while *improving*
real-SOL trading reliability. Diagnosis (2026-07-17, confirmed against the Helius per-method
dashboard): usage is dominated by `getTransaction`, whose **only automatic caller** is the
AMM pool prewarm (`fetch_fee_share_marker`, ~15 `getTransaction` per prewarm) triggered from
the ingest consumer for **every** migrated token in the ~25k tracking cache — amplified by a
failure-retry loop (`consumer.rs` resets `amm_pool_prewarmed` on error → re-burst per trade)
and a RAM-only flag (full re-warm every restart).

**Design principle:** the LaserStream gRPC firehose already carries every transaction with
its full (ALT-resolved) account list. RPC is reserved for (1) submitting txs, (2) resolving
ambiguity the feed can't prove (dropped tx), (3) boot snapshots. Any steady-state RPC read
of data the feed already carries is a bug.

Scope: `shared/executor/{core,pumpfun}`, `shared/ingest/pumpfun`, `hunter/{core,live}`.
Shared crates feed forge too — every API change must keep `forge-live` compiling.

---

## Phase 1 — kill `getTransaction` + always-on baseline (no transport changes)

### 1.1 Harvest AMM pool facts from the feed; delete the prewarm

Every PumpSwap swap instruction's account list contains everything the executor needs to
build its own swap for that coin: pool, both vaults, creator-vault ata+authority, and the
tail that both discriminates cashback vs non-cashback **and** carries the fee-share marker
(3rd-from-last account). Harvest it passively at decode time — zero RPC, and strictly
better coverage than the prewarm (any token with one observed AMM swap since boot is warm;
a harvest cannot fail the way an RPC prewarm can).

**(a) executor-pumpfun — parser + observe API** (`trader/amm.rs`):

- Extend `AmmPoolInfo` with `coin_creator_vault_ata: Pubkey` + `coin_creator_vault_authority: Pubkey`
  (today derived from `coin_creator` inside `build_swap_accounts` — move the derivation to
  `amm_pool_info` construction so both the RPC path and the harvest path fill the same
  struct, and `build_swap_accounts` just reads them). `coin_creator` stays (B6 self-heal
  re-derives from it).
- New `pub fn observe_amm_swap_accounts(&self, token_mint: &str, base_token_program_id: &str, keys: &[String]) -> bool`:
  - No-op `true` if `amm_pool_cache` already has the mint.
  - Parse `keys` as one PumpSwap buy/sell account list: **head by fixed IDL index**
    (0 pool … 7 pool_base_ata, 8 pool_quote_ata, 17 coin_creator_vault_ata,
    18 coin_creator_vault_authority — mirror `build_swap_accounts`, the SSOT for the
    layout), **tail by position from the end**: `keys[len-3] == PUMP_AMM_CASHBACK_GLOBAL`
    ⇒ cashback coin (`fee_share_marker = None`), else ⇒ `fee_share_marker = Some(keys[len-3])`
    (`len-2`/`len-1` are the buyback recipient + its WSOL ata — sanity-check `len-2 ==
    PUMP_AMM_BUYBACK_FEE_RECIPIENT`, reject the parse otherwise so a program upgrade fails
    safe to the cold path).
  - On successful parse: insert `AmmPoolInfo`, return `true`; any mismatch (length, pool ≠
    `derive_amm_pool(mint)`, sanity check) ⇒ return `false`, no side effects.
- **Guard test (no-DB, runs on plain `cargo test`):** build accounts for a synthetic pool via
  `build_swap_accounts` (both cashback and marker variants, buy and sell), feed them to the
  parser, assert the round-tripped `AmmPoolInfo` equals the input. This pins parser ↔
  builder to the same layout forever.
- Delete `prewarm_amm_pool`. (Before deleting, grep both products for callers —
  `hunter/live/src/services/token_sync.rs` registers a pool *watch* around line 606; if it
  also prewarms, point that one manual path at `amm_pool_info` directly.)

**(b) ingest-pumpfun — expose the swap's account keys** (`decode/grpc.rs`):

- In `decode_amm_live_pb`, when a **top-level** pump_swap buy/sell ix is decoded (skip
  inner-CPI routed swaps — the next direct swap will provide), resolve the ix's account
  indices through `LazyKeys` (static + ALT-loaded, the full resolved list) and attach them
  to the emitted trade event as `amm_swap_accounts: Option<Box<Vec<String>>>`.
- Hot-path budget: AMM trades are a small fraction of the firehose and the field is `None`
  everywhere else; `Box` keeps the event size flat. Failed txs are already not decoded as
  trades, so only successful swaps are harvested.

**(c) hunter — wire the hook, delete the prewarm trigger:**

- `hunter/core/src/ingest.rs`: replace `TraderHook::prewarm_amm_pool` with
  `fn observe_amm_swap_accounts(&self, mint: &str, token_program: &str, keys: &[String]) -> bool`
  (sync, no boxed future — it's pure CPU). Update `hunter/live/src/trader/trader_hook_impl.rs`.
- `hunter/live/src/ingest/consumer.rs` (trade handler): replace the `to_warm` +
  `tokio::spawn(prewarm)` block with: if `is_amm && !token_state.amm_pool_prewarmed` and the
  event carries `amm_swap_accounts`, call the hook inline (cheap parse, no I/O) and set
  `amm_pool_prewarmed = hook_returned_true`. No spawn, no reset-on-error loop — a failed
  parse just retries on the next swap for free.
- Keep the field name `amm_pool_prewarmed` (semantics unchanged: "trader cache warm for
  this mint").

**What remains RPC:** only the cold fallback inside `amm_pool_info` (manual/exit trade of a
token with no observed swap since boot) — see 1.2.

### 1.2 Harden the cold fallback (`fetch_fee_share_marker`)

- Signature limit 15 → **5**; fetch **sequentially with early exit** on first marker (the
  marker is in every successful swap, so tx #1 almost always suffices) instead of the
  current JoinSet that spawns all 15 `getTransaction` concurrently regardless of success.
- The caller (`amm_pool_info`) is only reached from real trade/manual paths now, whose
  retries are already bounded — no unbounded storm remains by construction.

### 1.3 Blockhash refresher interval (A1)

`hunter/live/src/main.rs` (`TraderConfig::new(...)` site, ~line 508): before wrapping in
`Arc`, override `config.cache.blockhash_refresh_ms = 30_000` and
`config.cache.blockhash_max_age_ms = 45_000` (hash validity is ~60–90 s; forge already
overrides to 300 s). Leave the executor crate default untouched. ~43,200 → ~2,880 calls/day.

### 1.4 SOL-balance poll (A2)

`hunter/live/src/main.rs` ~line 565: `sleep(30 s)` → `120 s` — **after verifying** the
cached balance's consumers are UI/telemetry only; if any affordability/buy-gate reads it,
use 60 s instead. Minor (~2.2k calls/day saved).

**Phase 1 result:** `getTransaction` ≈ 0/day; always-on baseline ~46k → ~4k calls/day;
everything left scales with actual trades.

---

## Phase 2 — push-based feeds (one transport change-set, optional but the endgame)

Both items extend the existing LaserStream subscription (`shared/ingest/core/src/transport/`),
so do them together. gRPC messages are covered by the LaserStream subscription, not
per-call RPC credits.

### 2.1 `blocks_meta` subscription → blockhash cache (A1 → 0)

- Add a `blocks_meta` filter to the Yellowstone `SubscribeRequest`; surface
  `(slot, blockhash)` through the host-adapter bridge.
- Executor: new `pub fn set_cached_blockhash(&self, hash, slot)` (accept only newer slots).
- Demote the refresher loop to a **watchdog**: tick 10 s, call `getLatestBlockhash` only if
  the cache is older than 10 s (feed stalled). Steady state: 0 RPC, hash fresher (~400 ms)
  than today's 2 s poll.

### 2.2 Nonce-account subscription → push re-arm (B2 → ~0)

- Subscribe the (small, fixed) nonce pool pubkeys via an `accounts` filter; on update,
  parse the nonce state's blockhash and call a new executor
  `pub fn on_nonce_account_update(&self, pubkey, hash)` that re-arms the slot.
- Keep `schedule_nonce_refresh` as fallback only: first poll delayed to 2 s (push normally
  wins), `refresh_max_attempts` 8 → 4. Aligns with the "notify over poll" rule.

### 2.3 (optional) Event-sourced SOL balance (A2 → ~0)

Debit on send / settle on confirmed fill + reconcile poll every 5–10 min + on-demand refresh
endpoint. Low value (getBalance is cheap) — do only if touching that code anyway.

---

## Explicitly unchanged (already minimal-correct designs)

| Consumer | Why it stays |
| --- | --- |
| B1 send fan-out | Submitting txs is RPC's job; scales with our own orders; landing reliability > pennies. |
| B3 AMM reserve fallback | Feed-first cache + staleness-gated RPC is the ideal shape (the pattern 1.1 copies). |
| B4/B5 `getSignatureStatuses` on feed-miss | The feed can't prove a *dropped* tx; one call on ambiguity prevents double-buy/sell. |
| B6 2006 self-heal | Rare, one call, fixes a real revert. |
| D boot snapshots | ~20 calls per restart. |
| E manual endpoints | On-demand only; bulk sync already routed via cheaper gTFA. |

## Definition of done / verification

- `cargo check` clean: `-p hunter-live -p hunter-lab -p forge-live -p executor-pumpfun`
  (+ ingest crates); clippy on touched code; no new warnings.
- New round-trip guard test green on plain `cargo test -p executor-pumpfun`; existing
  executor + ingest tests green.
- Runtime smoke (workstation, paper): run `hunter-live`, confirm a log line per first-seen
  AMM mint showing harvested pool facts; confirm an AMM paper exit builds its swap without
  any `getTransaction` in the RPC log.
- Post-deploy (EC2): Helius per-method dashboard — `getTransaction`/day ≈ 0,
  `getLatestBlockhash` ≈ 2.9k/day (Phase 1) → ≈ 0 (Phase 2).

## Risks

- **Program layout change on pump_amm upgrade:** parser sanity checks (`len-2` buyback
  recipient, pool PDA match) make a layout drift fail *safe* to the cold RPC path; the
  round-trip guard test catches builder/parser skew at compile-test time.
- **Inner-CPI-only swaps** aren't harvested — cold fallback covers; acceptable.
- **Shared-crate API changes** (`AmmPoolInfo` fields, hook signature, `prewarm_amm_pool`
  deletion) ripple to forge — verify forge call sites before changing signatures.
