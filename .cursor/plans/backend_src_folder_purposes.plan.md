# Backend `src/` Folder Purposes

Overview of each folder under `backend/src/`, based on `main.rs` wiring and each module's contents.

| Folder | Purpose |
|---|---|
| `config/` | Loads/validates app configuration (`settings.rs` from env, `constants.rs`). Source of `Settings`. |
| `ingest/` | The Helius data feed pipeline: `helius_ws.rs` (WebSocket connection), `subscription.rs`, `decoder.rs` (parse pump.fun txns), `pipeline.rs` (route events), `db_writer.rs` (persist to DB). Raw on-chain → structured events. |
| `models/` | Plain data types / structs for tokens, trades, transactions, positions, wallets, analysis, events, and SSE payloads. JSON is snake_case. |
| `state/` | In-memory shared runtime state: `app_state.rs` (the `AppState` passed to handlers), `token_cache.rs`, `creator_cache.rs`, `sol_price.rs` (SOL/USD cache + poller). |
| `storage/` | Database layer: `postgres.rs` (connect + migrations), `seed.rs` (warm caches), `repositories/` (all SQL lives here, one repo per entity). |
| `services/` | Higher-level business logic that isn't trading/strategy specific — e.g. `swing_analyzer.rs`, `price_service.rs`. |
| `strategies/` | Trading strategy engine: `runner.rs` (`StrategyRunner` consumes events) and `tpsl/` (take-profit/stop-loss rules, simulation, runtime cache). |
| `trader/` | On-chain execution: `pump_trader_optimized.rs` (`PumpFunTrader` builds/sends buy/sell txns) and `types.rs` (balances, holdings). |
| `api/` | HTTP layer (Actix): `mod.rs` registers all `/api` routes, `handlers/` holds thin request handlers (tokens, analysis, swing, strategies, wallets, trade, stream/SSE, system). |

## Data Flow

`ingest` feeds events → `strategies` decides → `trader` executes on-chain; `storage`/`state` persist & cache; `api` exposes it all over HTTP. All wired together in `main.rs`.
