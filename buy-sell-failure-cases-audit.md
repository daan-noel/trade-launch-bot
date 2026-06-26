# Buy/Sell Transaction Flow — Failure-Case Audit & Remediation Plan

## Context

The bot trades pump.fun tokens across two venues (bonding **curve** pre-migration,
PumpSwap **AMM** post-migration) on two driver paths (**bot** = tpsl snipers,
**manual** = API handlers). A transaction can fail for many reasons; some are code
bugs, some are unhandled exception cases. A known example is `Custom(6024)` (missing
`user_volume_accumulator` on a cashback curve sell); a separate creator-vault
staleness bug (`2006`) was already fixed.

This document is a **complete, verified catalog of every way a buy/sell can fail**
plus a **prioritized remediation plan** with exact change sites, so it can be
implemented from in a later session. No code is changed by this document.

Decisions taken: document **all** problems in detail; cashback fix = **Fix 1 + Fix 2**.

---

## Flow map (grounding)

| | Curve (pre-migration) | AMM (post-migration) |
|---|---|---|
| Bot buy | `buy_token_snipe[_write_ahead]` → `buy_token_inner` ([buy.rs](pump-trader/src/trader/buy.rs)) | n/a (snipes are always fresh creates) |
| Bot sell | `sell_token_once` (`confirm=false`, feed-confirmed) | `amm_sell` (`confirm=false`) |
| Manual buy | `buy_token` ([solana.rs:122-134](backend/src/api/handlers/trading/solana.rs#L122-L134)) | `amm_buy` |
| Manual sell | `sell_token` ([solana.rs:232-244](backend/src/api/handlers/trading/solana.rs#L232-L244)) | `amm_sell` |

- **Venue selection** = `is_migrated`. Manual: `routing.is_migrated` from a **live**
  `resolve_buy_routing`. Bot: `token_cache.is_migrated` (WS-fed, re-read each attempt).
- **Cashback** gates only the **curve sell** UVA account (slot 14), via the guard
  `if is_cashback || pdas.cashback_enabled` ([sell.rs:450](pump-trader/src/trader/sell.rs#L450)).
  Curve **buy** pushes the accumulators unconditionally; **AMM** reads cashback from
  the pool on-chain. So 6024 is a **curve-sell-only** failure mode.
- **Bot sell retry/classifier**: `sell_until_balance_cleared` + `classify_sell_revert`
  ([real.rs:798-830, 1058-1095](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L798-L830)),
  `SELL_MAX_ATTEMPTS = 6`. **`tpsl_sniper_2` is an intentional clone — mirror every
  edit** (classifier at tpsl2 `real.rs:943-975`, RefreshCreator arm ~`1216-1231`).
- Key reuse: `refresh_curve_creator_vault` ([query.rs:564-570](pump-trader/src/trader/query.rs#L564-L570))
  calls `ensure_token_pdas`, which re-reads **creator AND cashback** from chain into
  the cached `pdas` — so the bot cashback/migration recovery can reuse this machinery.

---

## Failure-case catalog (all cases, with current status)

Status: ✅ handled · ⚠️ partial/conditional · ❌ broken/unhandled · 🕒 latent (breaks on an external change)

### Quick matrix — the two flags you called out

**`is_cashback`** (curve-sell UVA only):

| Path | Source | Robust? |
|---|---|---|
| Curve **buy** (bot + manual) | n/a — accumulators pushed unconditionally | ✅ can never 6024 |
| Curve **sell — bot** | `token_cache.is_cashback_enabled` (set at create) | ⚠️ stale if toggled (B3) |
| Curve **sell — manual** | `token_cache` ❌ (should be `routing.cashback_enabled`) | ❌ 6024 bug |
| AMM **buy/sell** | `pool.is_cashback_coin` (on-chain) | ✅ immune |

**`is_migrated`** (venue selection):

| Path | Source | Wrong-`false` → | Wrong-`true` → |
|---|---|---|---|
| Buy — manual | `routing.is_migrated` (live) | curve buy on migrated → **6005** | AMM pool-not-found |
| Buy — bot | n/a (always curve) | — | — |
| Sell — manual | `routing.is_migrated` (**resolved once** before the loop) | curve sell on migrated → **6005** | AMM pool-not-found |
| Sell — bot | `token_cache.is_migrated` (re-read each attempt) | **6005**, self-corrects only if cache flips | — |

### A. On-chain reverts (tx lands, then fails — fee is paid)

- **A1 `Custom(6024)` — missing UVA on a cashback curve sell** ❌
  - Manual sell: reads `is_cashback` from `token_cache` ([solana.rs:182-186](backend/src/api/handlers/trading/solana.rs#L182-L186)),
    not the `routing` already fetched at line 170. Bites when the mint's `TokenPDAs`
    are already cached with the hardcoded `false` (a prior op on the same mint this
    process), so the sell skips its own `ensure_token_pdas` re-read and both OR
    operands are `false`. → **Fix 1**.
  - Bot sell: `is_cashback` from `token_cache` (set at create, never refreshed). A
    6024 here → `classify_sell_revert` falls to `StopFeeBurn` → `ExitFailed`, **no
    recovery**. → **Fix 3**.
- **A2 `Custom(6000)` NotAuthorized — wrong curve fee recipient** ✅ / 🕒
  - Fixed via hardcoded `PUMP_CURVE_FEE_RECIPIENT` ([constants.rs:56](pump-trader/src/constants.rs#L56)).
    pump.fun already rotated this once. The next rotation breaks **every** curve trade
    until the constant is updated. Same risk for `PUMP_AMM_BUYBACK_FEE_RECIPIENT` /
    `PUMP_AMM_CASHBACK_GLOBAL`. → **Fix 7 (monitoring)**.
- **A3 `ConstraintSeeds(2006)` — stale creator_vault (curve sell)** ✅ / ⚠️
  - Bot: classified → `refresh_curve_creator_vault` → retry (both clones). Manual:
    protected because it passes `Some(&routing.creator)` (live) and `execute_sell`
    recomputes the vault ([sell.rs:302-315](pump-trader/src/trader/sell.rs#L302-L315)).
    Residual: a creator rotated more times than `SELL_MAX_ATTEMPTS` → `ExitFailed`.
- **A4 `BondingCurveComplete(6005)` — curve trade after migration** ⚠️/❌
  - Manual sell: routing resolved **once before** the clear loop ([solana.rs:170](backend/src/api/handlers/trading/solana.rs#L170));
    the loop re-reads balance but not routing → a token migrating **mid-loop** routes
    every pass to the curve → repeated 6005 → 500. → **Fix 2**.
  - Bot: a 6005 where `token_cache.is_migrated` is still `false` (used==now==false) is
    **not** special-cased → `StopFeeBurn` → `ExitFailed`. This is the "missed migration
    event" case (memory `missed-tokens-restart-replay-gap`). → **Fix 3**.
- **A5 slippage reverts `6003` (curve) / `6004` (AMM)** ⚠️
  - Sell (bot + `sell_token`): ✅ retried with a fresh-reserve re-quote. **Manual buy:
    ❌ single-shot, a transient slippage revert → 500, no retry.** → **Fix 5**.
- **A6 AMM `Overflow` / bad account list** 🕒 — the trailing buyback block, cashback
  block, and per-coin fee-share marker are reverse-engineered & hardcoded
  ([amm.rs:468-512](pump-trader/src/trader/amm.rs#L468-L512)). A pump_amm layout
  upgrade reverts **every** AMM swap. → **Fix 7 (monitoring)**.
- **A7 compute-unit exhaustion** ⚠️ accepted — outlier heavy txs exceed
  `COMPUTE_UNIT_LIMIT_*` and revert while paying the fee ([constants.rs:170-187](pump-trader/src/constants.rs#L170-L187)).
  Deliberate (~1-in-15). Leave as-is; mention in monitoring.
- **A8 `close_account` reverts on dust** ✅ — preflight on, no tip, fails cheaply.

### B. Pre-send resolution / routing

- **B1 `resolve_buy_routing` RPC failure (manual buy + sell)** ⚠️ — flaky
  `getMultipleAccounts` → 400, **no retry**. Every manual trade gates on it.
  → **Fix 6 (small retry)**.
- **B2 AMM pool/config/marker resolution** ⚠️ — pool missing → bail
  ([amm.rs:556-560](pump-trader/src/trader/amm.rs#L556)); **first seller of a
  just-migrated non-cashback token can't sell** until someone else trades it
  (fee-share marker has no source, [amm.rs:574-582](pump-trader/src/trader/amm.rs#L574-L582)).
  AMM reserve read **propagates** its error (not a silent fallback). → mostly accept;
  surface a clearer error.
- **B3 cashback mutability (`toggle_cashback_enabled`)** ❌ — cashback is **mutable
  on-chain** (instruction exists in both IDLs) but the bot **never observes or
  re-reads it**: set at create ([decoder/create.rs:110-113](backend/src/ingest_laserstream/decoder/create.rs#L110-L113)),
  no ingest handler, DB never `UPDATE`d, `token_cache` value immutable. A creator
  toggling cashback after the snipe makes the bot's flag stale → 6024 (toggle-on) on
  the curve sell. → **Fix 3** (covers it).
- **B4 bot `token_cache` cold start** ⚠️ — a miss defaults `(is_cashback=false,
  is_migrated=false)`; a position acted on before DB-seed completes (or whose create
  was never ingested) can mis-route. Related to memory `missed-tokens-restart-replay-gap`.

### C. Execution environment

- **C1 nonce starvation under volume** ⚠️ — `acquire_nonce` bails "All nonce slots
  busy" ([nonce.rs:102](pump-trader/src/trader/nonce.rs#L102)). Pool size is fixed.
  → **Fix 7 (observability)**.
- **C2 nonce slot with no cached hash** ⚠️ — after an all-reads-failed refresh the
  slot's hash is cleared and isn't handed out until refreshed (compounds C1).
- **C3 nonce authority misconfigured** 🕒 — authority ≠ wallet → every durable-nonce
  tx on that slot fails silently; `check_nonce_authorities` is a manual audit only.
- **C4 re-arming a consumed nonce hash** ✅ — explicitly prevented ([nonce.rs:117-158](pump-trader/src/trader/nonce.rs#L117-L158)).
- **C5 recent-blockhash expiry (AMM buy)** ✅/⚠️ — 10 s freshness bound; a stalled
  refresher past validity drops the AMM buy.
- **C6 Jito auction loss / never lands** ✅ (sell retried) / ⚠️ (buy single-shot,
  covered by write-ahead recovery — see `@plans/trade-execution/buy-in-flight-recovery.md`).
- **C7 `confirm_transaction` false timeout (manual)** ⚠️ — ~2.35 s window
  ([constants.rs:193-203](pump-trader/src/constants.rs#L193-L203)). **Manual buy:**
  times out → 500, but the durable-nonce buy can land later → tokens into an
  **untracked** balance (manual buy opens no position; only the read-only boot
  `wallet_reconcile` flags it). → **Fix 5b**.
- **C8 sender fan-out total failure** ✅ — errors only if every endpoint fails.

### D. Account / balance / input

- **D1 no token account on sell** ✅ — bails. **D2 garbage `sol_amount`** ✅ —
  `buy_lamports_checked` + `MAX_BUY_SOL`. **D3 slippage range** ✅ — clamped `[10,5000]`.
  **D4 wrong token program** ✅ — sourced live from mint owner.
- **D5 `slippage None` = `min_out=1`** ⚠️ value-loss (MEV), not a revert. Note: the AMM
  builders also treat `None` as 1 — **`AMM_DEFAULT_SLIPPAGE_BPS` is dead code**
  (declared [constants.rs:238](pump-trader/src/constants.rs#L238), used only in a doc
  comment), so the "AMM None = 5%" claim in
  [@plans/trade-execution/slippage-logic-buy-sell.md](@plans/trade-execution/slippage-logic-buy-sell.md)
  is **wrong**. → **Fix 8 (doc/const cleanup)**.

### E. Concurrency / bookkeeping

- **E1 double-sell** ✅ (ExitGuard + per-signature attribution + full-window poll).
- **E2 double-buy** ✅ (never-resend + write-ahead marker + EntryGuard).
- **E3 position stuck `Holding` after manual sell** ⚠️ — `reconcile_externally_cleared_mint`
  retries ~12 s; reaper backstops (memory `manual-sell-holding-forever-bug`).
- **E4 `ExitFailed` parked** ⚠️ — needs manual action; if the DB write that marks it
  fails, the row stays `Holding`.
- **E5 untracked manual buy** ⚠️ — see C7.

---

## Remediation plan (prioritized)

### P0 — correctness bugs causing real failed trades

#### Fix 1 — manual-sell 6024 (cashback) [manual-path]
- **File:** [solana.rs:182-186](backend/src/api/handlers/trading/solana.rs#L182-L186)
- **Change:** replace the `token_cache` lookup with the live routing value already in
  scope from line 170:
  ```rust
  // before: token_cache (stale/empty → false)
  let is_cashback = routing.cashback_enabled;
  ```
- **Why correct:** `resolve_buy_routing` reads the bonding curve live each manual action
  (offset 82), so it's always current — and, being a re-read, also robust to a cashback
  toggle (B3) on the manual path.

#### Fix 2 — manual-sell re-resolve routing inside the clear loop [manual-path]
- **File:** `manual_sell` loop [solana.rs:199-253](backend/src/api/handlers/trading/solana.rs#L199-L253)
- **Problem:** `routing` (venue + creator + cashback) is resolved once at line 170; a
  mid-loop migration keeps routing every pass to the curve → 6005.
- **Change:** move `resolve_buy_routing` (and the `is_cashback = routing.cashback_enabled`
  from Fix 1) to the **top of each pass**, before the balance read. 3 passes off the hot
  path = at most 3 extra `getMultipleAccounts`. On a resolve error mid-loop, break with
  `last_err` as today.
- **Bonus:** re-routes correctly if migration happens between passes; keeps cashback
  fresh per pass (defense-in-depth with Fix 1).

#### Fix 3 — bot curve-sell structural-revert recovery: 6024 + 6005 [bot-path]
Generalize the existing `2006 → RefreshCreator` machinery so the two other
**recoverable** curve reverts re-read chain state and retry instead of dying as
`ExitFailed`. **Mirror in tpsl1 + tpsl2.**
- **Files:** `tpsl_sniper_1/execution/real.rs` (classifier `798-830`, decision
  application `1058-1095`, error-code consts `742-756`) and the tpsl2 clone.
- **Recommended unified design** (one off-path RPC covers all three codes):
  1. Add a trader method returning the freshly-read curve facts, reusing
     `ensure_token_pdas` (re-reads creator + cashback; a non-migrated curve is read
     fresh, not cache-served) — e.g.
     ```rust
     // pump-trader/src/trader/query.rs — generalizes refresh_curve_creator_vault
     pub async fn refresh_curve_facts(&self, mint: &str)
         -> anyhow::Result<CurveFacts /* { creator_vault, cashback_enabled, is_migrated } */>
     ```
     (`refresh_curve_creator_vault` already does the `ensure_token_pdas` half — extend
     it / add a sibling that also returns `cashback_enabled` and `is_migrated`.)
  2. Add error-code consts: `CURVE_MISSING_USER_VOLUME_ACCUMULATOR = 6024` and
     `BONDING_CURVE_COMPLETE = 6005` (confirm the exact Anchor names against the IDLs).
  3. Extend `SellRetryDecision` with `RefreshCashback` and `RerouteMigrated` (or a
     single `RefreshCurveFacts` the application arm interprets).
  4. In `classify_sell_revert` curve branch (`used_migrated == now_migrated`,
     `!used_migrated`): map `6024 → RefreshCashback`, `6005 → RerouteMigrated`, keeping
     `2006 → RefreshCreator`, slippage → `Retry`, else `StopFeeBurn`.
  5. In the decision-application match (mirror the `RefreshCreator` arm at
     [real.rs:1071-1090](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L1071-L1090)):
     - **RefreshCashback:** call `refresh_curve_facts`; this re-reads cashback into the
       cached `pdas`, so the OR guard `is_cashback || pdas.cashback_enabled` includes the
       UVA next attempt. **For full correctness (handles toggle-OFF too), also update the
       value passed to `sell_token_once`** — update `token_cache`'s `is_cashback_enabled`
       (same field the create decoder sets) and let the loop re-read it, or thread the
       returned `cashback_enabled` into the next `sell_token_once`. Recommended: update
       `token_cache` so subsequent exits/positions are correct and the toggle is observed.
     - **RerouteMigrated:** call `refresh_curve_facts`; if now migrated, update
       `token_cache.is_migrated` (the field `now_migrated` reads at
       [real.rs:1055-1056](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L1055-L1056))
       so the next attempt's venue selection routes to the AMM; on failure → `Failed`.
  6. Unit-test `classify_sell_revert` for the new codes (extend the existing
     `#[cfg(test)]` block).
- **Minimal alternative:** treat `6024` exactly like `2006` (reuse
  `RefreshCreator`/`refresh_curve_creator_vault`, which also refreshes
  `pdas.cashback_enabled`) — covers the common missing-UVA / toggle-ON case via the OR
  guard, but **not** toggle-OFF, and does nothing for 6005. The unified design is
  preferred for "all cases."

#### Fix 4 — buy-path cashback hardening (Fix 2 from the 6024 plan) [manual-path]
- **File:** [buy.rs:163](pump-trader/src/trader/buy.rs#L163) —
  `derive_token_pdas(mint, creator, &tp, false)` hardcodes `cashback_enabled=false`.
- **Problem:** the only `derive_token_pdas` caller that lies (query.rs:522/544 pass the
  real `routing.cashback_enabled`). Harmless today only because every curve-sell caller
  passes the flag explicitly; a latent landmine for any future path reading
  `pdas.cashback_enabled` alone.
- **Change:** thread the true flag into the buy path so the cached PDAs are
  self-consistent:
  - add a `cashback_enabled: bool` param to `buy_token_inner` (and the public
    `buy_token` / `buy_token_snipe[_write_ahead]`), pass it to `derive_token_pdas`.
  - **Manual buy** already has it: pass `routing.cashback_enabled` from
    [solana.rs](backend/src/api/handlers/trading/solana.rs).
  - **Snipe buy** has it in `token_cache` (`token.is_cashback_enabled`) at the call site
    in `tpsl_sniper_*/execution/real.rs` — thread it through.
  - The curve buy ix itself doesn't change (it always includes the accumulators); this
    only fixes the cached `pdas` so a later sell relying on `pdas.cashback_enabled` is
    correct even if the caller flag is wrong.

### P1 — robustness

#### Fix 5 — manual-buy slippage-revert retry [manual-buy]
- **File:** `manual_buy` [solana.rs:115-141](backend/src/api/handlers/trading/solana.rs#L115-L141)
- **Problem:** single-shot; a transient 6003/6004 → 500, user must re-click.
- **Change:** wrap the buy in a bounded retry (2-3 attempts). **Only retry on a proven
  on-chain revert** (no tokens bought) — mirror `classify_silent_send`
  ([real.rs:42-48](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L42-L48)):
  `Some(false)` → resend; `Some(true)`/`None`/`Err` → stop (durable-nonce tx may land,
  re-sending risks a double-buy). Requires the buy to return its **signature** (today
  `bool`) — see Fix 5b. `buy_token_inner` already re-reads curve reserves each call on
  the manual path, so a retry re-quotes automatically.

#### Fix 5b — manual-buy confirm-timeout classification [manual-buy]
- **Files:** `manual_buy`; `buy_token`/`amm_buy` return types; `confirm_transaction` caller.
- **Problem (C7/E5):** a timeout returns 500 though the durable-nonce buy may land →
  untracked tokens.
- **Change:** have the curve/AMM buy return the **signature** (like `sell_token_once →
  Option<String>`). On a `confirm_transaction` timeout, call `signature_state(&sig)`:
  - landed-success → `200 {"success": true, "pending": false}`;
  - pending → `200/202 {"pending": true, "signature": sig}` so the UI shows "submitted,
    confirming" instead of a hard failure, and (optionally) kick a wallet reconcile so a
    late-landing manual buy gets tracked;
  - reverted → 500 (or feed into Fix 5's retry).
- **Note:** plumbing the signature through `buy_token`/`amm_buy` is the bulk of this and
  also unlocks Fix 5. Keep `amm_buy`'s existing `confirm` flag semantics.

#### Fix 6 — `resolve_buy_routing` small retry [manual-path]
- **File:** the `resolve_buy_routing` calls in `manual_buy`/`manual_sell`
  ([solana.rs:99-106, 170-177](backend/src/api/handlers/trading/solana.rs#L99-L106)).
- **Change:** a 2-attempt retry with a short backoff before returning the 400, so a
  single flaky `getMultipleAccounts` doesn't fail an entire manual trade. Off the hot
  path; bounded.

### P2 — operational / latent / docs

#### Fix 7 — program-constant rot + nonce monitoring [operational]
- **Constant rot (A2/A6):** add a metric/alert that fires when the **rate** of `6000`
  (curve) or AMM structural/`Overflow` reverts **across many distinct mints** spikes —
  the signature of pump rotating a fee recipient or changing the AMM layout (memory
  `fee-recipient-rotation-bug`). Document the update runbook: verify the new account
  against a live swap + a zero-SOL `simulate-*` probe before shipping the constant.
  (No automatic on-chain discovery — keep it a guarded manual update.)
- **Nonce (C1-C3):** surface `nonce_wait_events` / the "All nonce slots busy" bail and
  the `check_nonce_authorities` result as metrics/log alerts; a frequent busy-bail means
  the pool needs resizing (respect the EC2 connection-count guardrail).

#### Fix 8 — slippage doc + dead constant [operational]
- Remove the dead `AMM_DEFAULT_SLIPPAGE_BPS` ([constants.rs:238](pump-trader/src/constants.rs#L238))
  **or** wire it into the AMM builders' `None` arm if a default AMM-buy floor is actually
  wanted (manual buys already pass `Some(500)`, so wiring is largely moot).
- Correct [@plans/trade-execution/slippage-logic-buy-sell.md](@plans/trade-execution/slippage-logic-buy-sell.md):
  AMM `None` = `min_out 1` (no floor), same as the curve — it does **not** apply a 5%
  default. Note that bot/manual **sells** intentionally pass `None` (clear at any price)
  via `resolve_sell_slippage_bps` ([tuning.rs:53-58](backend/src/config/constants/tuning.rs#L53-L58)).

#### Optional refinements (low value, note only)
- Manual sell returns `200` if any pass sold, even with a nonzero remainder; consider
  returning the leftover balance so the UI knows it didn't fully clear.
- Manual sell collapses landed-revert vs never-landed into a string; the distinction
  exists in `sell_token` (`OnChainRevert`) but isn't surfaced to the API.

---

## Out of scope / accept-and-monitor
- A7 CU-exhaustion outliers (deliberate cost tradeoff).
- B2 first-AMM-trade marker gap (inherent to a brand-new pool with no swaps).
- The **ingest-side root cause** of B3/B4/A4-bot (missed create/migration events) —
  tracked by memory `missed-tokens-restart-replay-gap`. Fix 3's reactive chain re-read
  mitigates the *trade* symptom; durable-slot checkpointing / missed-create backfill is a
  separate ingest workstream.

---

## Verification

Per-fix, before/after:
1. **Build/typecheck:** `cargo check -p backend` and `cargo check -p pump-trader` (use
   `--target-dir target-check` if `backend.exe` is running). Clippy on touched files.
2. **Unit tests:** `cargo test --bin backend` (extend the `classify_sell_revert` tests
   for 6024/6005); `cargo test -p pump-trader` (the curve/AMM tx-size tests must still
   pass after any account-list change in Fix 3/4).
3. **6024 (Fix 1/4):** the probe path —
   `cargo run -p backend -- probe simulate-sell <mint>` on a **cashback curve token**
   must pass (17-account sell incl. `user_volume_accumulator` at slot 14).
   `simulate_curve_sell` forces a fresh `ensure_token_pdas`, so a passing sim proves the
   live build is correct. Reproduce the original failing manual sell of
   `7feWADwudSfNyfw9yek834EL5KpKKkPdi9NN74P2pump` after a same-session manual buy.
4. **Bot recovery (Fix 3):** unit-test the classifier maps 6024→RefreshCashback and
   6005→RerouteMigrated; integration-check against a token observed to have toggled
   cashback / a token whose migration event was dropped (or simulate by stale-seeding
   `token_cache.is_migrated=false` on an already-migrated mint and confirming the exit
   re-routes to AMM instead of `ExitFailed`).
5. **Manual mid-loop migration (Fix 2):** force a migrated token through `manual_sell`
   with a stale cache and confirm it routes AMM (no 6005 loop).
6. **Manual buy (Fix 5/5b):** simulate a slippage revert (tight slippage on a moving
   token) → bounded retry; simulate a confirm timeout → `200 {pending:true, signature}`
   not a 500; confirm no double-buy on a landed-but-unconfirmed tx.
7. **Frontend:** if Fix 5b changes the buy/sell response shape, `npm run build` clean and
   update the manual-trade UI to handle the `pending` status.
8. **Docs (DoD):** update `@plans/trade-execution/slippage-logic-buy-sell.md` (Fix 8), and
   add an `@plans/trade-execution/` note for the generalized curve-fact refresh (Fix 3).
   Mirror all tpsl1 edits into tpsl2 and re-grep to confirm parity.
