# Ingest — LaserStream (the sole live transport)

File-level map of `backend/src/ingest_laserstream/`. Helius LaserStream (Yellowstone gRPC) is the **only** live ingest path (WebSocket transport removed).
Logic explainer: `@project_plans/ingest/laserstream-workflow.md`, `@project_plans/ingest/token_sync-workflow.md`.

## Data flow
```
client.rs (gRPC, cheap log pre-filter)  --Arc<SubscribeUpdateTransaction>-->
pipeline.rs (decode_protobuf + TokenCache update + fan-out)  -->  { db_tx, strategy_tx, sse_tx }
db_writer.rs (builds the Helius blob off-thread via build_raw_blob)  --batched-->  Postgres  -->  TradeSignals.notify(wallet,mint)
```
Channels: `update_tx` cap 1024 (`Arc<SubscribeUpdateTransaction>`) · `db_tx` cap 4096 (`DbWriteOp`) · `strategy_tx` cap 512 (`StrategyPing`) · `sse_tx` broadcast (`SseEvent`). Pipeline enqueues with `send().await` (real backpressure — never silently drops).

> **Tier B — protobuf-native ingest decode.** The hot path no longer builds the heap-heavy Helius `Value` per tx. `client.rs` only **pre-filters** (a cheap `log_messages` scan) and forwards the typed protobuf (shared via `Arc`); `pipeline.rs` decodes it directly with `decoder::grpc::decode_protobuf`; the persisted Helius-shaped blob is synthesised **off-thread in the DbWriter** (`adapter::build_raw_blob`), only for txs we actually persist. `decode_protobuf` is now the **sole** decoder: token_sync (RPC + replay) lowers its base64 results to `SubscribeUpdateTransaction` via `adapter_rpc::rpc_to_protobuf` and decodes through the same path — the old `Value` decode path (`update_tx_to_value` → `decode_result`, `decoder/json/`) has been deleted.

## Files
| File | Key items | Responsibility |
|---|---|---|
| `client.rs` | `LaserStreamClient`, `XTokenInterceptor`, `connect()`, `build_subscribe_request()`, `run()`, `run_once()`, `tx_is_relevant()` | gRPC producer: TLS+`x-token` auth, reconnect w/ backoff (max 30s), replay from `last_slot`, re-subscribe on `pools_changed` (250ms debounce). 64MB max msg. Per-tx work is now only the cheap `tx_is_relevant` log pre-filter; forwards `Arc<SubscribeUpdateTransaction>` (no `Value` build). |
| `adapter.rs` | `build_raw_blob()`, `compiled_ix()`, `inner_ix()`, `token_balances()` | Protobuf → Helius JSON blob (the persisted `raw_data`). `build_raw_blob` runs off-thread in the DbWriter for real-time (`source='live'`) txs and inline in token_sync for backfilled (`source='sync'`) txs, so blobs are byte-consistent across sources. Account-key order: static ++ loaded_writable ++ loaded_readonly. |
| `adapter_rpc.rs` | `rpc_to_protobuf()`, `meta_from_json()` | **Inverse of `adapter.rs`** (RPC → protobuf): how token_sync runs the protobuf decoder instead of the `Value` path. Lowers an `encoding="base64"` `getTransaction`/gTFA result (`wrap_transaction_result` shape) to `SubscribeUpdateTransaction`: base64 → `bincode` `VersionedTransaction` → `scb::Message`; JSON `meta` → `scb::TransactionStatusMeta` (incl. `loadedAddresses`). Base64 (not `jsonParsed`) is required — jsonParsed pre-parses Compute-Budget/System/Token ixs, dropping the raw `data` `decode_protobuf` needs. Wired into `services/token_sync.rs` (all RPC fetches `encoding=base64`; replay supplies native protobuf); verified live by the `--ignored` `gtfa_base64_decodes_via_protobuf` harness. The old `Value` decode path (`decoder/json/`, `decode_result`, `update_tx_to_value`) has been **deleted** — `decode_protobuf` is now the sole decoder. |
| `pipeline.rs` | `IngestPipeline` (`new`, `pool_index`, `pools_changed`, `channel_pair`, `run`), `run_pool_subscription_refresh()` | Hot path: stamp ingest `received_at` → `decode_protobuf` → filter (Mayhem/post-migration policy via `settings_rx`) → update `TokenCache` → fan out to DB/strategy/SSE. On `save_raw` **and** `settings_rx.persist_raw` (the `ingest.persist_raw` toggle; off skips raw persistence to curb DB growth), enqueues `DbWriteOp::Raw(RawBlobJob{ Arc<update>, signature, slot, block_time, received_at })`. Feeds trader live reserves; pre-warms AMM pool on first AMM trade. Pings the `TradeSignals` **mint lane** (`notify_mint`) right after the cache update, so the TPSL2 scalp-entry arming wakes on the trade instead of a fixed timer. Registers/evicts pools in `pool_index`. |
| `db_writer.rs` | `DbWriter` (`new`,`run`,`flush`), `DbWriteOp` enum, `RawBlobJob`, `TokenMetricsWrite` | Batches (64 ops / 25ms), dedups, persists; **synthesises each raw blob via `build_raw_blob` here (off the ingest hot path)** with the ingest-time `received_at`. Signals `TradeSignals` per `(wallet,mint)` after trades land. Metrics upsert bounded to 8 concurrent. |
| `maintenance.rs` | `run_partition_maintenance()` | Every 6h: ensure current+2 future weekly partitions and drop > `KEEP_WEEKS` (5) for **both** `raw_transactions` (on `received_at`) and `trades` (on `block_time`, migration 0002). |
| `mod.rs` | re-exports + `proto` | module wiring |

### `DbWriteOp` variants
`Raw(RawBlobJob)` (typed protobuf + ingest scalars; blob built at flush) · `Token(Token)` · `Wallet(String)` (touch last_seen) · `Trade(Trade)` · `Metrics(TokenMetricsWrite)` · `Migration{mint}`.

### Pipeline event handlers
`on_token_created` (Token+Wallet+Metrics+ping+SSE) · `on_trade_executed` (Trade+Wallet+Metrics + feed trader reserves + pre-warm AMM + ping + SSE) · `on_token_migrated` (register pool + Migration op + ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only).

> `on_trade_executed` moves `Trade.instruction_labels` **out** of the in-memory Trade (leaving `Null`) and attaches it only to the cloned DB copy — so the per-trade label JSON array is deep-copied zero times (the DB copy keeps the labels for the trades API; the capped in-memory ring and strategy/exit logic read only Token-level labels). The trade is then applied to `TokenState`, the rugged-recompute throttle decided, and the first-AMM-trade pool-prewarm check-and-set resolved — all under a **single** `get_mut` guard (one shard write-lock per trade, not two).
>
> **Rugged-recompute throttle:** `recompute_rugged` is flagged at most once per `RUGGED_RECHECK_INTERVAL_SECONDS` (5 min) per mint via `TokenState.last_rugged_check_at`, not on every trade. The DbWriter only runs `compute_is_rugged` (up to 3 whole-history aggregate scans) when the flag is set, so a stale-but-still-trading mint no longer re-scans on every trade — the verdict only moves on the 1h `RUGGED_STALE_SECONDS` scale anyway.

## Decoder — `decoder/` (single protobuf path: `grpc/`; shared leaves at root)

Layout: the `decode_protobuf` orchestration lives in `grpc/`; everything source-agnostic stays at the `decoder/` root. Both live ingest and token_sync feed `decode_protobuf` — token_sync lowers its base64 RPC results to `SubscribeUpdateTransaction` first (see `adapter_rpc.rs`).

| File | Key items |
|---|---|
| `mod.rs` (root, shared) | `HeliusDecoder` (`new`, `with_pool_index`, `decode_migrate`), `DecodeOutput{Transaction,Ignored}`, module wiring. `decode_protobuf` lives in `grpc/` but is a `HeliusDecoder` method (call sites unchanged). |
| `trade.rs` (root, shared) | `decode_trade_events_from_logs`, `decode_pump_swap_trades_from_logs`, `build_amm_trade`, `compute_sol_change`; the Borsh `RawTradeEvent`/`DecodedTradeEvent` (+`from_raw`) machinery and `DecodedAmmTrade` reused by both inner-ix decoders (lamports→SOL normalize) |
| `instructions.rs` (root, shared) | `InstructionKind{Create,Buy,Sell,Migrate,Unknown}`, `determine_instruction_type`, and the leaves `classify_pump_ix`, `extract_compute_budget`, `label_instruction(program_id, parsed_type, data)` (protobuf passes `parsed_type=None`) |
| `create.rs` (root, shared) | `decode_create_events_from_logs`, `decode_create` (source-agnostic: takes raw `create_data` + `pump_ix_datas` bytes), creator-wallet precedence (CreateEvent.creator → ix arg → signer idx) |
| `grpc/mod.rs` | **The protobuf-native decoder.** `decode_protobuf(&SubscribeUpdateTransaction, received_at)` reads the protobuf directly (no `Value`, no per-ix base58 round-trip); `LazyKeys` holds the account-key list as raw 32-byte slices and base58-encodes per index only on demand (memoized) — program-ids are matched on raw bytes, so the common log-derived trade (mint/user come from the event payload) encodes almost no keys, and only the rare create/balance paths materialize the full list; `PbIx` (program-id *index* + borrowed `accounts`/`data` bytes) + protobuf-native key/ix/label/account helpers + `decode_amm_live_pb` (AMM, pool resolved via the shared index). Null-data raw-tx carrier (events embed but never read `raw_tx`; the blob is built by `build_raw_blob`). `#[cfg(test)]` unit tests decode synthetic txs and assert the expected events. |
| `grpc/trade.rs` | protobuf-native trade helpers: `decode_trade_events_from_inner_pb`, `compute_token_change_pb`, `decode_trade_from_balances_pb` (typed `scb::TokenBalance` lists) |

> **Shared leaves.** The root leaves (Borsh decoders incl. `RawTradeEvent`/`from_raw`, `classify_pump_ix`, `label_instruction`, `determine_instruction_type`, `build_amm_trade`, `compute_sol_change`, `decode_create`, `decode_migrate`) are source-agnostic, consumed by `grpc/` for both curve and AMM-live decode and (via the same `decode_protobuf` call) by token_sync.

## Codegen — `generated/` + `proto/`
- Committed prost/tonic bindings (**no build-time `protoc`**): `generated/geyser.rs`, `generated/solana.storage.confirmed_block.rs`, `generated/mod.rs`.
- `.proto` sources in `proto/` (`geyser.proto` w/ local `from_slot=11`, `solana-storage.proto`). Regen only when `.proto` changes — see `proto/README.md` (Docker-based).

## Notes for edits
- **`trades` table = this feed.** TPSL exit loop confirms fills by polling `trades` (gRPC feed), not a separate RPC — account for index lag.
- No blocking I/O / `.await`-on-lock / unbounded alloc per event in `pipeline.rs`; DB+SSE go through channels, never inline.
