# Buy/Sell workflow cleanup — retry collapse, dedup & hot-path hygiene

**Date:** 2026-06-10
**Scope:** `pump-trader` crate (trader hot path + helpers) and the two TPSL
strategy services in `backend`.
**Goal:** Phases 1–5 already removed the per-trade RPCs, so *ready-time* is lean.
This pass targets what's left: the **retry/confirmation orchestration** (which
multiplies transactions), a few **per-call inefficiencies**, and **duplicated
code**. The aim is low latency *and* stability *and* clean, modular code — not
shaving the tx-build path further.

> Continuation of [latency-trade-path-rpc-reduction.md](latency-trade-path-rpc-reduction.md)
> (Phases 1–5). Think of this as "Phase 6".

---

## Findings

### 1. The workflow problem — triple-nested sell retries + double confirmation

The sell path stacks **three independent retry loops**, each sending real
transactions:

- `on_trade_executed` → `while retries < 10` (1 s sleep)  — `service_tpsl.rs`
- `sell_with_retries` → `SELL_MAX_ATTEMPTS = 6` (backoff)  — `service_tpsl.rs`
- `sell_token` → `MAX_SELL_ATTEMPTS = 5` (250 ms sleep)    — `sell.rs`

→ worst case **10 × 6 × 5 = 300** sell transactions for a single curve exit
(AMM is 10 × 6 = 60, since `amm_sell` is already single-attempt). Each reverting
tx still pays base + priority fee.

- **Double-send / over-sell risk:** the innermost loop retries the *same fixed
  amount* with no balance re-check. If a tx lands late while `confirm_transaction`
  times out, the next attempt fires a second full sell. Only the outer
  `sell_with_retries` adjusts `amount` from the WS balance.
- **Double confirmation:** every trade blocks on `confirm_transaction`
  (RPC `getSignatureStatus`) **and then** the caller re-confirms by polling the
  DB/WS feed. Two mechanisms confirming the same thing.
- **Wasted leading latency:** `confirm_transaction` sleeps `CONFIRM_POLL_MS`
  (1 s) *before* its first status check, so every trade waits ≥ 1 s to return
  even when it landed in 400 ms.

### 2. Per-call inefficiencies

- **New HTTP client every wallet scan:** `get_all_token_accounts` builds a fresh
  `reqwest::Client` (new TLS/conn pool) per call instead of reusing `self.http`.
- **Jito tip behind an async Mutex:** `jito_tip_ix: Arc<Mutex<Option<…>>>` is set
  once at init but `lock().await.clone()`'d on every buy/sell. It's immutable
  after `initialize` — should be a plain field like `compute_budget_ixs`.
- **Hot-path caches use `tokio::Mutex`:** `token_pdas`, `user_token_accounts`,
  `amm_pool_cache`, `amm_global_config` lock with an *async* mutex for tiny
  non-await get/insert sections. `std::sync::Mutex` (as `ReserveCache` /
  `BlockhashCache` already do) is cheaper and removes await points.
- **Constant AMM PDAs re-derived every swap:** `amm_swap_accounts` runs ~5
  `find_program_address` calls (global_config, event_authority, fee_config,
  global/user volume accumulators) that never change — derivable once.

### 3. Duplication / modularization

- **Two near-identical strategy services:** `tpsl/service_tpsl.rs` and
  `tpsl_sniper_1/service_tpsl.rs` differ by **4 module-path lines + 1 attribute**
  out of 667. The whole orchestration is copy-pasted.
- **Three copies of template-building:** `build_template` is reimplemented inline
  in `replenish_pool_async` and `prebuild_one_template_async`, which also both run
  after every buy doing nearly the same top-up.
- **PDA derivation duplicated** across `buy_token_inner`,
  `get_creator_from_mint_pda`, `resolve_buy_routing`.

---

## Plan (this pass)

**Trader crate (`pump-trader`)**

- **T1** `jito_tip_ix` → plain `Option<Instruction>` (set in `initialize`); drop
  the Mutex and every `lock().await.clone()`.
- **T2** `get_all_token_accounts` reuses `self.http`.
- **T3** `confirm_transaction` checks status *before* the first sleep (fast
  return when a tx lands quickly).
- **T4** Convert `token_pdas` / `user_token_accounts` / `amm_pool_cache` /
  `amm_global_config` from `tokio::sync::Mutex` to `std::sync::Mutex` (the
  compiler enforces no guard is held across an `.await`).
- **T5** Precompute the program-constant AMM PDAs once in `new()` and read the
  fields in `amm_swap_accounts`.
- **T6** Extract `sell_token_once` (one attempt: ensure PDAs + account, acquire
  nonce, build/send/confirm). `sell_token` becomes a thin retry loop over it
  (manual keeps its retry); TPSL calls `sell_token_once` so it stops multiplying
  the service-level retries.
- **T7** `replenish_pool_async` / `prebuild_one_template_async` reuse one shared
  template builder; keep a single post-buy top-up.

**Backend strategies**

- **B1** Collapse the `on_trade_executed` 10× loop: move the per-attempt
  migration/cashback re-read **into** `sell_with_retries` (pass `&TokenCache`),
  and call `execute_sell_for_position` once. Migration self-heal is preserved
  (each `sell_with_retries` attempt re-routes); the ~12 s backoff window matches
  the old ~10 s.
- **B2** TPSL curve sell calls `sell_token_once` instead of `sell_token`.

Net sell attempts per exit: curve **300 → ≤ 6**, AMM **60 → ≤ 6**.

---

## Deferred (recommended next, higher-risk)

- **Merge the two TPSL services into one shared module.** They're byte-identical
  bar module paths, but `TpslRuntimeCache` / `TPSLStrategyHandler` are *distinct
  types* per module, so sharing needs a trait abstraction over them. High value,
  but wants integration tests before doing it blind.
- **Single confirmation source.** Drop the blocking RPC confirm on the snipe buy
  entirely and let the WS/DB poll be the sole confirmation (it already sets entry
  price). Left out of this pass to keep the change set reviewable.
- **`getMultipleAccounts`/PDA-derivation helper** shared across buy + query.

---

## Status — implemented

All of T1–T7 + B1 + B2 landed (the deferred items above are still open).

### Files touched

- `pump-trader/src/trader/mod.rs` — `jito_tip_ix` → plain `Option<Instruction>`;
  four caches → `std::sync::Mutex`; precomputed AMM PDA fields + `new()` derives
  them; module-doc refreshed.
- `pump-trader/src/trader/init.rs` — set the plain `jito_tip_ix` field.
- `pump-trader/src/trader/buy.rs` — read the plain tip field; drop the redundant
  `prebuild_one_template_async` call; `.lock().unwrap()`.
- `pump-trader/src/trader/sell.rs` — **new `sell_token_once`** (single attempt);
  `sell_token` is now a thin retry loop over it; account resolution via
  `resolve_cached_token_account`; `.lock().unwrap()`.
- `pump-trader/src/trader/amm.rs` — tip field; AMM swap uses precomputed PDA
  fields; removed the program-constant PDA helper methods; `.lock().unwrap()`.
- `pump-trader/src/trader/query.rs` — `get_all_token_accounts` reuses `self.http`;
  `.lock().unwrap()`.
- `pump-trader/src/trader/tx.rs` — `confirm_transaction` polls before sleeping.
- `pump-trader/src/trader/pool.rs` — shared `build_template_with_seed` free fn;
  `space_rent_for` helper; deleted `prebuild_one_template_async`.
- `backend/src/strategies/tpsl/service_tpsl.rs` and
  `backend/src/strategies/tpsl_sniper_1/service_tpsl.rs` (identical) —
  `on_trade_executed` real branch is a single exit pass; `sell_with_retries` /
  `execute_sell_for_position` take `&TokenCache` and re-read routing each attempt;
  curve sell calls `sell_token_once`.

### Verification (run)

- `cargo check -p pump-trader` / `-p backend` → clean (no new warnings; the
  remaining backend warnings are pre-existing unused imports in
  `handler_tpsl.rs` / `mod.rs` / `simulation_tpsl.rs`).
- `cargo test -p pump-trader --lib` → **11/11** pass.
- `cargo test -p backend --bins` → **19/19** pass (1 ignored, network-gated).

### Behavioural notes

- Manual trading is unchanged: `manual_sell` still calls `sell_token` (which keeps
  its `MAX_SELL_ATTEMPTS` retry, now expressed as a loop over `sell_token_once`).
- TPSL exits: a held position that fails all `sell_with_retries` attempts is
  reopened to Holding and re-triggers on the next trade event (event-driven retry)
  instead of a blind 1 s × 10 outer loop — so migration self-heal is preserved
  while the worst-case transaction count drops from ~300 to ≤ 6.
