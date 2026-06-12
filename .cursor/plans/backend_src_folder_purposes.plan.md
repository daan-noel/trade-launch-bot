# Backend `src/` Folder Purposes

Overview of each folder under `backend/src/`, based on `main.rs` wiring and each module's contents.

> **Updated 2026-06-11** to the current layout: the WS `ingest/` folder was
> removed (LaserStream is the sole transport → `ingest_laserstream/`), the trader
> moved to the standalone `pump-trader` crate (`trader/` is now a thin re-export),
> swing analysis lives in `analyzers/`, and `sol_price` moved into `services/`.

| Folder | Purpose |
|---|---|
| `config/` | Loads/validates app configuration (`settings.rs` from env, `constants.rs`). Source of `Settings`. |
| `ingest_laserstream/` | The **sole** ingest pipeline (LaserStream / Yellowstone gRPC): `client.rs` (gRPC connect/stream/reconnect), `adapter.rs` (protobuf → Helius-shaped `Value`), `decoder/` (parse pump.fun txns), `pipeline.rs` (route events + feed reserve cache / AMM prewarm), `db_writer.rs`, `maintenance.rs`, `generated/` (committed protobuf codegen). Raw on-chain → structured events. (The old WS `ingest/` folder was deleted.) |
| `models/` | Plain data types / structs for tokens, trades, transactions, positions, wallets, analysis, events, and SSE payloads. JSON is snake_case. |
| `state/` | In-memory shared runtime state: `app_state.rs` (the `AppState` passed to handlers), `token_cache.rs`, `creator_cache.rs`. (SOL price now lives in `services/sol_price.rs`.) |
| `storage/` | Database layer: `postgres.rs` (connect + migrations), `seed.rs` (warm caches), `repositories/` (all SQL lives here, one repo per entity). |
| `services/` | Higher-level non-strategy business logic: `token_sync.rs` (gTFA backfill + incremental sync), `helius_rpc.rs`, `laserstream_replay.rs` (sync replay window), `sol_price.rs` (SOL/USD cache + poller), `wallet_tokens.rs`, `http.rs`, `clients/` (coingecko/jupiter). |
| `analyzers/` | Read-only analysis algorithms — `swing_analyzer.rs` (price-swing detection over a token's trades). |
| `strategies/` | Trading strategy engine: `runner.rs` (`StrategyRunner` consumes events) and the sibling TPSL strategies `tpsl_sniper_1/` (legacy fingerprint entry) + `tpsl_sniper_2/` (scalp/continuation entry). Each is modularized into `entry/ exit/ execution/ service.rs runtime_cache.rs handler.rs backtest.rs paper_run.rs`. (The old shared `tpsl/` folder was deleted.) |
| `trader/` | Thin re-export of the standalone **`pump-trader`** crate (`PumpFunTrader`, `TraderConfig`, `WalletHolding`); the actual on-chain buy/sell/AMM execution + caches live in `pump-trader/src/trader/`. |
| `api/` | HTTP layer (Actix): `mod.rs` registers all `/api` routes, `handlers/` holds thin request handlers (tokens, analysis, swing, strategies, wallets, trade, stream/SSE, system). |
| `bin/` | Standalone binaries / dev utilities (sit alongside the main backend binary). |

## Data Flow

`ingest_laserstream` feeds events → `strategies` decides → `trader` (`pump-trader` crate) executes on-chain; `storage`/`state` persist & cache; `api` exposes it all over HTTP. All wired together in `main.rs`.
