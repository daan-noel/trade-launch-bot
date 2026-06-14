# Buy/Sell Efficiency Review

Scope: `pump-trader/` trade path + `tpsl_sniper_*/execution/real.rs` caller. The hot path is already well-tuned (cached PDAs/routing, pre-built buy templates, durable-nonce pool, cached tip/blockhash, feed-confirm instead of RPC poll). The snipe buy is effectively **one network hop** (no pre-send RPC). Findings below are the *remaining* gaps, ranked by impact.

## High impact

### 1. Token-account rent is never reclaimed (SOL leak) — biggest cost
- **Where:** [sell.rs](pump-trader/src/trader/sell.rs) `build_curve_sell_ixs`, [amm.rs](pump-trader/src/trader/amm.rs) `build_amm_sell_ixs`.
- Every buy creates a token account (`create_with_seed` / base ATA) holding **~0.002 SOL rent**. The sell empties it but never **closes** it, so rent is stranded on every position. At snipe volume this dwarfs every other fee (priority fee ≈ 0.00003 SOL, base fee 0.000005 SOL, tip 0.0002–0.005 SOL).
- **Fix:** on the *final* clearing sell (full balance → 0), append `close_account(user_token_account)` to recover rent. Caveat: only when selling 100% of remaining balance — guard against the partial-fill/retry path in `sell_until_balance_cleared` so it doesn't close a still-funded account. Verify curve-sell+nonce+close stays under the 1232 B tx limit (AMM sell is already near the limit — may not fit there).

### 2. Sell-confirm polls a full SQL aggregation per tick
- **Where:** [real.rs](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L507) → `net_token_amount_by_wallet_and_mint` ([trade_repo.rs:370](backend/src/storage/repositories/trade_repo.rs#L370)).
- The exit loop confirms the clear by running `SELECT SUM(CASE…) FROM trades WHERE wallet=$1 AND mint=$2` on **every poll tick**, for **every concurrent exit**. That's a repeated aggregate scan over the large partitioned `trades` table on the exit hot path.
- **Fix:** maintain the net balance from the WS reserve/trade feed already flowing through ingest (the loop is already woken by `TradeSignals` for this wallet+mint), and only fall back to the SQL SUM. Or at minimum confirm a covering index on `(wallet_address, mint_address)` exists and the query is index-only.

## Medium impact

### 3. No TLS/connection warmup → first trade pays handshake latency
- **Where:** [mod.rs](pump-trader/src/trader/mod.rs#L287) `http: reqwest::Client::new()`; senders/RPC are hit cold on the first send.
- The first POST to each Helius Sender + RPC pays a full TLS handshake (~1 RTT+) on the critical send. `initialize()` warms tip/blockhash/nonce caches but not the HTTP connection pool.
- **Fix:** in `initialize()`, fire a cheap warmup request (e.g. `getHealth`) to each `helius_sender_urls` entry and the RPC to seed the keep-alive pool. Optionally set explicit `pool_idle_timeout`, `tcp_nodelay(true)`, `http2_prior_knowledge` on the client.

### 4. Nonce slot map uses an async mutex with a sync-only critical section
- **Where:** [nonce.rs](pump-trader/src/trader/nonce.rs#L37) `nonce_slots: Mutex<HashMap>` (tokio `Mutex`).
- `acquire_nonce` holds `nonce_slots.lock().await` only for a synchronous scan (no `.await` inside the section); the `notified()` wait is already outside the lock. This is the one shared lock every buy **and** sell contends on. An async mutex adds scheduler overhead vs a sync lock for a section that never yields — same reasoning already applied to `JitoTipCache`/`BlockhashCache` (both `std::sync::Mutex`).
- **Fix:** switch `nonce_slots` to `std::sync::Mutex`/`parking_lot::Mutex`. `schedule_nonce_refresh` also only locks for a sync write, so it converts cleanly too.

## Low impact (micro / polish)

### 5. `schedule_nonce_refresh` does a `get_account` RPC after every send
- [nonce.rs](pump-trader/src/trader/nonce.rs#L86): one background RPC per trade to re-read the advanced nonce hash. Off the hot path, but it's an RPC-per-trade that could often be avoided (the new hash is derivable once the advance lands). Low priority; leave unless RPC quota matters.

### 6. Per-trade heap allocations on the hot path
- `mint.to_string()` (several), `buy_data`/`sell_data` via `vec![disc…].extend` ([buy.rs:181](pump-trader/src/trader/buy.rs#L181), [sell.rs:292](pump-trader/src/trader/sell.rs#L292)), two `DashMap` inserts + key clones per buy. Each is microseconds — negligible against network, list only if profiling the CPU side.

## Already good (no action)
- Snipe buy: zero pre-send RPC (no ATA check, no reserve read, no confirm) — tip + nonce served from cache.
- Sells use `confirm=false` on the live path (no redundant RPC confirm) and escalate the Jito tip per retry; a non-landing tx costs nothing.
- Sender fan-out serializes the body once (`Arc`), dedups on-chain (tip paid once).
- CU limits split per path so priority fee isn't sized for the heaviest path; CU price already cut 5×.
- Reserve/routing/pool/config all cache-first with freshness bounds + venue tagging.

---
**Suggested order:** #1 (recover rent — real money) → #2 (DB load on exits) → #3 (first-trade latency) → #4 (lock contention under load).
