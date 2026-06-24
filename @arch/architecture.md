# Architecture — backend skeleton

File-level map of the `backend` crate (binary-only). Read this instead of re-exploring `backend/src/`.
Deep-dive detail: `@plans/` (ingest, tpsl-strategy, trade-execution, token-analysis, deploy).

## Two-crate split

- `pump-trader/` — standalone trade-execution crate (`pump_trader`). Has `lib.rs` + unit tests. See [@arch/trade-execution.md](@arch/trade-execution.md).
- `backend/` — ingest, strategies, storage, HTTP API. Binary-only (`--bin backend`, no `lib.rs`).
- `backend/src/trader/mod.rs` re-exports `pump_trader::{PumpFunTrader, TraderConfig, WalletHolding}`.

## Composition root — `backend/src/main.rs`

`main` builds config → trader → DB → shared state → long-lived tokio tasks joined in one `tokio::select!`. Any task resolving (return/panic/abort) is a **fault** → `main` exits non-zero so a supervisor restarts. `TokenCache` seed runs in a spawned background task (not on the boot path).

**Long-lived tasks:** ingest gRPC producer → ingest pipeline → DbWriter · StrategyRunner · SOL price poller · pool-subscription refresh · token-cache eviction · token-list DB-base refresh · partition maintenance · optional HTTP server.

## Top-level module layout — `backend/src/`

| Module | Responsibility |
| --- | --- |
| `sweep/` | Strategy-agnostic param-sweep & backtest engine. See [@arch/sweep.md](@arch/sweep.md) |
| `api/` | HTTP route registration + handlers |
| `analyzers/` | `swing_analyzer.rs` (price-reversal detection), `compute_chain_stats` |
| `config/` | `settings.rs` (env load), `constants.rs` (program IDs, curve params) |
| `ingest_laserstream/` | Yellowstone gRPC live transport → pipeline → db_writer. See [@arch/ingest.md](@arch/ingest.md) |
| `models/` | Domain types (Token, Trade, Position, SseEvent, Tpsl{1,2}Rule, Wallet*) |
| `services/` | `sol_price.rs` (60s poller), `token_sync.rs` (RPC backfill), `wallet_reconcile.rs` (boot balance sweep), `helius_rpc.rs`, `laserstream_replay.rs` |
| `state/` | In-memory shared state (below) |
| `storage/` | Postgres + repositories. See [@arch/database.md](@arch/database.md) |
| `strategies/` | StrategyRunner + tpsl_sniper_{1,2}. See [@arch/strategies.md](@arch/strategies.md) |
| `trader/` | Re-export shim for `pump-trader` |

## HTTP API — `backend/src/api/`

`api/mod.rs` — `configure()` registers all `/api/*` routes. Handlers are thin: take `web::Data<Arc<AppState>>`, delegate to services/repos.

| Handler file | Owns |
| --- | --- |
| `handlers/tokens/tokens.rs` | `list_tokens`, `get_token`, `get_trades` |
| `handlers/tokens/batch.rs` | `get_tokens_batch` (up to 500 mints, `tokens LEFT JOIN tokens_info`) |
| `handlers/tokens/creation_stats.rs` | `get_creation_stats`, `get_grouped_creation_stats` |
| `handlers/tokens/sync.rs` | `sync_token`, `preview_sync` (RPC backfill, gated by `SyncGate`) |
| `handlers/tokens/analysis.rs` | `get_token_analysis`, `list_creators`, `get_creator`, `list_analysis_results` |
| `handlers/tokens/swing.rs` | `detect_token_swings` (single), `detect_tokens_swings_batch` (detached job → 202) |
| `handlers/trading/solana.rs` | `manual_buy`, `manual_sell` (Sell All), `get_wallet_tokens`, `get_prices` |
| `handlers/system/stream.rs` | `stream_events` (`/api/stream`) — renders events once, fans via `Arc<SseFrame>` |
| `handlers/system/system.rs` | `get/set_live_mode`, `get_sol_price`, `get/update_settings` |
| `handlers/system/wallets.rs` | profile/wallet/tag CRUD |
| `handlers/system/jobs.rs` | `job_status`, `cancel_simulation`, `simulation_result`, `cancel_swing`, `swing_result` |
| `handlers/strategies/tpsl1.rs` | rule CRUD + lifecycle, `matched`, `simulate` (detached → 202), `paper-result` |
| `handlers/strategies/tpsl1_positions.rs` | position queries |
| `handlers/strategies/tpsl2*.rs` | identical surface for TPSL2 |
| `handlers/strategies/grouped_sweep.rs` | generic sweep handler set. See [@arch/sweep.md](@arch/sweep.md) |

## Shared state — `backend/src/state/`

| File | Owns |
| --- | --- |
| `app_state.rs` | `AppState` — DB pools (`db`=api, `batch_db`=batch), all caches, watch channels, `sse_tx`/`sse_frame_tx`, `TradeSignals`, `trader`; repo accessors |
| `token_cache.rs` | `TokenCache` = `DashMap<mint, TokenState>`; slim `CachedTrade` projection; wallet-interned `u32`; runtime-bounded eviction |
| `token_list_cache.rs` | `TokenListCache` — staleness-bounded snapshot for `/api/tokens` (live overlay + DB base) |
| `backtest_trade_cache.rs` | `BacktestTradeCache` — cross-run per-mint trade cache for backtests; freshness keyed on `trade_count` |
| `swing_run_cache.rs` | `SwingRunCache` — capacity-bounded (3) store of swing legs by run id |
| `swing_results.rs` | `SwingResults` — finished-outcome store for "Swing Detection All" runs (lazy-TTL 600s) |
| `trade_signals.rs` | `TradeSignals` — wakeup hub: `(wallet,mint)` lane + mint-only lane. **Notify over poll** |
| `ingest_health.rs` | `IngestHeartbeat` — stamped by DbWriter each flush; OS-thread watchdog force-exits on stale+work-pending |
| `token_metrics.rs` | price/market-cap/volume/ATH computation |
