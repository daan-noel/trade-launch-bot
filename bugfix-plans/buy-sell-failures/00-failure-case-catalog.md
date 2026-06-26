# Reference — Buy/Sell failure-case catalog (all cases, with current status)

> Workstream B (buy-sell-failures). **Reference doc, not a fix.** It's the grounding for every
> `0N-*.md` fix in this folder. No code is changed by this document.
> Paths are pre-crate-split — see [../README.md](../README.md#-path-caveat--line-refs-predate-the-crate-split).

## Context

The bot trades pump.fun tokens across two venues (bonding **curve** pre-migration, PumpSwap **AMM**
post-migration) on two driver paths (**bot** = tpsl snipers, **manual** = API handlers). A known
example is `Custom(6024)` (missing `user_volume_accumulator` on a cashback curve sell); a separate
creator-vault staleness bug (`2006`) was already fixed.

## Flow map (grounding)

| | Curve (pre-migration) | AMM (post-migration) |
|---|---|---|
| Bot buy | `buy_token_snipe[_write_ahead]` → `buy_token_inner` ([buy.rs](../../pump-trader/src/trader/buy.rs)) | n/a (snipes are always fresh creates) |
| Bot sell | `sell_token_once` (`confirm=false`, feed-confirmed) | `amm_sell` (`confirm=false`) |
| Manual buy | `buy_token` ([solana.rs:122-134](../../backend/src/api/handlers/trading/solana.rs#L122-L134)) | `amm_buy` |
| Manual sell | `sell_token` ([solana.rs:232-244](../../backend/src/api/handlers/trading/solana.rs#L232-L244)) | `amm_sell` |

- **Venue selection** = `is_migrated`. Manual: `routing.is_migrated` from a **live**
  `resolve_buy_routing`. Bot: `token_cache.is_migrated` (WS-fed, re-read each attempt).
- **Cashback** gates only the **curve sell** UVA account (slot 14), via
  `if is_cashback || pdas.cashback_enabled` ([sell.rs:450](../../pump-trader/src/trader/sell.rs#L450)).
  Curve **buy** pushes the accumulators unconditionally; **AMM** reads cashback from the pool
  on-chain. So 6024 is a **curve-sell-only** failure mode.
- **Bot sell retry/classifier**: `sell_until_balance_cleared` + `classify_sell_revert`
  ([real.rs:798-830, 1058-1095](../../backend/src/strategies/tpsl_sniper_1/execution/real.rs#L798-L830)),
  `SELL_MAX_ATTEMPTS = 6`. **`tpsl_sniper_2` is an intentional clone — mirror every edit**
  (classifier at tpsl2 `real.rs:943-975`, RefreshCreator arm ~`1216-1231`).
- Key reuse: `refresh_curve_creator_vault` ([query.rs:564-570](../../pump-trader/src/trader/query.rs#L564-L570))
  calls `ensure_token_pdas`, which re-reads **creator AND cashback** from chain into the cached
  `pdas` — so bot cashback/migration recovery can reuse this machinery.

## Failure-case catalog

Status: ✅ handled · ⚠️ partial/conditional · ❌ broken/unhandled · 🕒 latent (breaks on an external change)

### Quick matrix — the two flags

**`is_cashback`** (curve-sell UVA only):

| Path | Source | Robust? |
|---|---|---|
| Curve **buy** (bot + manual) | n/a — accumulators pushed unconditionally | ✅ can never 6024 |
| Curve **sell — bot** | `token_cache.is_cashback_enabled` (set at create) | ⚠️ stale if toggled (B3) |
| Curve **sell — manual** | `token_cache` ❌ (should be `routing.cashback_enabled`) | ❌ 6024 bug → [01](01-manual-sell-6024-cashback.md) |
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
  - Manual sell: reads `is_cashback` from `token_cache` ([solana.rs:182-186](../../backend/src/api/handlers/trading/solana.rs#L182-L186)),
    not the `routing` already fetched at line 170. Bites when the mint's `TokenPDAs` are already
    cached with the hardcoded `false`, so the sell skips its own `ensure_token_pdas` re-read and
    both OR operands are `false`. → **[Fix 01](01-manual-sell-6024-cashback.md)**.
  - Bot sell: `is_cashback` from `token_cache` (set at create, never refreshed). A 6024 here →
    `classify_sell_revert` falls to `StopFeeBurn` → `ExitFailed`, **no recovery**.
    → **[Fix 04](04-bot-curve-sell-revert-recovery.md)**.
- **A2 `Custom(6000)` NotAuthorized — wrong curve fee recipient** ✅ / 🕒
  - Fixed via hardcoded `PUMP_CURVE_FEE_RECIPIENT` ([constants.rs:56](../../pump-trader/src/constants.rs#L56)).
    The next rotation breaks **every** curve trade until updated. Same risk for
    `PUMP_AMM_BUYBACK_FEE_RECIPIENT` / `PUMP_AMM_CASHBACK_GLOBAL`.
    → **[Fix 07 (monitoring)](07-constant-rot-nonce-monitoring.md)**.
- **A3 `ConstraintSeeds(2006)` — stale creator_vault (curve sell)** ✅ / ⚠️
  - Bot: classified → `refresh_curve_creator_vault` → retry (both clones). Manual: protected
    because it passes `Some(&routing.creator)` (live) and `execute_sell` recomputes the vault
    ([sell.rs:302-315](../../pump-trader/src/trader/sell.rs#L302-L315)). Residual: a creator rotated
    more times than `SELL_MAX_ATTEMPTS` → `ExitFailed`.
- **A4 `BondingCurveComplete(6005)` — curve trade after migration** ⚠️/❌
  - Manual sell: routing resolved **once before** the clear loop ([solana.rs:170](../../backend/src/api/handlers/trading/solana.rs#L170));
    the loop re-reads balance but not routing → a token migrating **mid-loop** routes every pass
    to the curve → repeated 6005 → 500. → **[Fix 03](03-manual-sell-reresolve-routing.md)**.
  - Bot: a 6005 where `token_cache.is_migrated` is still `false` is **not** special-cased →
    `StopFeeBurn` → `ExitFailed` (the "missed migration event" case, memory
    `missed-tokens-restart-replay-gap`). → **[Fix 04](04-bot-curve-sell-revert-recovery.md)**.
- **A5 slippage reverts `6003` (curve) / `6004` (AMM)** ⚠️
  - Sell (bot + `sell_token`): ✅ retried with a fresh-reserve re-quote. **Manual buy: ❌
    single-shot**, a transient slippage revert → 500, no retry.
    → **[Fix 05](05-manual-buy-slippage-and-confirm.md)**.
- **A6 AMM `Overflow` / bad account list** 🕒 — the trailing buyback block, cashback block, and
  per-coin fee-share marker are reverse-engineered & hardcoded
  ([amm.rs:468-512](../../pump-trader/src/trader/amm.rs#L468-L512)). A pump_amm layout upgrade
  reverts **every** AMM swap. → **[Fix 07 (monitoring)](07-constant-rot-nonce-monitoring.md)**.
- **A7 compute-unit exhaustion** ⚠️ accepted — outlier heavy txs exceed `COMPUTE_UNIT_LIMIT_*`
  and revert while paying the fee ([constants.rs:170-187](../../pump-trader/src/constants.rs#L170-L187)).
  Deliberate (~1-in-15). Leave as-is; mention in monitoring.
- **A8 `close_account` reverts on dust** ✅ — preflight on, no tip, fails cheaply.

### B. Pre-send resolution / routing

- **B1 `resolve_buy_routing` RPC failure (manual buy + sell)** ⚠️ — flaky `getMultipleAccounts`
  → 400, **no retry**. Every manual trade gates on it. → **[Fix 06](06-resolve-routing-retry.md)**.
- **B2 AMM pool/config/marker resolution** ⚠️ — pool missing → bail
  ([amm.rs:556-560](../../pump-trader/src/trader/amm.rs#L556)); **first seller of a just-migrated
  non-cashback token can't sell** until someone else trades it (fee-share marker has no source,
  [amm.rs:574-582](../../pump-trader/src/trader/amm.rs#L574-L582)). → mostly accept; surface a
  clearer error.
- **B3 cashback mutability (`toggle_cashback_enabled`)** ❌ — cashback is **mutable on-chain** but
  the bot **never observes or re-reads it**: set at create
  ([decoder/create.rs:110-113](../../backend/src/ingest_laserstream/decoder/create.rs#L110-L113)),
  no ingest handler, DB never `UPDATE`d, `token_cache` value immutable. A creator toggling cashback
  after the snipe makes the bot's flag stale → 6024 on the curve sell.
  → **[Fix 04](04-bot-curve-sell-revert-recovery.md)** covers it.
- **B4 bot `token_cache` cold start** ⚠️ — a miss defaults `(is_cashback=false, is_migrated=false)`;
  a position acted on before DB-seed completes (or whose create was never ingested) can mis-route.
  Related to memory `missed-tokens-restart-replay-gap`.

### C. Execution environment

- **C1 nonce starvation under volume** ⚠️ — `acquire_nonce` bails "All nonce slots busy"
  ([nonce.rs:102](../../pump-trader/src/trader/nonce.rs#L102)). Pool size fixed.
  → **[Fix 07](07-constant-rot-nonce-monitoring.md)**.
- **C2 nonce slot with no cached hash** ⚠️ — after an all-reads-failed refresh the slot's hash is
  cleared and isn't handed out until refreshed (compounds C1).
- **C3 nonce authority misconfigured** 🕒 — authority ≠ wallet → every durable-nonce tx on that slot
  fails silently; `check_nonce_authorities` is a manual audit only.
- **C4 re-arming a consumed nonce hash** ✅ — explicitly prevented ([nonce.rs:117-158](../../pump-trader/src/trader/nonce.rs#L117-L158)).
- **C5 recent-blockhash expiry (AMM buy)** ✅/⚠️ — 10 s freshness bound; a stalled refresher past
  validity drops the AMM buy.
- **C6 Jito auction loss / never lands** ✅ (sell retried) / ⚠️ (buy single-shot, covered by
  write-ahead recovery — see `@plans/trade-execution/buy-in-flight-recovery.md`).
- **C7 `confirm_transaction` false timeout (manual)** ⚠️ — ~2.35 s window
  ([constants.rs:193-203](../../pump-trader/src/constants.rs#L193-L203)). Manual buy times out →
  500, but the durable-nonce buy can land later → **untracked** balance.
  → **[Fix 05](05-manual-buy-slippage-and-confirm.md)**.
- **C8 sender fan-out total failure** ✅ — errors only if every endpoint fails.

### D. Account / balance / input

- **D1 no token account on sell** ✅ — bails. **D2 garbage `sol_amount`** ✅ — `buy_lamports_checked`
  + `MAX_BUY_SOL`. **D3 slippage range** ✅ — clamped `[10,5000]`. **D4 wrong token program** ✅ —
  sourced live from mint owner.
- **D5 `slippage None` = `min_out=1`** ⚠️ value-loss (MEV), not a revert. The AMM builders also treat
  `None` as 1 — `AMM_DEFAULT_SLIPPAGE_BPS` is **dead code** (declared
  [constants.rs:238](../../pump-trader/src/constants.rs#L238), used only in a doc comment).
  → **[Fix 08](08-slippage-doc-dead-const.md)**.

### E. Concurrency / bookkeeping

- **E1 double-sell** ✅ (ExitGuard + per-signature attribution + full-window poll).
- **E2 double-buy** ✅ (never-resend + write-ahead marker + EntryGuard).
- **E3 position stuck `Holding` after manual sell** ⚠️ — `reconcile_externally_cleared_mint`
  retries ~12 s; reaper backstops (memory `manual-sell-holding-forever-bug`).
- **E4 `ExitFailed` parked** ⚠️ — needs manual action; if the DB write that marks it fails, the row
  stays `Holding`.
- **E5 untracked manual buy** ⚠️ — see C7 / [Fix 05](05-manual-buy-slippage-and-confirm.md).

## Out of scope / accept-and-monitor

- A7 CU-exhaustion outliers (deliberate cost tradeoff).
- B2 first-AMM-trade marker gap (inherent to a brand-new pool with no swaps).
- The **ingest-side root cause** of B3/B4/A4-bot (missed create/migration events) — tracked by
  memory `missed-tokens-restart-replay-gap` and the
  [tpsl-realtime workstream](../tpsl-realtime/00-gap-replay-mechanisms.md).
  [Fix 04](04-bot-curve-sell-revert-recovery.md)'s reactive chain re-read mitigates the *trade*
  symptom; durable-slot checkpointing / missed-create backfill is a separate ingest workstream.

## Verification (shared across fixes)

1. **Build:** `cargo check -p backend-deploy` + `cargo check -p backend-core` + `cargo check -p
   pump-trader` (use `--target-dir target-check` if a bin `.exe` is running). Clippy on touched files.
2. **Unit tests:** `cargo test -p backend-deploy` (extend `classify_sell_revert` for 6024/6005);
   `cargo test -p pump-trader` (curve/AMM tx-size tests must still pass after any account-list change).
3. Per-fix verification lives in each `0N-*.md`.
