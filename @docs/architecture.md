# Architecture — backend skeleton & wiring

File-level map of the `backend` crate (binary-only). Reference this instead of re-reading `backend/src/`.
Subsystem deep-dives: [ingest.md](ingest.md) · [strategies.md](strategies.md) · [trade-execution.md](trade-execution.md) · [database.md](database.md) · [frontend.md](frontend.md).

## Two-crate split
- `pump-trader/` — standalone trade-execution crate (`pump_trader`). Has `lib.rs` + real unit tests. See [trade-execution.md](trade-execution.md).
- `backend/` — ingest, strategies, storage, HTTP API. Binary-only (`--bin backend`, no `lib.rs`).
- `backend/src/trader/mod.rs` re-exports `pump_trader::{PumpFunTrader, TraderConfig, WalletHolding}`.

## Composition root — `backend/src/main.rs`
`main` builds config → trader → DB → shared state → long-lived tokio tasks joined in one `tokio::select!` (any exit ⇒ log + stop).
- `require_bearer_auth()` — Actix middleware; mutating verbs need bearer **only if** `API_AUTH_TOKEN` set, GET/OPTIONS always pass.
- `parse_wallet_keypair()` — base58 → `Keypair`.
- `run_probe()` — one-shot `probe` subcommands (ladder/fanout/simulate-sell/holdings), run before DB/ingest, then exit.

**Long-lived tasks:** ingest gRPC producer → ingest pipeline → DbWriter · StrategyRunner · SOL price poller · pool-subscription refresh · partition maintenance · optional HTTP server.

**Shared state (Arc + channels created here):** `TokenCache`, `Tpsl1RuntimeCache`/`Tpsl2RuntimeCache`, `TokenListCache`, `sol_price` watch, `app_settings` watch, `live_mode` watch, `sse_tx` broadcast (producers) + `sse_frame_tx` broadcast (rendered frames to subscribers), `TradeSignals` (wakeup hub), `pool_index` (DashMap), `pools_changed` (Notify). When the HTTP server is enabled, `run_sse_render_bridge` is spawned to render `sse_tx` events to wire bytes once and fan them out on `sse_frame_tx`.

## Top-level module layout — `backend/src/`
| Module | Responsibility |
|---|---|
| `sweep/` | Strategy-agnostic param-sweep & backtest engine: corpus → (group →) sweep → folded per-combo metrics → per-strategy DB tables. Grouping + registry + grouped engine. See [sweep.md](sweep.md) |
| `api/` | HTTP route registration + handlers (tokens, trading, strategies, system) |
| `analyzers/` | `swing_analyzer.rs` — price-reversal (swing) detection over in-memory trades; `compute_chain_stats` groups legs into chains for the tokens-list chain-column sort |
| `config/` | `settings.rs` (env load) + `constants.rs` (pump.fun/Raydium program IDs, curve params) |
| `ingest_laserstream/` | Yellowstone gRPC live transport → pipeline → db_writer. See [ingest.md](ingest.md) |
| `models/` | Domain types (Token, Trade, Position, PaperRun, SseEvent, Tpsl{1,2}Rule, Wallet*) |
| `services/` | `sol_price.rs`, `token_sync.rs` (RPC backfill), `helius_rpc.rs`, `laserstream_replay.rs`, `wallet_tokens.rs`, `clients/`, `http.rs` |
| `state/` | In-memory shared state (below) |
| `storage/` | Postgres + repositories. See [database.md](database.md) |
| `strategies/` | StrategyRunner + tpsl_sniper_{1,2}. See [strategies.md](strategies.md) |
| `trader/` | Re-export shim for `pump-trader` |

## HTTP API — `backend/src/api/`
- `api/mod.rs` — `configure()` registers all `/api/*` routes. Scopes: `/api/token(s)/*`, `/api/stream`, `/api/system/*`, `/api/strategies/tpsl{1,2}/*`, `/api/strategies/sweeps/*` (generic grouped sweeps), `/api/solana/*`, `/api/profiles|wallets|tags`, `/api/creators`, `/api/analysis`.
- Handlers are thin: take `web::Data<Arc<AppState>>`, delegate to services/repos.

| Handler file | Owns |
|---|---|
| `handlers/tokens/tokens.rs` | `list_tokens`, `get_token`, `get_trades` |
| `handlers/tokens/sync.rs` | `sync_token`, `preview_sync` (RPC backfill, gated by `SyncGate`) |
| `handlers/tokens/analysis.rs` | `get_token_analysis`, `list_creators`, `get_creator`, `list_analysis_results` |
| `handlers/tokens/swing.rs` | `detect_token_swings`, `detect_tokens_swings_batch` |
| `handlers/trading/solana.rs` | `manual_buy`, `manual_sell` (**Sell All**: live-balance clear loop selling 100% each pass ≤ `SELL_ALL_MAX_PASSES`, then fire-and-forget `close_token_account` for rent), `get_wallet_tokens`, `get_wallet_token(_balance)`, `get_prices` |
| `handlers/system/stream.rs` | `stream_events` (`/api/stream`) + `SseFrame`/`run_sse_render_bridge` — render each event to wire bytes ONCE (one cache read), fan shared `Arc<SseFrame>` to all subscribers (no per-subscriber re-serialization) |
| `handlers/system/system.rs` | `get/set_live_mode`, `get_sol_price`, `get/update_settings` |
| `handlers/system/wallets.rs` | profile/wallet/tag CRUD |
| `handlers/strategies/tpsl1.rs` | rule CRUD + lifecycle (`activate`/`pause`/`stop`), `matched`, `simulate`, `paper-result` (GET = view, DELETE = "Clear results", paper + idle only) |
| `handlers/strategies/tpsl1_positions.rs` | position queries (by id/mint/wallet/rule, list) |
| `handlers/strategies/tpsl2*.rs` | identical surface for TPSL2 |

## Shared state — `backend/src/state/`
- `app_state.rs` — `AppState` (db, helius urls, all caches, watch channels, `sse_tx` + `sse_frame_tx`, `pool_index`, `pools_changed`, `trade_signals`, `sync_gate`, `backtest_trade_cache`, `trader`, `pump_program_id`). Also exposes repo accessors (`token_repo()`, `trade_repo()`, `settings_repo()`, `tpsl{1,2}_{rule,position,paper}_repo()`, `wallet{,_profile,_tag}_repo()`, `analysis_repo()`) so handlers call `state.x_repo()` instead of `XRepo::new(state.db.clone())`. `SyncGate` bounds `/api/token/sync` (max 4 concurrent, dedup by mint → 409 on collision).
- `token_cache.rs` — `TokenCache` = `DashMap<mint, TokenState>`; `TokenState` holds Token + capped trade buffer (~50K, `trades_base` survives trims) + metrics + `is_migrated` + `amm_pool_prewarmed` (the "warm AMM pool once per mint" guard — lives on the token's own state, so it's bounded by the cache rather than a separate never-evicted map). Cache-local; ingest never round-trips DB.
- `token_list_cache.rs` — `TokenListCache`, pre-sorted snapshot served by `/api/tokens` (saves per-request sort+clone).
- `backtest_trade_cache.rs` — `BacktestTradeCache`, cross-run per-mint trade-history cache for backtests (`Arc<Vec<Trade>>`). Freshness keyed on `TokenState::trade_count` (exact, free — no TTL/round-trip); FIFO eviction bounded by total cached trades. Re-running a tweaked rule re-fetches nothing for unchanged tokens. Backtest-only; never on the ingest/strategy hot path.
- `swing_run_cache.rs` — `SwingRunCache`, capacity-bounded (3) store of raw swing legs from "Swing Detection All" runs, keyed by a client-generated run id (`DashMap<run_id, Arc<SwingRun>>`, inner `DashMap<mint, Vec<SwingLeg>>` for lock-free concurrent chunk writes; oldest run FIFO-evicted). The batch swing handler stashes each chunk's legs (when the request carries `run_id`); `GET /api/tokens` reads them back to sort the browser-derived chain columns (`swing_pairs`/`max_seq_pairs`/`chain_count`) via `compute_chain_stats` at the requested `swing_chain_latency_ms` — so a latency change re-sorts without re-running detection. Swing-page-only; not on the ingest/strategy hot path.
- `trade_signals.rs` — `TradeSignals` wakeup hub with two lanes. **`(wallet,mint)` lane:** `register()`→`WaitGuard`, `notify()` called by DbWriter after a trade row is queryable (also bumps the slot `seq`). Keyed two levels deep (`DashMap<wallet, DashMap<mint, Slot>>`) so `notify` — run once per committed trade — resolves its `(wallet, mint)` with two **O(1)** `&str`-borrowed `get`s and no per-trade `String` alloc, not a linear shard scan. `WaitGuard::seq()` lets a waiter tell "new trade landed" from "bare fallback tick" so the sell-confirm loop skips its net-balance SQL when nothing changed. **Mint-only lane:** `register_mint()`→`MintWaitGuard`, `notify_mint()` called by the **ingest pipeline** right after the trade is appended to the `token_cache`, for watchers that follow the *whole mint* (TPSL2 scalp-entry arming, which reads the cache); same `is_empty` short-circuit keeps it cheap on the per-trade hot path. Pattern: **notify over poll**.
- `token_metrics.rs` — price/market-cap/volume/ATH computation.

## Config — `backend/src/config/`
- `settings.rs` — `Settings::from_env()`: Helius (api key, RPC, Sender URLs, LaserStream), wallet key, nonce accounts, DB url + pool sizing, server host/port/workers/CORS/`API_AUTH_TOKEN`.
- `constants.rs` — pump.fun + Raydium program IDs, bonding-curve reserve params, compute-budget/slippage constants.

## Cross-refs
Logic explainers live in `@project_plans/` (ingest, tpsl-strategy, trade-execution, token-analysis). This file maps *where code lives*; those explain *why*.
