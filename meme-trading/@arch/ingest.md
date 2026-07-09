# Ingest — LaserStream (the sole live transport)

File-level map of `ingest-laserstream/` (standalone crate) and `live/src/ingest/` (host adapter). Helius LaserStream (Yellowstone gRPC) is the **only** live ingest path.
Logic explainers: `@plans/ingest/laserstream-workflow.md`, `@plans/ingest/token_sync-workflow.md`, `@plans/ingest/backpressure-watchdog.md`, `@plans/ingest/reconnect-restart-flow.md`.

## Architecture

`ingest-laserstream` is a **standalone drop-in crate** — no workspace deps. It emits decoded `IngestEvent`s out a bounded mpsc channel; the host (`live`) owns all sinks.

```text
Ingest::builder().build()?.start(live)
  -> (Receiver<IngestEvent>, IngestHandle)

Internal crate topology:
  transport::run()  (gRPC, classify)
    --(Arc<SubscribeUpdateTransaction>, TxRelevance, DateTime<Utc>)-->
  decode task  (Decoder::decode_relevant_pb)
    --IngestEvent-->  host event channel

Host adapter (live/src/ingest/):
  consumer.rs  (translate IngestEvent -> trading_core types, fan-out)
    --> { db_tx, strategy_tx, sse_tx, trader, trade_signals }
  db_writer.rs  (batch persistence) --> Postgres --> TradeSignals.notify
  watchdog.rs  (OS thread, DbHeartbeat)
```

Channels: `update_tx` cap 4096 · `event_rx` cap 8192 · `db_tx` cap 16384 · `strategy_tx` cap 512 · `sse_tx` broadcast.

**Backpressure:** money-critical writes (`Trade`/`Migration`/`Token`/`Wallet`) use `send().await` (never dropped); recomputable writes (`Metrics`/`Raw`) use `try_send` (dropped on Full). See `@plans/ingest/backpressure-watchdog.md`.

**Liveness watchdog:** `db_writer.rs` stamps `DbHeartbeat` at the end of every `flush()`. A dedicated OS thread (via `watchdog::spawn_watchdog`) force-exits (`exit(1)`) when the heartbeat is stale AND the DB queue is non-empty AND live mode is on.

## `ingest-laserstream/src/` — crate modules

| File | Responsibility |
| --- | --- |
| `lib.rs` | `Ingest` builder + `IngestHandle`; spawns transport + decode tasks; `start(live)` → `(Receiver<IngestEvent>, IngestHandle)` |
| `event.rs` | Crate-owned output types: `IngestEvent`, `Trade`, `TokenCreated`, `TokenMigrated`, `LiquidityEvent`, `CreatorActivityEvent`, `BuyInstructionArgs`, `Side`, `Venue`, `Reserves`; `RawTx` under `raw-json` feature |
| `protocol.rs` | `Protocol` descriptor: pre-decoded program IDs (`ProgramId` w/ bytes + base58) + discriminators decoded once at build time |
| `config.rs` | `IngestConfig` (all tunables, no env reads), `Commitment` enum |
| `error.rs` | `IngestError`, `Result<T>` alias |
| `pool.rs` | PDA derivation (`derive_pool`), `register_pool`, `pool_for_mint`; `PoolIndex = Arc<DashMap<String, String>>` |
| `transport/mod.rs` | gRPC producer: TLS auth, reconnect w/ backoff, replay from `last_slot`, idle-reconnect timer, backpressure guard; `connect`, `build_subscribe_request`, `TransportConfig` |
| `decode/mod.rs` | `Decoder` (+ `HeliusDecoder` back-compat alias), `TxRelevance`, `DecodeOutput` |
| `decode/grpc.rs` | `decode_protobuf` (self-classify), `decode_relevant_pb` (hot path), `decode_amm_protobuf` (backfill); `LazyKeys`. **Curve TradeEvents: read "Program data:" logs first, but the validator truncates logs past a byte limit, so a multi-buy bundle can lose trailing legs — when logs are empty OR carry "Log truncated", re-decode from the complete inner-instruction self-CPI events and take the larger set. AMM path is still log-only (latent same risk).** |
| `decode/trade.rs` | Borsh `RawTradeEvent`, trade helpers |
| `decode/instructions.rs` | `InstructionKind`, labeler |
| `decode/create.rs` | `decode_create_events_from_logs` |
| `raw_tx.rs` | `encode_payload` (protobuf wire bytes), `build_raw_tx_event` — **`raw-tx` feature** |
| `backfill.rs` | `rpc_to_protobuf` (RPC result → protobuf) — **`rpc-backfill` feature** |

### Feature gates

| Feature | Unlocks |
| --- | --- |
| `raw-tx` | `IngestEvent::RawTx` (carries protobuf `payload` bytes), `raw_tx::encode_payload` |
| `rpc-backfill` | `serde_json` dep, `backfill::rpc_to_protobuf` |

`live` enables both. `IngestHandle` exposes `set_live`, `track_pools`, `untrack_pools`, `pool_index`, `pools_changed`. (Liveness is tracked host-side by `live`'s own `DbHeartbeat`, not an ingest health channel.)

## `live/src/ingest/` — host adapter

| File | Responsibility |
| --- | --- |
| `mod.rs` | `spawn_ingest(...)` — builds `Ingest`, starts it, spawns consumer + db_writer tasks, starts watchdog thread; returns `IngestSpawnResult` |
| `consumer.rs` | `IngestConsumer` — translates `IngestEvent` → `trading_core` types; fans out to token_cache, DB, strategy, SSE, trader; handles `track_mayhem` / `track_post_migration` policy transitions |
| `db_writer.rs` | `DbWriter` — batches (1000 ops / 150ms), dedups, persists; stamps `DbHeartbeat` each flush; signals `TradeSignals` per `(wallet,mint)`; `DbWriteOp` variants: `Raw(RawBlobJob)` · `Token` · `Wallet` · `Trade` · `Metrics` · `Migration` |
| `watchdog.rs` | `DbHeartbeat` (atomic ms stamp), `spawn_watchdog` (OS thread); force-exits when DB queue backed up and no commit within `watchdog_stall_timeout_secs` |

### Consumer event handlers

`on_token_created` (Token+Wallet+Metrics+cache+ping+SSE) · `on_trade` (Trade+Wallet+Metrics+reserves+AMM prewarm+ping+SSE) · `on_token_migrated` (pool gate+Migration+ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only)

## Decoder — `decode/`

Protobuf-native only (no Value/JSON path in decode). Both live ingest and `token_sync.rs` feed `decode_protobuf`.

Pool→mint index (`PoolIndex`) is shared: the decode task auto-registers pools on `TokenMigrated`; the consumer un-registers when `track_post_migration` is off.

Codegen: committed prost/tonic bindings in `generated/`; `.proto` sources in `proto/`. Regen only when `.proto` changes.

## Key rules

- `trades` table = this feed. TPSL exit loop confirms fills from this feed (not a separate RPC).
- No blocking I/O / `.await`-on-lock / unbounded alloc per event on the ingest hot path; DB+SSE go through channels.
- `ingest-laserstream` has **zero workspace deps** — standalone drop-in crate.
