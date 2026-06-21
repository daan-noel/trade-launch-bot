# Real-Trading Hardening + Multi-Wallet Plan

> Goal: make the bot **safe to run with real SOL first**, then scale it to **multiple wallets**.
> Built from an audit of the real (on-chain) execution path vs. the paper path.

---

## Decisions already made (with you)

| # | Topic | Decision |
|---|-------|----------|
| 1 | Where private keys live | In `.env` / a key file. The **database stores only metadata** (label, pubkey, default flag). The UI never sees secret keys. |
| 2 | Order of work | **Phase 1 = safety on the single wallet (ship before real trading).** Phase 2 = multi-wallet + SOL management. |
| 3 | Two positions on the same token | **Allowed.** This forces *per-signature attribution* (each position tracks its own buy/sell transaction), not a shortcut. |
| 4 | Slippage config | Wire the existing global `trade.slippage_bps` setting into strategy buy+sell **now**; per-rule override later. |

## Ground rules (from CLAUDE.md)

- **Every change lands in BOTH `tpsl_sniper_1` and `tpsl_sniper_2`** — they are intentional clones.
- No new RPC call in the sell-confirm loop; read state from `runtime_cache`, not the DB per event.
- Update the matching `@docs/*.md` map after each logic change.
- Done = `cargo check --bin backend` + `cargo test --bin backend` + `cargo test -p pump-trader` clean, `cargo clippy` on touched code, `npm run build` clean.

---

# PHASE 1 — Safety on the single wallet
*(ship this before putting real SOL in)*

## 1A. Reliability fixes

**The core problem:** the exit (sell) runs in a fire-and-forget background task, and the "I'm exiting this position" guard is released *after* the sell finishes. If the task panics mid-sell, the position's bag is stranded **and** the guard stays locked forever — and a restart (Docker auto-restarts) does not recover it.

| Fix | What | Files |
|-----|------|-------|
| **1A-1 RAII guard release** | Make the `exiting` guard free itself automatically (on a `Drop` guard), so a panic/early-return can't wedge a position permanently. | `runtime_cache.rs` (`exiting` set, `try_begin_exit`/`end_exit`), `service.rs` (`trigger_real_exit`) — both clones |
| **1A-2 ExitPending recovery on boot** | On startup, load positions stuck in `ExitPending` (today only `Holding` is loaded) and re-drive their sell. Critical because Docker auto-restarts after a crash. | `runtime_cache.rs` (`load_from_db`), position repo (`find_all_exit_pending`), `service.rs` (re-arm on boot) — both clones |
| **1A-3 AMM revert reclassify** | A code comment assumes every sell uses `min_out=1`, so "revert = structural, don't retry". That's wrong for AMM (it has a real 5% slippage floor) and will be wrong for curve too once 1B adds slippage. Treat a slippage-floor rejection as **retryable**, distinct from a structural revert. | `execution/real.rs` (`classify_sell_revert`) — both clones |

## 1B. Wire slippage into strategy trades

**Problem:** strategy buy/sell currently pass `None` for slippage → curve trades accept *any* price (`min_out=1`, zero protection); only AMM gets the 5% default. The `trade.slippage_bps` setting you already have is only used by the **manual** buy/sell buttons.

- Resolve slippage from `app_state.settings().slippage_bps` (fallback `DEFAULT_SLIPPAGE_BPS = 500`), reusing the existing `resolve_slippage` logic in `api/handlers/trading/solana.rs` (extract a shared helper).
- **Sell side:** pass `Some(slippage_bps)` into `sell_token_once(...)` / `amm_sell(...)` — clear win, do this.
- **Buy side:** pass `Some(slippage_bps)` into `buy_token_snipe(...)`.
  - ⚠️ **Latency note:** curve-buy slippage needs the current price (`virtual_sol / virtual_token`) to set a sane `min_out`. The fallback chain should be **event-reserves → `reserve_cache` → genesis constants → RPC (last resort)**, so RPC effectively never fires on a snipe:
    1. **Triggering-event reserves** — the trade that fires the entry signal already carries `virtual_token_reserves`/`virtual_sol_reserves`. Use those directly (a miss becomes structurally impossible for snipe-on-trade) rather than re-reading the side cache.
    2. **WS-fed `reserve_cache`** — fresh snapshot for the mint if step 1 isn't threaded through (`curve_reserves`, already wired).
    3. **Genesis constants** — for a true first-touch with no prior trade, reserves are deterministic: `INITIAL_VIRTUAL_TOKEN_RESERVES` / `INITIAL_VIRTUAL_SOL_RESERVES` (`config/constants/token_math.rs`).
    4. **RPC** (`curve_virtual_reserves`) — correct safety net, but it's a network round-trip *inline before tx build*; keep it as the rare last resort, never the common path.
  - Note: `min_out` is just slippage protection — "buy, but abort if the fill is worse than X" (like a limit order). It is **not** required for the buy to function; a failed/missing read falls back to `min_out=1` so slippage never blocks a buy.
- No DB change — uses the existing setting; the Settings page already edits it.

## 1C. Per-signature attribution *(enables concurrent same-token positions safely)*

**Problem (in plain terms):** with one wallet, both the entry and the exit are recovered from the shared `trades` feed keyed only by *(wallet, mint)*:

- **Entry** — `adopt_existing_fill_if_present` reads the buy via `trade_repo.find_latest_by_wallet_mint_type(wallet, mint, Buy)` — the *latest* buy for that pair (the buy's own returned signature is currently discarded for entry-recording).
- **Exit** — `sell_until_balance_cleared` confirms by polling the *net* token balance (`Σ buys − Σ sells` for that wallet+mint).

If two positions hold the same token at once, both adopt the *same* latest buy, and the shared net balance can't tell their sells apart → double-counted entries, a position falsely seen as "sold", wrong PnL.

**Fix:** attribute each fill by the **transaction signature** the bot already gets back from its own trade (`buy_until_filled_or_give_up` and each sell attempt already return their `sig`).

**Two kinds of "tx field" — keep them straight:**

- **Signatures** (`entry_tx`, `exit_tx`) — *which* tx(s) made the fill. This is the part that can be **multi-leg** (the exit already is; the entry will be once we scale into a position). A single `TEXT` cannot hold several legs → these become **JSONB arrays**, and the old single columns are dropped (keeping both would be redundant; an array of length 1 covers today's single-leg case).
- **Summaries** (`entry_price`, `entry_token_amount`, `entry_time` + exit equivalents) — the rolled-up *result* of the fill. **Kept as-is.** With multi-leg they become roll-ups (weighted-avg price, summed tokens, first/last time). Not redundant with the arrays — different data.
- **`target_tx` stays a single `TEXT`** — it's someone *else's* trigger trade, inherently one tx, never the bot's multi-leg fill.

1. **Entry** — drop `entry_tx TEXT`, add `entry_tx_signatures JSONB`. Today the bug is *which* fill is adopted: thread the buy's own returned signature into `poll_feed_until_entry_fill` and read the fill with a new `trade_repo.find_fill_by_signature(wallet, mint, sig)` (sums *that signature's* legs) instead of `find_latest_by_wallet_mint_type`; store the sig in the array. Single-leg today, but the array means multi-leg entry later needs **no second migration**.
2. **Exit** — drop `exit_tx TEXT`, add `exit_tx_signatures JSONB`. Today only the *last* sell leg is stored (`position.close(last_sell.tx_signature, …)`) and confirmation uses the shared net balance. Record **all** of this position's own sell signatures in the array, and confirm the exit by summing *those* signatures' token legs against the position's `entry_token_amount` — so concurrent positions never confirm against each other's sells. Keep the existing "poll the full window before retry" buffering.
3. **Migration** — on the **four** position tables (2 real + 2 paper — the `Position` struct is shared, so columns must exist on all of them even though only the real path populates them): drop `entry_tx` + `exit_tx`, add `entry_tx_signatures JSONB NOT NULL DEFAULT '[]'` + `exit_tx_signatures JSONB NOT NULL DEFAULT '[]'`. Legacy rows: backfill the old single value into a 1-element array; empty `[]` falls back to the net-balance confirm (recovery only). **Tradeoff:** the old `entry_tx` `NOT NULL UNIQUE` guard is gone — but it's no longer needed, since each position now adopts its *own* buy signature (two concurrent positions = two distinct buy txs), so attribution enforces uniqueness in code.

**Files:** `storage/repositories/trade_repo.rs` (`find_fill_by_signature` + per-signature leg sum), position repos (read/write the two arrays; confirm-by-sig), `models/position.rs` (`entry_tx_signatures: Vec<String>` + `exit_tx_signatures: Vec<String>`, replacing `entry_tx`/`exit_tx`), `execution/real.rs` (thread the buy sig into entry; accumulate sell sigs; sum-by-sig confirm — both clones), frontend (positions table reads the arrays, shows first/last leg), new migration.

---

# PHASE 2 — Multiple wallets + SOL management

**Key finding:** `PumpFunTrader` bakes the wallet into per-wallet state at construction — a wallet-specific AMM PDA (`mod.rs:318`), the token-account cache, and **nonce accounts are per-wallet**. So the right design is **one `PumpFunTrader` instance per wallet (a registry)** — not one trader switching keys.

| Step | What |
|------|------|
| **2A. Wallet config** | `.env` holds `WALLETS_JSON=[{label, key, nonce_accounts}]`. A `trading_wallets` DB table holds **only** `label / pubkey / is_default` (no secret). Legacy `WALLET_PRIVATE_KEY` stays as the default wallet. |
| **2B. Trader registry** | Build one `PumpFunTrader` per wallet in `main.rs`; replace the single `Arc<PumpFunTrader>` with a `TraderRegistry` (resolve by wallet id, plus a `default()`). |
| **2C. Per-rule wallet** | Add nullable `wallet_id` to the tpsl rule tables (model + migration + repos + API + frontend rule-form dropdown). NULL → default wallet. The chosen wallet flows into all the per-signature queries from 1C. |
| **2D. SOL / exposure guard** | **Don't store exposure in a table** (it drifts). Keep a live **committed-SOL counter per wallet** (Σ `buy_amount` of open real positions). Before each buy, require `free_SOL − reserve_floor ≥ buy_amount` (reserve floor = **0.02 SOL**). Add `get_sol_balance(wallet)` to `pump-trader/query.rs`, refreshed periodically (off the hot path). **Ship: balance-floor guard only.** *Deferred (later, easy add-on):* an optional hard ceiling `trade.max_committed_sol` per wallet — leave a hook so it can be wired in without refactoring (see "Deferred" below). |
| **2E. Wallets UI** | A read-only **Wallets** page: label, pubkey, live SOL balance, committed SOL, open positions. Reuse existing memo/context patterns so it doesn't re-render on every price tick. |

---

## How we'll verify

**Phase 1 (gate before real SOL):**
- Builds + tests + clippy clean.
- Unit tests: guard frees on panic; ExitPending re-arms on boot; `find_fill_by_signature` sums multi-leg; per-signature sell-confirm; slippage fallback order.
- `cargo run -p backend -- probe simulate-sell` with a real slippage value (revert reclassification).
- Live test on a funded throwaway wallet at **0.01 SOL**: single buy→exit, then **two concurrent rules on the same token** — confirm both attribute and exit independently (the heart of decision #3).

**Phase 2:**
- Registry builds N traders from `WALLETS_JSON`; per-rule `wallet_id` routes correctly (assert `wallet_pubkey()` per trade).
- Exposure guard blocks a buy when free − reserve < buy_amount; committed counter tracks open/close.
- `npm run build` clean; Wallets page stable under live ticks.

---

## Resolved decisions

1. **Reserve floor = 0.02 SOL** per wallet (kept untouched for fees/tips/rent).
2. **Default wallet:** the existing `WALLET_PRIVATE_KEY` automatically becomes the default wallet (no migration; new wallets added via `WALLETS_JSON`).
3. **Big buys + slippage:** **fail-safe** — when a buy can't fill within slippage, **reject** it (don't fill at a bad price).
4. **Exposure guard:** ship the **balance-floor guard only** for now (stop buying when a wallet runs low). The explicit per-wallet ceiling is **deferred** — see below.

## Deferred (do later, no rework needed)

- **`trade.max_committed_sol` — explicit per-wallet exposure ceiling.** A hard cap on total SOL tied up in open positions on a wallet, on top of the balance floor. To add later: (1) add the key to the settings registry (`settings_repo.rs`, same pattern as `slippage_bps`); (2) in the Phase-2 pre-buy guard, also require `committed_sol + buy_amount ≤ max_committed_sol`; (3) add the field to the Settings page. Phase 2 will build the committed-SOL counter regardless, so this is a ~one-check + one-setting add-on.
