# Architecture — workspace skeleton

File-level map of the backend workspace. Read this instead of re-exploring source.
The old single `backend` crate was split over a shared core and then renamed to the
`live`/`lab` topology: a shared core lib, the trader lib, two ingest-transport libs, and
two binaries (`live` = live trading, `lab` = analysis). Deep-dive detail: `@plans/`.

## Crate map

| Crate | Kind | Owns | Depends on |
| --- | --- | --- | --- |
| `trading_core` | lib | Everything shared by both bins: `config` (incl. protocol/CU constants), `models`, `storage` (pools + repos), core `services`, core `state`, the actix api framework + auth + SSE bridge, **core handlers**, the trading-free **strategy domain** (`strategies/` = fingerprint matching + metric series + the shared cost/summary kernel; the pure fold lives in `hunter-engine`), and the **ingest contract** (`ingest`). Exposes `configure_core_routes`. | `hunter-engine` (NOT `pump-trader`) |
| `pump-trader` | lib | Trade execution (`PumpFunTrader`, sims, cashback). Owns protocol/tuning constants in-crate (`constants`, folded in from the former `pump-constants`). See [@arch/trade-execution.md](@arch/trade-execution.md) | — |
| `ingest-laserstream` | lib | Helius LaserStream gRPC live transport: client→pipeline→db_writer + heartbeat/watchdog (partition maintenance removed — Timescale policies). Exposes its own raw transport API (`Ingest`/`IngestHandle`/`IngestEvent`/`Protocol`); `live/src/ingest/` bridges it onto the `trading_core::ingest` contract. See [@arch/ingest.md](@arch/ingest.md) | **standalone — no workspace deps** (tonic/prost/tokio-stream/solana-sdk/borsh; NOT `trading_core`, NOT `pump-trader`) |
| `hunter-engine` | lib | The **pure** strategy fold: `reduce(state, event) -> effects` (no clock/IO/rand, purity-guarded). Metric registry, fingerprint matching, deadness verdict, cost/summary kernel, the on-disk event-log format. Consumed by `trading_core`, `live`, and `lab`. See [@arch/strategies.md](@arch/strategies.md) | **pure — serde/smallvec/chrono-no-clock/uuid only** |
| `live` | **bin** | Live-trading box: `strategies/engine/` (the live adapters around the fold — decision loop, producers, exec, sinks, event-log recorder), `trader/` (pump-trader shim), deploy services/state/handlers, the `probe` subcommand, deploy `main.rs`. Serves core + deploy routes. | `trading_core` + `hunter-engine` + `ingest-laserstream` + `pump-trader` |
| `lab` | **bin** | Analysis box (no trading keys, no gRPC): `sweep/` engine, `analyzers/`, local state/handlers, backtest harness, local `main.rs`. Serves core + local routes. See [@arch/sweep.md](@arch/sweep.md) | `trading_core` + rayon/arrow/parquet (NOT `pump-trader`, NOT `ingest-laserstream`) |

**Deliberately-duplicated protocol constants** (program IDs, mints, `LAMPORTS_PER_SOL`)
live in both `trading_core::config::constants` and `pump_trader::protocol` so the trader
stays dependency-free (it must not pull `trading_core`). The split is guarded, not
trusted: `live/tests/protocol_constants_ssot.rs` (`live` is the only crate depending on
both) asserts the two copies stay byte-equal, so a program-ID change applied to one crate
can't silently break the other.

## Composition roots — the two `main.rs` files

Each bin builds config → DB pools → shared state → long-lived tokio tasks joined in one
`tokio::select!`. Any task resolving (return/panic/abort) is a **fault** → `main` exits
non-zero so a supervisor restarts. `TokenCache` seed runs in a spawned background task
(not on the boot path). Both bins gate the HTTP server on `HTTP_ENABLED` and wrap it with
the bearer-auth middleware (fail-closed on mutating requests) + CORS.

### `live/src/main.rs` — live trading

Builds trader → DB pools → caches → `CoreState` → `DeployState`. Long-lived tasks:

- **ingest** (host adapter `live::ingest::spawn_ingest`, which drives `ingest_laserstream::Ingest`): gRPC producer · pipeline · DbWriter
- **the generic engine** (`strategies::engine::spawn_engine`) — the one serialized decision loop, driven by the ingest `strategy_rx` + a 500 ms tick + confirmed fills; plus its PG recovery reaper
- **SOL price poller**
- optional **HTTP server** (core + deploy routes)

Plus fire-and-forget spawns off the boot path: boot wallet reconcile, SOL-balance refresh
(30s), token-cache eviction, token-list DB-base refresh, SSE render bridge. `probe`
subcommand (`probe <ladder|fanout|check-nonces|simulate-*|holdings|cashback-*>`) runs a
one-shot validation against live infra and exits before any DB/ingest/HTTP startup.

### `lab/src/main.rs` — analysis

No trader, no ingest, no strategy runner; loads `Settings::from_env_local` (no trading
keys / no HELIUS gRPC required). Builds DB pools → empty `TokenCache` → `CoreState` →
`LocalState`. Long-lived tasks:

- **SOL price poller**
- optional **HTTP server** (core + local routes)

Plus on boot: grouped-sweep orphan reconcile (crash recovery), token-list DB-base refresh,
SSE render bridge. The token list is served from the DB base (no live ingest feeds it).

## State structs — narrow-state handler convention

The old fat `AppState` is replaced by three structs. `CoreState` holds everything
mode-agnostic; each bin's state holds `Arc<CoreState>` + its own handles and `Deref`s to
it, so a handler reads `state.token_repo()` / `state.token_cache` whichever state it was
injected with. Handlers take the **narrowest** state they need:

- core handlers → `web::Data<Arc<CoreState>>`
- deploy handlers → `web::Data<DeployState>`
- local handlers → `web::Data<LocalState>`

### `CoreState` — `trading_core/src/state/core_state.rs`

| Field / accessor | Owns |
| --- | --- |
| `db` (api), `batch_db` | workload-isolated Postgres pools (`db`=fast handlers, `batch_db`=heavy jobs) |
| `helius_rpc_url`, `helius_laserstream_url`, `helius_api_key`, `pump_program_id` | endpoint/config literals |
| `token_cache`, `token_list` | live token state + staleness-bounded `/api/tokens` snapshot |
| `sse_tx`, `sse_frame_tx` | cold SSE event lane + pre-rendered frame fan-out |
| `settings` (watch), `sol_price` (watch) | in-memory settings source-of-truth + SOL/USD |
| `*_repo()` accessors | thin per-call repo handles (token/trade/settings/analysis/creation-stats/wallet/profile/tag; strategy: `RuleRepo`/`FingerprintRepo`/`StrategyRepo`) |

### `DeployState` — `live/src/state/deploy_state.rs`

`core: Arc<CoreState>` + `trader` · `engine: EngineHandle` (the handle onto the generic decision
loop — rule/fingerprint CRUD ping it to reload, and manual/stop closes route through it) ·
`armed: ArmedRegistry` (live armed-(token,rule) snapshot for `GET /api/strategies/armed`) ·
`strategy_repo` / `rule_repo` / `fingerprint_repo` (the durable read/write surfaces the HTTP layer
shares with the engine) · `pool_index` + `pools_changed` (live pool→mint index) · `trade_signals`
(confirm-loop wakeup hub) · `sync_gate` (`SyncGate`, per-mint dedup + concurrency for `/token/sync`) ·
`live_mode` (watch).

### `LocalState` — `lab/src/state/local_state.rs`

`core: Arc<CoreState>` + `sweep_running` / `sweep_cancel` /
`sweep_progress` (single-flight grouped sweep) · `sim_cancels` / `sim_progress` /
`sim_results` (per-rule backtests — disk-backed under `$SWEEP_LAKE_DIR/sim-results/`,
meta index + one-row working set in RAM; kept until re-sim / rule-config change, no TTL) ·
`backtest_sem` (concurrency cap) · `sweep_corpus_cache`
(`SweepCorpusCache`, warm-path corpus reuse).

## Ingest contract — `trading_core::ingest` + `<transport>::spawn(...)`

The transport-agnostic contract lives in `trading_core::ingest`: `IngestHandles`, the
`TraderHook` trait (IoC, keeps transport crates free of `pump-trader`), and re-exports of
`StrategyPing` / `TradeSignals`. Each ingest crate depends on `trading_core` and exposes one
`spawn(...)` of this shape; `ingest-laserstream` is the live transport. The deploy `main.rs`
is the only caller.

```text
spawn(helius_laserstream_url, helius_api_key, pump_program_id, db, token_cache,
      sse_tx, settings_rx, live_rx, trader: Arc<dyn TraderHook>, trade_signals)
    -> IngestHandles { pool_index, pools_changed, strategy_rx,
                       producer_task, pipeline_task, db_writer_task }
```

`spawn` internally creates the `IngestHeartbeat` and starts the watchdog
(`ingest_health.rs`), the pool-subscription refresh, and the queue-depth logger — all crate-owned,
not caller-driven. (Partition maintenance is gone: TimescaleDB retention/compression policies now
manage chunk lifecycle declaratively.) The deploy bin bridges
its `PumpFunTrader` to `TraderHook` via `trader::TraderHookBridge`. See [@arch/ingest.md](@arch/ingest.md).

## Strategy layering — one engine, three layers

There is one generic engine (Phase 7 retired the per-strategy tpsl/swing stack). The
decision core is a pure fold; the two bins are adapters, so the live and backtest
paths can never drift:

| Layer | Lives in | Holds |
| --- | --- | --- |
| **Pure fold** (all decision logic) | `hunter-engine` | `reduce`, metric registry, fingerprint matching, deadness verdict, cost/summary kernel. See [@arch/strategies.md](@arch/strategies.md) |
| **Domain glue** (rule CRUD + validation + DB↔engine converters) | `trading_core` | `strategies/{rules,fingerprint_axes,kernel,death}` + `RuleRepo`/`FingerprintRepo`. Written once; used by both edges |
| **Runtime edge — deploy** | `live` | `strategies/engine/` adapters (decision loop + producers + exec + sinks + event-log) + position reads |
| **Runtime edge — local** | `lab` | replay/simulate + the generic grouped sweep, both driving the same fold |

Rule CRUD = a core helper (`strategies::rules`, validate + build + `RuleRepo` write)
wrapped by a per-bin handler whose only difference is the side effect (deploy pings the
engine to reload; local does nothing).

## HTTP API — handlers by crate

`trading_core/src/api/mod.rs` `configure_core_routes` registers mode-agnostic routes with
full `/api/...` paths (no nested scope, so configs compose). Each bin then `.configure`s
its own route set (`configure_deploy_routes` / `configure_local_routes`, both `web::scope("/api")`).
The frontend split is build-time, so there is no runtime capability advertisement — each bin
is built into its own SPA (`@live`/`@lab`) with a static nav. See [@arch/frontend.md](@arch/frontend.md).

### Core routes (`trading_core`, take `Arc<CoreState>`)

| Handler file | Owns |
| --- | --- |
| `handlers/tokens/tokens.rs` | `get_token`, `get_trades` (token detail/trades reads) |
| `handlers/tokens/batch.rs` | `post_tokens_batch` (up to 500 mints, `tokens LEFT JOIN tokens_info`) |
| `handlers/tokens/creation_stats.rs` | `get_creation_stats`, `get_grouped_creation_stats` |
| `handlers/system/stream.rs` | `stream_events` (`/api/stream`) + `run_sse_render_bridge` (renders once, fans `Arc<SseFrame>`) |
| `handlers/system/system.rs` | `get_sol_price`, `get/update_settings` |
| `handlers/system/wallets.rs` | profile / wallet / tag CRUD |
| `handlers/strategies/mod.rs` | (doc-only) points at the generic rule-domain helpers in `strategies::rules` (validate → build → `RuleRepo`), used by both CRUD edges |

### Deploy routes (`live`, take `DeployState`)

| Handler file | Owns |
| --- | --- |
| `handlers/system/live_mode.rs` | `get/set_live_mode` |
| `handlers/tokens/sync.rs` | `sync_token`, `preview_sync` (RPC backfill, gated by `SyncGate`) |
| `handlers/trading/solana.rs` | `manual_buy`, `manual_sell` (Sell All), `get_wallet_tokens`, `get_wallet_token(_balance)`, `get_prices` |
| `handlers/trading/portfolio.rs` | `/api/portfolio/{holdings,summary,positions}` — thin reads over `services::portfolio` (holdings + cost basis + PnL + bot tag; wallet KPI summary; cross-strategy open-positions roll-up). Holdings/Home/Live-Trading surfaces |
| `handlers/trading/cashback.rs` | `get_cashback_status`, `claim_cashback` |
| `handlers/strategies/positions.rs` | position reads over `StrategyRepo` — by rule / mint / wallet / id; per-row "Sell ALL" routes the close through `EngineHandle::manual_close`. The `{strategy}` path segment is retained for URL back-compat but ignored (all positions are `'generic'`) |
| `handlers/strategies/engine.rs` | generic rule + fingerprint CRUD + lifecycle (`/strategy-rules/*`, `/fingerprints/*`, activate/pause/stop, pause-all/stop-all) + `/meta/strategy-registry` + `/strategies/armed`, over `RuleRepo`/`FingerprintRepo` + the `EngineHandle`. (The legacy per-`{strategy}` `rules.rs` handler was deleted in Phase 7) |

### Local routes (`lab`, take `LocalState`)

| Handler file | Owns |
| --- | --- |
| `handlers/tokens/list.rs` | `list_tokens` (`GET /api/tokens`, in-RAM engine over a full snapshot) |
| `handlers/tokens/metric_series.rs` | `GET /api/tokens/{mint}/metric-series` — every metric's value at every trade (rule-authoring chart panes) |
| `handlers/system/jobs.rs` | `job_status`, `cancel/result` for simulations |
| `handlers/strategies/engine.rs` | generic `simulate` (detached → 202) + its result page/summary/matched (+ batch `POST …/simulate/summaries`) — one surface for every rule (`rule_id` or inline `draft`) |
| `handlers/strategies/engine_crud.rs` | lab-side rule + fingerprint CRUD (`strategy_rules` / `fingerprints`, no live engine to ping) |
| `handlers/strategies/positions.rs` | the simulated-result table (in-memory server-side paging over the finished sim's rows) |
| `handlers/strategies/grouped_sweep.rs` | generic grouped param-sweep handler set. See [@arch/sweep.md](@arch/sweep.md) |
| `handlers/replay.rs` | `POST /api/replay/inspect` — re-run a recorded event log through the fold (time-travel debugger) |

## Other shared state — `trading_core/src/state/`

| File | Owns |
| --- | --- |
| `token_cache.rs` | `TokenCache` = `DashMap<mint, TokenState>`; slim `CachedTrade` projection; wallet-interned `u32`; runtime-bounded eviction (`run_token_cache_eviction`) |
| `token_list_cache.rs` | `TokenListCache` — staleness-bounded snapshot for `/api/tokens` (live overlay + DB base; `run_token_list_db_refresh`) |
| `token_metrics.rs` | price / market-cap / volume / ATH computation |
| `trade_signals.rs` | `TradeSignals` — wakeup hub: `(wallet,mint)` lane + mint-only lane. **Notify over poll** (held by `DeployState`) |

`IngestHeartbeat` + watchdog live in `ingest-laserstream/src/ingest_health.rs` (not in
`state/`). Local-only state (`job_progress`, `sim_results`,
`swing_results`, `swing_run_cache`) lives in `lab/src/state/`. See
[@arch/database.md](@arch/database.md) for pools + repos.
