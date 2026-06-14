# Buy/Sell Efficiency Review

Scope: `pump-trader/` trade path + `tpsl_sniper_*/execution/real.rs` caller. The hot path is already well-tuned (cached PDAs/routing, pre-built buy templates, durable-nonce pool, cached tip/blockhash, feed-confirm instead of RPC poll). The snipe buy is effectively **one network hop** (no pre-send RPC). Findings below are the *remaining* gaps, ranked by impact.

## High impact

### Sell-confirm polls a full SQL aggregation per tick — ✅ DONE
- **Where:** [real.rs](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L507) → `net_token_amount_by_wallet_and_mint` ([trade_repo.rs:370](backend/src/storage/repositories/trade_repo.rs#L370)).
- The exit loop confirms the clear by running `SELECT SUM(CASE…) FROM trades WHERE wallet=$1 AND mint=$2` on **every poll tick**, for **every concurrent exit**. That's a repeated aggregate scan over the large partitioned `trades` table on the exit hot path. (The `idx_trades_wallet_mint` composite index makes it an index range scan, not a full-table scan — but it's still a per-tick DB round-trip.)
- **Fix (implemented):** `TradeSignals` now carries a per-key `seq`, bumped in `notify()` once per trade the DbWriter persists for that wallet+mint. The confirm loop registers its guard once per exit and re-runs the SQL aggregate **only when `seq` advanced** — bare fallback ticks skip it, so the scan now runs ~once per landed trade instead of once per tick. SQL is kept as the authoritative "cleared" gate (deduped by PK) rather than a pure in-memory balance, because feed redelivery would double-count and over-sell. Applied to both `tpsl_sniper_{1,2}` clones.

## Low impact (micro / polish)

### `schedule_nonce_refresh` does a `get_account` RPC after every send — left as-is (by design)
- [nonce.rs](pump-trader/src/trader/nonce.rs#L86): one background RPC per trade to re-read the advanced nonce hash. Off the hot path, but it's an RPC-per-trade that could often be avoided (the new hash is derivable once the advance lands). Low priority; leave unless RPC quota matters. **Not changed:** deriving the advanced hash client-side is non-trivial and a wrong hash fails every subsequent send — not worth the risk for an off-hot-path background call.

### Per-trade heap allocations on the hot path — partially addressed
- `buy_data`/`sell_data` ([buy.rs:181](pump-trader/src/trader/buy.rs#L181), [sell.rs:402](pump-trader/src/trader/sell.rs#L402)) now use `Vec::with_capacity(24)` so the two `extend`s don't reallocate. ✅
- `mint.to_string()` (several) + two `DashMap` inserts + key clones per buy: left untouched — the clones are correctness-load-bearing cache keys and each cost is microseconds, negligible against network. Revisit only if profiling the CPU side.
