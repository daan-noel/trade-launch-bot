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

- ~~**Merge the two TPSL services into one shared module.**~~ **DONE (2026-06-10).**
  The premise was wrong: `TpslRuntimeCache` / `TPSLStrategyHandler` / the service /
  util were *byte-identical* copies (not divergent types), and the live functions
  `find_entry` / `find_exit` were byte-identical too — the only real fork was
  `simulation_tpsl.rs` (`tpsl_sniper_1` has the richer `simulate_exit` /
  `run_simulation` backtest). `tpsl` was the live-wired copy; `tpsl_sniper_1` is a
  strict superset. Resolved by repointing the live wiring (runner, app_state, main,
  `strategies/mod.rs`, api handler) `tpsl::` → `tpsl_sniper_1::` and **deleting the
  `strategies/tpsl/` folder** — zero live-behavior change (the
  `disabled_trailing_matches_legacy_find_exit` test guards the legacy exit path).
  No trait abstraction was needed. Future `tpsl_sniper_2/3/4` differ only in
  entry/exit; the next step is a shared core with a swappable `find_entry` /
  `find_exit` seam.
- ~~**Single confirmation source.**~~ **IMPLEMENTED (2026-06-10) — pending live
  validation.** The snipe buy double-confirmed the same event in series: an RPC
  `confirm_transaction` inside `buy_token_snipe` (`buy.rs`), *then* the strategy's
  WS/DB poll (`service_tpsl.rs`) which is what actually sets entry price/amount
  from the on-chain fill. Dropped the RPC confirm on the snipe path only
  (`skip_confirm`; manual `buy_token` keeps it). `buy_token_snipe` now returns the
  submitted signature; `buy_with_retries` was restructured so the WS/DB poll is the
  sole confirmation, with `signature_state` (one-shot status check, new in `tx.rs`)
  classifying a silent send: **re-send only on a confirmed on-chain revert**
  (`Some(false)`); on landed-but-lagging (`Some(true)`) wait without re-send; on
  pending/unknown (`None`) give up — never re-send a possibly-live nonce tx
  (double-buy guard). A top-of-attempt `record_entry_if_present` adopts any fill
  that landed before re-sending. Net: happy path drops ~5 RPC calls + the
  serialized confirm window; failure path keeps revert-retry. The post-poll
  decision is extracted into a pure `classify_silent_send` (enum `SilentSendOutcome`)
  with 5 unit tests (`service_tpsl.rs`) that lock the double-buy invariant —
  *re-send only on a confirmed on-chain revert* — without a chain/SOL.
  **Tier B (no-SOL full-flow tests):** the trader is abstracted behind a
  `SnipeExecutor` trait (`send_snipe_buy` / `check_signature` / `wallet`; impl'd for
  `PumpFunTrader`); `buy_with_retries` is now generic over it with injectable
  `BuyRetryCfg` timing. 6 `#[ignore]` `#[tokio::test]`s drive a scripted
  `FakeExecutor` against a **real local Postgres** (unique mint/wallet ids,
  self-cleaning) covering: happy-path entry record, top-guard adoption (0 sends),
  revert→resend→record (2 sends), pending→give-up (1 send), status-error→give-up
  (1 send), landed-lag→record-without-resend (1 send) — each asserts the send count
  so the double-buy guard is proven end-to-end. **Run:**
  `$env:DATABASE_URL=...; cargo test -p backend -- --ignored` (they compile in CI but
  skip without a DB). **Open:**
  validate on a low-size live snipe (watch re-send / poll-timeout rates); tune the per-attempt
  poll window (currently `BUY_POLL_MAX_ATTEMPTS`×`BUY_POLL_INTERVAL_MS` = 12×1s) for
  faster revert detection; a dropped-tx (`None`) could later be safely re-sent via
  nonce-account introspection (future).
- ~~**`getMultipleAccounts`/PDA-derivation helper** shared across buy + query.~~
  **DONE (2026-06-10).** Two `pub(super)` helpers in `query.rs`:
  `bonding_curve_pda(mint)` (the single `bonding-curve` PDA) and
  `derive_token_pdas(mint, creator, token_program, cashback)` (the full curve-PDA
  set → `TokenPDAs`), both off `self.pump_program`. Rewired all four inline sites —
  `buy_token_inner` (`buy.rs`), `resolve_buy_routing`, `resolve_migrated_batch`,
  `get_creator_from_mint_pda` (`query.rs`) — dropping the redundant
  `Pubkey::from_str(PUMP_FUN_PROGRAM_ID)` re-parses (import removed from `query.rs`)
  and `TokenPDAs` from `buy.rs`'s imports. Behaviour-identical by construction
  (same seeds, same program id); `cargo check`/`test -p pump-trader` clean, backend
  builds. The pure derivation was shared first; the single-mint
  `getMultipleAccounts` read + offset-parse was then also extracted into
  `read_curve_routing(mint) -> CurveRouting` (creator / token_program /
  is_migrated / cashback), so `resolve_buy_routing` and `get_creator_from_mint_pda`
  now share **both** the read and the derivation. The only standalone read left is
  `resolve_migrated_batch` (a batched 100-mint-per-request variant — intentionally
  separate).

## Live validation (pending — snipe-buy confirmation change)

Use a **0.001 SOL** buy size for the first live snipe test (the ~0.0004 SOL tx fee
is a large fraction of anything smaller). Buy size is the TPSL rule's `buy_amount`
(DB-driven, set via the API/frontend) — not a code constant. Watch the re-send /
poll-timeout log rates and entry-record latency.

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
