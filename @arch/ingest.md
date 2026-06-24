# Ingest — LaserStream (the sole live transport)

File-level map of `backend/src/ingest_laserstream/`. Helius LaserStream (Yellowstone gRPC) is the **only** live ingest path.
Logic explainers: `@plans/ingest/laserstream-workflow.md`, `@plans/ingest/token_sync-workflow.md`, `@plans/ingest/backpressure-watchdog.md`, `@plans/ingest/reconnect-restart-flow.md`.

## Data flow

```text
client.rs (gRPC, pre-filter + classify)  --(Arc<SubscribeUpdateTransaction>, TxRelevance)-->
pipeline.rs (decode + TokenCache update + fan-out)  -->  { db_tx, strategy_tx, sse_tx }
db_writer.rs (build_raw_blob off-thread)  --batched-->  Postgres  -->  TradeSignals.notify(wallet,mint)
```

Channels: `update_tx` cap 4096 · `db_tx` cap 16384 · `strategy_tx` cap 512 · `sse_tx` broadcast.

**Backpressure:** money-critical writes (`Trade`/`Migration`/`Token`/`Wallet`) use `send().await` (never dropped); recomputable writes (`Metrics`/`Raw`) use `try_send` (dropped on Full). See `@plans/ingest/backpressure-watchdog.md`.

**Liveness watchdog:** DbWriter stamps `IngestHeartbeat` at the end of every `flush()`. A dedicated OS thread force-exits (`exit(1)`) when the heartbeat is stale AND the DB queue is non-empty AND live mode is on. See `@plans/ingest/backpressure-watchdog.md`.

## Files — `backend/src/ingest_laserstream/`

| File | Responsibility |
| --- | --- |
| `client.rs` | gRPC producer: TLS auth, reconnect w/ backoff (max 30s), replay from `last_slot`, idle-reconnect timer (10s), backpressure guard (`send_timeout` 30s) |
| `adapter.rs` | Protobuf → Helius JSON blob (`build_raw_blob`, runs off-thread in DbWriter) |
| `adapter_rpc.rs` | RPC result → protobuf (inverse of `adapter.rs`; used by token_sync) |
| `pipeline.rs` | Hot path: decode → filter → TokenCache update → fan-out to DB/strategy/SSE. Pings `TradeSignals` mint lane after cache update |
| `db_writer.rs` | Batches (256 ops / 25ms), dedups, persists; stamps `IngestHeartbeat` each flush; signals `TradeSignals` per `(wallet,mint)` |
| `maintenance.rs` | Every 6h: ensure today+2 future daily partitions, drop everything past `KEEP_DAYS` for `raw_transactions` + `trades` |
| `mod.rs` | re-exports + `proto` |

### `DbWriteOp` variants

`Raw(RawBlobJob)` · `Token(Token)` · `Wallet(String)` · `Trade(Trade)` · `Metrics(TokenMetricsWrite)` · `Migration{mint}`

### Pipeline event handlers

`on_token_created` (Token+Wallet+Metrics+ping+SSE) · `on_trade_executed` (Trade+Wallet+Metrics+reserves+AMM prewarm+ping+SSE) · `on_token_migrated` (register pool+Migration+ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only)

## Decoder — `decoder/`

Protobuf-native only (the old JSON/Value path has been deleted). Both live ingest and token_sync feed `decode_protobuf`.

| File | Responsibility |
| --- | --- |
| `grpc/mod.rs` | `decode_protobuf` / `decode_relevant_pb` — two entry points, one decode body; `LazyKeys` (base58 on demand) |
| `grpc/trade.rs` | Protobuf-native trade helpers |
| `mod.rs` (root) | `HeliusDecoder`, `TxRelevance`, module wiring |
| `trade.rs` (root) | Borsh `RawTradeEvent`, `build_amm_trade`, `compute_sol_change` |
| `instructions.rs` (root) | `InstructionKind`, `determine_instruction_type`, `label_instruction` |
| `create.rs` (root) | `decode_create_events_from_logs`, creator-wallet precedence |

Codegen: committed prost/tonic bindings in `generated/`; `.proto` sources in `proto/`. Regen only when `.proto` changes.

## Key rules

- `trades` table = this feed. TPSL exit loop confirms fills from this feed (not a separate RPC).
- No blocking I/O / `.await`-on-lock / unbounded alloc per event in `pipeline.rs`; DB+SSE go through channels.
