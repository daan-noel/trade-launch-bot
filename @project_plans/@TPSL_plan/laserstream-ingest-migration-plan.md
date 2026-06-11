# LaserStream (Yellowstone gRPC) Ingest Migration Plan

> Goal: add a LaserStream **gRPC** transport for lower latency and reduced Helius
> cost, while keeping the existing pipeline / decoder-downstream / strategy flow
> intact. WS stays as a toggleable fallback.

## Context — current state

- `backend/src/ingest/helius_ws.rs` opens a WS (`tokio_tungstenite`) to Helius
  **Atlas**, sends `transactionSubscribe` (`jsonParsed`), and forwards raw JSON
  frames into an `mpsc::Sender<String>` channel.
- `backend/src/ingest/pipeline.rs` consumes raw frames → `decoder.decode()` →
  `filter_events` → `db_writer` + `ping_strategy` + SSE.
- Subscriptions: static on the pump.fun **program** (curve firehose) + **dynamic
  per-pool** subs for migrated *tracked* tokens (chunked 90 accounts / sub).
- The decoder is tightly coupled to Helius `jsonParsed`: it relies on Helius's
  `parsed/type` for System / SPL-Token / ComputeBudget / ATA instructions and on
  the `jsonParsed` accountKeys shape. **Trade events themselves are decoded from
  log messages** (`decode_trade_events_from_logs`) — transport-agnostic.
- `raw_transactions` stores the full `jsonParsed` JSON. It is **write-only**
  (`find_by_signature` / `exists` are `#[allow(dead_code)]`, zero callers) — kept
  only for later analysis.

## Decisions (locked)

- **Keep the firehose.** Track all new tokens (created after server start) +
  DB-seeded tokens; track AMM only for tracked tokens that migrate (existing
  per-pool model — unchanged). No pump_amm program firehose.
- **New folder**, fittable with the current workflow.
- **gRPC raw data saved SEPARATELY** from the existing WS `raw_transactions`
  (its own table/store).
- **Keep WS** as a toggleable alternative (`INGEST_TRANSPORT=ws|laserstream`);
  may remove later after validation.
- **LaserStream region/endpoint: TBD** (fill in later). API key already has the
  LaserStream plan; same `HELIUS_API_KEY` authenticates gRPC via `x-token`.

## Target architecture

### New folder (sibling to `ingest/`)
`backend/src/ingest_laserstream/`
- `mod.rs` — spawn entry point (mirrors `helius_ws::run` signature).
- `client.rs` — gRPC connect / stream / reconnect.
- `subscribe.rs` — `SubscribeRequest` builder (curve + dynamic pools).
- `decode.rs` — protobuf → `Vec<InternalEvent>` (+ `RawTransaction`).

### Convergence point (what makes it fittable)
- Refactor `pipeline.rs` to extract its "process one decoded tx" half
  (`sort_events` → `filter_events` → `save_raw` → `apply_event`) into a shared fn
  taking `(RawTransaction, Vec<InternalEvent>)`.
- Both transports feed that fn. **Everything downstream is untouched:**
  `filter_events`, `db_writer`, `pool_index`, `pools_changed`, `ping_strategy`,
  SSE, strategies.

### Transport — `client.rs`
- `yellowstone-grpc-client` + `yellowstone-grpc-proto` over `tonic`.
- Connect to the regional LaserStream gRPC endpoint; auth `x-token = HELIUS_API_KEY`.
- One long-lived bidi stream; commitment `processed`; keepalive.
- Same inputs as `helius_ws::run`: `live_rx` (pause), `pool_index`, `pools_changed`.

### Subscription — `subscribe.rs`
- Protobuf `SubscribeRequest` with a `transactions` filter:
  `account_include = [pump_fun_program] + active migrated-pool accounts`,
  `failed = false`.
- **Dynamic pools** = send an updated `SubscribeRequest` on the *same* stream
  (replaces per-pool WS messages + the 90/sub chunking). `pools_changed` →
  rebuild & resend. The "AMM only for migrated tracked tokens" logic is unchanged.

### Decoder — `decode.rs`
- Input = `SubscribeUpdateTransaction` (raw instruction bytes, raw pubkeys,
  `meta.log_messages`, inner ixs, pre/post balances).
- **Reuse as-is:** the log-based core (`decode_trade_events_from_logs`,
  create-from-logs) — gRPC delivers `log_messages` as strings.
- **Rewrite (the one real cost):** the `parsed/type`-dependent parts — CU
  price/limit + System / SPL-Token / ATA / ComputeBudget classification
  (`decoder/instructions.rs:289-298`) — decoded from raw bytes instead.

### Backfill (#1) — absorbed
- Track `last_processed_slot`; on reconnect resubscribe with
  `from_slot = last_processed_slot` → LaserStream **replays the gap**, replacing
  most of `token_sync`'s `getTransaction` backfill calls.

### Raw persistence (gRPC) — separate store
- New **separate** table (e.g. `raw_transactions_grpc`), decoupled from the WS
  `raw_transactions`, so the two raw formats never co-mingle while both transports
  run.
- Format: recommend **re-serialized JSON** (keeps analysis workflow consistent);
  protobuf **bytes** is the smaller alternative if storage size dominates —
  decide at implementation.
- Apply the Part A storage strategy below to this table from day one.

### Config / deps
- Add `HELIUS_LASERSTREAM_URL` (regional, pin closest to the bot) + reuse
  `HELIUS_API_KEY`.
- Add `INGEST_TRANSPORT=ws|laserstream` toggle (default `ws` until validated).
- Cargo: `yellowstone-grpc-client`, `yellowstone-grpc-proto`, `tonic`, `prost`.

### Rollout
- Startup spawns WS *or* LaserStream based on the toggle; downstream identical.
- Validate gRPC by A/B comparing decoded trades for the same tokens vs WS, then
  flip the default. Keep WS as fallback; remove later if desired.

## Part A (related): raw storage at scale

- Partition the raw table(s) by time — **native declarative range partitioning**
  (monthly), not hand-named tables. App keeps inserting into one logical table.
- Enable `lz4` TOAST compression (PG14+) on the JSON/blob column.
- Retention horizon via `pg_partman` or a monthly cron; dropping old data =
  `DROP TABLE`/`DETACH PARTITION` (instant), never a giant `DELETE`.
- Optionally move the raw table(s) to their own DB/instance later for IO/backup
  isolation (it has no FKs to the trading tables).

## Adjacent latency work (do regardless of transport)

- **Co-locate** the bot in the same region as the LaserStream endpoint (network
  RTT often dominates).
- **Hot-path audit:** ensure nothing blocking sits between "tx decoded" and
  `ping_strategy` (`on_trade_executed`); DbWriter already async.
- **Submission latency** (separate topic): already using `HELIUS_FAST_SENDER_URL`;
  consider staked/SWQOS sender or Jito bundles.

## Effort / risk

- **Main cost:** decoder rewrite for protobuf (the `parsed/type` parts). Same work
  the deferred `jsonParsed → base64` item would need — done once, here, for the
  bigger payoff.
- **Low risk to downstream:** pipeline/db/strategy untouched; WS fallback retained.

## Open items

- LaserStream region + gRPC endpoint URL.
- Final gRPC raw persisted format (JSON vs protobuf bytes).
- Whether to retire WS after validation.
