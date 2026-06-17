# Architecture — backend skeleton & wiring

File-level map of the `backend` crate (binary-only). Reference this instead of re-reading `backend/src/`.
Subsystem deep-dives: [ingest.md](ingest.md) · [strategies.md](strategies.md) · [trade-execution.md](trade-execution.md) · [database.md](database.md) · [frontend.md](frontend.md).

## Two-crate split
- `pump-trader/` — standalone trade-execution crate (`pump_trader`). Has `lib.rs` + real unit tests. See [trade-execution.md](trade-execution.md).
- `backend/` — ingest, strategies, storage, HTTP API. Binary-only (`--bin backend`, no `lib.rs`).
- `backend/src/trader/mod.rs` re-exports `pump_trader::{PumpFunTrader, TraderConfig, WalletHolding}`.

## Composition root — `backend/src/main.rs`
`main` builds config → trader → DB → shared state → long-lived tokio tasks joined in one `tokio::select!`. Any of the forever-tasks resolving (clean return, error, or panic) is a **fault**: `main` logs it and returns `Err` via the `task_fault` helper so the process **exits non-zero** and a supervisor restarts it (a clean HTTP-server stop is the one legitimate `Ok` shutdown — the old `_ = task` arms returned `Ok(())`, masking a panicked ingest/strategy task as a clean exit). The `TokenCache` seed (`storage::seed`) runs in a spawned background task, *not* on the boot path — ingest/HTTP start immediately and the cache hydrates concurrently (build-then-insert keeps it race-safe vs the live pipeline; a seed failure is logged, not fatal).
- `require_bearer_auth()` — Actix middleware, **fail-closed**: mutating verbs (POST/PUT/DELETE/PATCH) require a matching `Authorization: Bearer <API_AUTH_TOKEN>`; GET/OPTIONS always pass. `API_AUTH_TOKEN` is **required** at startup (`Settings::from_env` rejects missing/empty), so a forgotten token blocks trades instead of exposing them. The browser path supplies the bearer via the proxy (nginx `proxy_set_header Authorization` in prod, the Vite dev proxy in dev) — the token stays server-side, never in the bundle.
- `parse_wallet_keypair()` — base58 → `Keypair`.
- `run_probe()` — one-shot `probe` subcommands (ladder/fanout/simulate-sell/holdings), run before DB/ingest, then exit.

**Long-lived tasks:** ingest gRPC producer → ingest pipeline → DbWriter · StrategyRunner · SOL price poller · pool-subscription refresh · token-cache eviction · partition maintenance · optional HTTP server.

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
| `services/` | `sol_price.rs` (60s poller; **CoinGecko primary with bounded retry + `Retry-After`/exponential backoff, falls back to Jupiter** so one source being down/rate-limited doesn't stall the SOL/USD feed), `token_sync.rs` (RPC backfill; full "Fetch All" backfills **stream gTFA pages, decoding + flushing every `FLUSH_BACKFILL_ROWS`** so a high-volume mint's whole history of heavy raw-tx frames never materializes at once), `helius_rpc.rs`, `laserstream_replay.rs` (replay capped at `MAX_REPLAY_TXS` alongside the time cap), `wallet_tokens.rs`, `clients/` (`coingecko.rs`, `jupiter.rs` — both now `error_for_status` + backoff), `http.rs` |
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
| `handlers/tokens/creation_stats.rs` | `get_creation_stats` (`/api/tokens/creation-stats`) — token-creation-time bias aggregate (heatmap fold + absolute trend) for the Dashboard; reads `CreationStatsRepo`, shapes window/maturity/segment params (pure helpers unit-tested) |
| `handlers/tokens/sync.rs` | `sync_token`, `preview_sync` (RPC backfill, gated by `SyncGate`) |
| `handlers/tokens/analysis.rs` | `get_token_analysis`, `list_creators`, `get_creator`, `list_analysis_results` |
| `handlers/tokens/swing.rs` | `detect_token_swings`, `detect_tokens_swings_batch` |
| `handlers/trading/solana.rs` | `manual_buy` (validates `sol_amount`: finite, >0, ≤`MAX_MANUAL_BUY_SOL` → 400 before any on-chain work, **and the mint via `validate_solana_address`**), `manual_sell` (**Sell All**: live-balance clear loop selling 100% each pass ≤ `SELL_ALL_MAX_PASSES`, then fire-and-forget `close_token_account` for rent; mint validated up front), `get_wallet_tokens`, `get_wallet_token(_balance)` (wallet+mint validated before the RPC), `get_prices` (mints validated + capped at `MAX_PRICE_IDS`=100 before the single Jupiter fan-out — amplification guard) |
| `handlers/system/stream.rs` | `stream_events` (`/api/stream`) + `SseFrame`/`run_sse_render_bridge` — render each event to wire bytes ONCE (one cache read), fan shared `Arc<SseFrame>` to all subscribers (no per-subscriber re-serialization) |
| `handlers/system/system.rs` | `get/set_live_mode`, `get_sol_price`, `get/update_settings` |
| `handlers/system/wallets.rs` | profile/wallet/tag CRUD |
| `handlers/strategies/tpsl1.rs` | rule CRUD + lifecycle (`activate`/`pause`/`stop`), `matched`, `simulate`, `paper-result` (GET = view, DELETE = "Clear results", paper + idle only) |
| `handlers/strategies/tpsl1_positions.rs` | position queries (by id/mint/wallet/rule, list) |
| `handlers/strategies/tpsl2*.rs` | identical surface for TPSL2 |

## Shared state — `backend/src/state/`
- `app_state.rs` — `AppState` (db, helius urls, all caches, watch channels, `sse_tx` + `sse_frame_tx`, `pool_index`, `pools_changed`, `trade_signals`, `sync_gate`, `backtest_trade_cache`, `trader`, `pump_program_id`). Also exposes repo accessors (`token_repo()`, `trade_repo()`, `settings_repo()`, `tpsl{1,2}_{rule,position,paper}_repo()`, `wallet{,_profile,_tag}_repo()`, `analysis_repo()`) so handlers call `state.x_repo()` instead of `XRepo::new(state.db.clone())`. `SyncGate` bounds `/api/token/sync` (max 4 concurrent, dedup by mint → 409 on collision).
- `token_cache.rs` — `TokenCache` = `DashMap<mint, TokenState>`; `TokenState` holds Token + capped trade buffer (~50K, `trades_base` survives trims) + metrics + `is_migrated` + `amm_pool_prewarmed` (the "warm AMM pool once per mint" guard — lives on the token's own state, so it's bounded by the cache rather than a separate never-evicted map). Cache-local; ingest never round-trips DB. **Runtime-bounded:** `run_token_cache_eviction` (spawned in `main`, coarse interval) drops mints idle past `TOKEN_CACHE_EVICT_IDLE_SECONDS` (last trade, or creation time if never traded) that hold no open position — the held-mint exemption (`Tpsl{1,2}RuntimeCache::is_mint_held`, paper + real) is read from memory, no DB round trip. Evicted mints re-add on a manual `token_sync`, same contract as the seed's activity window.
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
