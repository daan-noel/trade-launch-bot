# Buy/Sell Efficiency Review

Scope: `pump-trader/` trade path + `tpsl_sniper_*/execution/real.rs` caller. The hot path is already well-tuned (cached PDAs/routing, pre-built buy templates, durable-nonce pool, cached tip/blockhash, feed-confirm instead of RPC poll). The snipe buy is effectively **one network hop** (no pre-send RPC). Findings below are the *remaining* gaps, ranked by impact.

## High impact

### Sell-confirm polls a full SQL aggregation per tick
- **Where:** [real.rs](backend/src/strategies/tpsl_sniper_1/execution/real.rs#L507) → `net_token_amount_by_wallet_and_mint` ([trade_repo.rs:370](backend/src/storage/repositories/trade_repo.rs#L370)).
- The exit loop confirms the clear by running `SELECT SUM(CASE…) FROM trades WHERE wallet=$1 AND mint=$2` on **every poll tick**, for **every concurrent exit**. That's a repeated aggregate scan over the large partitioned `trades` table on the exit hot path. (The `idx_trades_wallet_mint` composite index makes it an index range scan, not a full-table scan — but it's still a per-tick DB round-trip.)
- **Fix:** maintain the net balance from the WS reserve/trade feed already flowing through ingest (the loop is already woken by `TradeSignals` for this wallet+mint), and only fall back to the SQL SUM.

## Low impact (micro / polish)

### `schedule_nonce_refresh` does a `get_account` RPC after every send
- [nonce.rs](pump-trader/src/trader/nonce.rs#L86): one background RPC per trade to re-read the advanced nonce hash. Off the hot path, but it's an RPC-per-trade that could often be avoided (the new hash is derivable once the advance lands). Low priority; leave unless RPC quota matters.

### Per-trade heap allocations on the hot path
- `mint.to_string()` (several), `buy_data`/`sell_data` via `vec![disc…].extend` ([buy.rs:181](pump-trader/src/trader/buy.rs#L181), [sell.rs:292](pump-trader/src/trader/sell.rs#L292)), two `DashMap` inserts + key clones per buy. Each is microseconds — negligible against network, list only if profiling the CPU side.
