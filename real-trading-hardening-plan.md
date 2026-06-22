# Real-Trading Hardening Plan (single wallet)

> Goal: make the bot **safe to run with real SOL** on a single wallet.
> Built from an audit of the real (on-chain) execution path vs. the paper path.
> **Scope:** one wallet only — multi-wallet support was removed (see "Out of scope").

---

## Decisions already made (with you)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Where the private key lives | In `.env` only — **never** in code, the database, or the UI. |
| 2 | Two positions on the same token | **Allowed** (e.g. `tpsl_sniper_1` and `tpsl_sniper_2` both fire on it). This forces *per-signature attribution* — each position tracks its own buy/sell transaction — not a `(wallet, mint)` shortcut. |
| 3 | Slippage config | Wire the existing global `trade.slippage_bps` setting into strategy buy+sell **now**; per-rule override later. |

## Ground rules (from CLAUDE.md)

- **Every change lands in BOTH `tpsl_sniper_1` and `tpsl_sniper_2`** — they are intentional clones.
- No new RPC call in the sell-confirm loop; read state from `runtime_cache`, not the DB per event.
- Update the matching `@docs/*.md` map after each logic change.
- Done = `cargo check --bin backend` + `cargo test --bin backend` + `cargo test -p pump-trader` clean, `cargo clippy` on touched code, `npm run build` clean.

---

# Hardening work

*(ship all of this before putting real SOL in)*

## 1A. Reliability fixes

**The core problem:** the exit (sell) runs in a fire-and-forget `tokio::spawn` task, and the "I'm exiting this position" guard (`end_exit`) is released only *after* the sell finishes (`service.rs`, after the `sell_and_close_position(...).await`). If the task panics mid-sell:

- a panicking spawned task **does not crash the process** — it dies silently, so there is no restart to recover from;
- the guard `end_exit` line never runs → the position's `exiting` slot stays locked forever;
- the position is left `ExitPending` (no longer `Holding`), so strategy eval will never re-trigger its exit;
- the bag is stranded with **nothing in-process** that will ever re-drive it.

So three distinct fixes are needed — guard release, in-process re-drive, and boot re-drive — they do not substitute for each other.

| Fix | What | Files |
|-----|------|-------|
| **1A-1 RAII guard release** | Make the `exiting` guard free itself automatically (an RAII `Drop` guard returned by `try_begin_exit`), so a panic/early-return can't wedge the `exiting` slot permanently. **Note:** this only frees the *slot* — it does **not** re-drive the sell. A panicked exit leaves the position `ExitPending`; 1A-4 (in-process) and 1A-2 (boot) are what actually re-attempt it. | `runtime_cache.rs` (`exiting` set, `try_begin_exit`/`end_exit`), `service.rs` (`trigger_real_exit`) — both clones |
| **1A-2 ExitPending recovery on boot** | On startup, load positions stuck in `ExitPending` (today only `Holding` is loaded) and re-drive their sell. Covers the **full-process-crash + restart** case (Docker auto-restarts). Does **not** cover a single spawned-task panic, since that doesn't restart the process — see 1A-4. | `runtime_cache.rs` (`load_from_db`), position repo (`find_all_exit_pending`), `service.rs` (re-arm on boot) — both clones |
| **1A-4 In-process ExitPending reaper** | A periodic sweep (off the hot path, e.g. on the existing maintenance/poll cadence) that finds `ExitPending` positions **whose `exiting` guard is not currently held** and re-arms their exit. This is what recovers a position after a *task* panic (no restart). Guard-not-held is the safety interlock that prevents double-driving an exit that's still in flight. | `runtime_cache.rs` (query held-vs-pending), `service.rs` (re-arm), `main.rs` or maintenance task (cadence) — both clones |
| **1A-3 Sell-revert reclassify by error code** | [detail below — distinguish slippage-floor reverts from structural reverts using the on-chain program error, not a `min_out=1` assumption] | `execution/real.rs` (`classify_sell_revert` + caller) — both clones |

### 1A-3 detail — classify a sell revert by its on-chain error, not by `min_out`

**Current code:** `classify_sell_revert(state, used_migrated, now_migrated)` returns `StopFeeBurn` on `Ok(Some(false))` (tx landed *and* reverted) whenever the route is stable (`used_migrated == now_migrated`), else `Retry`. The comment justifies this with "TPSL drives min_out=1 → a revert is structural." That assumption is **already false for AMM** (`None` → 5% default floor) and becomes false for curve too once 1B passes `Some(slippage_bps)`.

**The real problem:** a landed-and-reverted sell collapses two *different* causes into the same `Some(false)` signal:

- **Slippage-floor rejection** — price moved past `min_out` between build and land. **Retryable** (re-quote and resend).
- **Structural revert** — already-sold / empty ATA / account-not-initialized / wrong venue after migration. **Not retryable** (stop, don't burn fees).

You cannot tell these apart from the landed bool alone — you must read the **program error** out of the failed tx.

**Fix:**

1. Thread the revert reason through. Replace the bare landed-bool with a small classification the sell attempt extracts from the failed tx's program error / custom error code (curve `SlippageExceeded`/`TooLittleSolReceived`, AMM `ExceededSlippage` → `Slippage`; account/balance/uninitialized → `Structural`; anything else → `Unknown`).
2. `classify_sell_revert` maps: `Slippage → Retry`, `Structural → StopFeeBurn`, `Unknown` on a stable route → `StopFeeBurn` (stay conservative on fee burn) **and log the raw code** so unmapped reasons surface.
3. Drop the `min_out=1` premise from the comment; the decision is now venue-agnostic and error-code-driven, matching the fact that both venues now carry real slippage protection.

## 1B. Wire slippage into strategy trades

**Problem:** strategy buy/sell currently pass `None` for slippage → curve trades accept *any* price (`min_out=1`, zero protection); only AMM gets the 5% default. The `trade.slippage_bps` setting you already have is only used by the **manual** buy/sell buttons.

- Resolve slippage from `app_state.settings().slippage_bps` (fallback `DEFAULT_SLIPPAGE_BPS = 500`), reusing the existing `resolve_slippage` logic in `api/handlers/trading/solana.rs` (extract a shared helper).
- **Sell side:** pass `Some(slippage_bps)` into `sell_token_once(...)` / `amm_sell(...)` — clear win, do this.
- **Buy side:** pass `Some(slippage_bps)` into `buy_token_snipe(...)`, computing `min_out` from the price the **triggering trade event already carries** — no chain, no inline network call.
  - Curve-buy `min_out` needs current price (`virtual_sol / virtual_token`). On the snipe path that price is **already in hand**: the trade that fired the entry signal carries `virtual_sol_reserves` / `virtual_token_reserves` (`models/trade.rs`, populated by the decoder at ingest). Thread those straight into the buy.
  - ⚠️ **Do not** call `curve_reserves` / `curve_virtual_reserves` (RPC) inline before tx build — that's a network round-trip on the hot path, a sell-confirm-class budget violation. The old multi-tier chain (`reserve_cache → genesis → RPC`) is **wrong for this path**: step 1 always hits for snipe-on-trade, so the genesis tier is unreachable (and isn't wired into `curve_reserves` today anyway) and the RPC tier is exactly the latency we're avoiding.
  - **Single fallback:** if (and only if) the event somehow lacks reserves — structurally near-impossible for snipe-on-trade — fall back to `min_out=1` (no protection). `min_out` is optional slippage protection ("buy, but abort if the fill is worse than X"), **never** required for the buy to function, so a missing read must never block or delay the snipe.
  - Scope: this supersedes the cache→RPC chain in `curve_reserves` **only for the strategy snipe buy**. That existing chain stays for any non-snipe caller that has no triggering event.
- No DB change — uses the existing setting; the Settings page already edits it.

## 1C. Per-signature attribution *(enables concurrent same-token positions safely)*

**Problem (in plain terms):** with one wallet, both the entry and the exit are recovered from the shared `trades` feed keyed only by *(wallet, mint)*:

- **Entry** — `adopt_existing_fill_if_present` reads the buy via `trade_repo.find_latest_by_wallet_mint_type(wallet, mint, Buy)` — the *latest* buy for that pair (the buy's own returned signature is currently discarded for entry-recording).
- **Exit** — `sell_until_balance_cleared` confirms by polling the *net* token balance (`Σ buys − Σ sells` for that wallet+mint).

If two positions hold the same token at once (decision #2 allows it — e.g. both strategy clones fire on the same mint), both adopt the *same* latest buy, and the shared net balance can't tell their sells apart → double-counted entries, a position falsely seen as "sold", wrong PnL. On a real-money path this means a double-sell or a stranded bag.

**Fix:** attribute each fill by the **transaction signature** the bot already gets back from its own trade (`buy_until_filled_or_give_up` and each sell attempt already return their `sig`).

**Two kinds of "tx field" — keep them straight:**

- **Signatures** (`entry_tx`, `exit_tx`) — *which* tx(s) made the fill. This is the part that can be **multi-leg** (the exit already is; the entry will be once we scale into a position). A single `TEXT` cannot hold several legs → these become **JSONB arrays**, and the old single columns are dropped (keeping both would be redundant; an array of length 1 covers today's single-leg case).
- **Summaries** (`entry_price`, `entry_token_amount`, `entry_time` + exit equivalents) — the rolled-up *result* of the fill. **Kept as-is.** With multi-leg they become roll-ups (weighted-avg price, summed tokens, first/last time). Not redundant with the arrays — different data.
- **`target_tx` stays a single `TEXT`** — it's someone *else's* trigger trade, inherently one tx, never the bot's multi-leg fill.

1. **Entry** — drop `entry_tx TEXT`, add `entry_tx_signatures JSONB`. Today the bug is *which* fill is adopted: thread the buy's own returned signature into `poll_feed_until_entry_fill` and read the fill with a new `trade_repo.find_fill_by_signature(wallet, mint, sig)` (sums *that signature's* legs) instead of `find_latest_by_wallet_mint_type`; store the sig in the array. Single-leg today, but the array means multi-leg entry later needs **no second migration**.
   - ⚠️ **Index required:** `find_fill_by_signature` and the per-signature sell-confirm both filter `trades` by `(wallet, mint, tx_signature)`. `trades` is one of the large, continuously-growing partitioned tables — without a supporting index this is a seq scan on the **entry/exit-confirm hot path** (data-scale guardrail violation). The 1C migration must add an index on `trades (wallet, mint, tx_signature)` (compatible with the partitioning scheme).
2. **Exit** — drop `exit_tx TEXT`, add `exit_tx_signatures JSONB`. Today only the *last* sell leg is stored (`position.close(last_sell.tx_signature, …)`) and confirmation uses the shared net balance. Record **all** of this position's own sell signatures in the array, and confirm the exit by summing *those* signatures' token legs against the position's `entry_token_amount` — so concurrent positions never confirm against each other's sells. Keep the existing "poll the full window before retry" buffering.
3. **Migration** — on the **four** position tables (2 real + 2 paper — the `Position` struct is shared, so columns must exist on all of them even though only the real path populates them): drop `entry_tx` + `exit_tx`, add `entry_tx_signatures JSONB NOT NULL DEFAULT '[]'` + `exit_tx_signatures JSONB NOT NULL DEFAULT '[]'`. Legacy rows: backfill the old single value into a 1-element array; empty `[]` falls back to the net-balance confirm (recovery only). **Tradeoff — keep a uniqueness backstop:** moving to a JSONB array drops the DB-level `entry_tx` `NOT NULL UNIQUE` guard. Don't replace it with "code enforces it" alone — this is a real-money path and the constraint was the last line against double-recording the same buy. Restore an equivalent backstop: a **unique expression index** on the adopted buy signature (e.g. `UNIQUE` on `entry_tx_signatures->>0` for the single-leg case, or a normalized side index over the array), **or** an explicit app-level dedup (reject adopting a signature already attributed to an open position) before insert. The in-code attribution argument (two concurrent positions = two distinct buy txs) explains why correctness *should* hold; the backstop is what catches it when it doesn't.

**Files:** `storage/repositories/trade_repo.rs` (`find_fill_by_signature` + per-signature leg sum), position repos (read/write the two arrays; confirm-by-sig), `models/position.rs` (`entry_tx_signatures: Vec<String>` + `exit_tx_signatures: Vec<String>`, replacing `entry_tx`/`exit_tx`), `execution/real.rs` (thread the buy sig into entry; accumulate sell sigs; sum-by-sig confirm — both clones), frontend (positions table reads the arrays, shows first/last leg), new migration.

---

## How we'll verify

**Gate before real SOL:**

- Builds + tests + clippy clean.
- Unit tests: RAII guard frees the `exiting` slot on panic; in-process reaper (1A-4) re-arms an `ExitPending` position whose guard isn't held; ExitPending re-arms on boot; `find_fill_by_signature` sums multi-leg; per-signature sell-confirm; buy `min_out` derived from the triggering event's reserves, `min_out=1` on missing reserves; `classify_sell_revert` maps slippage error codes → Retry and structural codes → StopFeeBurn.
- `cargo run -p backend -- probe simulate-sell` with a real slippage value (confirm slippage-vs-structural revert classification fires correctly).
- Live test on a funded throwaway wallet at **0.01 SOL**: single buy→exit, then **two concurrent rules on the same token** — confirm both attribute and exit independently (the heart of decision #2).

---

## Resolved decisions

1. **Big buys + slippage:** **fail-safe** — when a buy can't fill within slippage, **reject** it (don't fill at a bad price).

## Deferred (do later, no rework needed)

- **SOL balance-floor guard — don't spend the wallet to zero.** A pre-buy gate so a bad run (or an `ExitPending` pile-up tying up SOL) can't drain the wallet below the SOL it needs for fees/tips/rent — which would brick its own ability to *sell*. Add `get_sol_balance(wallet)` to `pump-trader/query.rs`, refreshed **off the hot path** (never inline before a buy) and cached; keep a live **committed-SOL counter** (Σ `buy_amount` of open real positions) in **shared state** (`AppState`/`PumpFunTrader`), read by **both** clones — not per-strategy `runtime_cache`, or each would see only half the commitments and both could pass on the same SOL. Pre-buy: require `cached_balance − reserve_floor − committed_sol ≥ buy_amount`, reserve floor = **0.02 SOL**; reject the buy if it fails. No DB table for exposure (it drifts).
- **`trade.max_committed_sol` — explicit exposure ceiling.** A hard cap on total SOL tied up in open positions, on top of the balance-floor guard above. To add: (1) add the key to the settings registry (`settings_repo.rs`, same pattern as `slippage_bps`); (2) extend the balance-floor pre-buy gate to also require `committed_sol + buy_amount ≤ max_committed_sol`; (3) add the field to the Settings page. Builds on the committed-SOL counter from the balance-floor guard, so it's a ~one-check + one-setting add-on once that lands.

## Out of scope

- **Multi-wallet support — removed.** One wallet only. The single `Arc<PumpFunTrader>` and the existing `WALLET_PRIVATE_KEY` / `NONCE_ACCOUNTS` `.env` config stay as-is; no trader registry, no `trading_wallets` table, no per-rule `wallet_id`, no Wallets page. Revisit only if the single-wallet path is proven safe and the need actually arises.
