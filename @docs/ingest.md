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

> **Tier B — protobuf-native ingest decode.** The hot path no longer builds the heap-heavy Helius `Value` per tx. `client.rs` only **pre-filters** (a cheap `log_messages` scan) and forwards the typed protobuf (shared via `Arc`); `pipeline.rs` decodes it directly with `decoder::grpc::decode_protobuf`; the persisted Helius-shaped blob is synthesised **off-thread in the DbWriter** (`adapter::build_raw_blob`), only for txs we actually persist. The `Value` decode path (`adapter::update_tx_to_value` → `decode_result`) is **retained for token_sync** (RPC + replay), so the decoder is forked — a fix usually belongs in both `decode_protobuf` and `decode_result` (like the `tpsl_sniper_{1,2}` clones).

## Files
| File | Key items | Responsibility |
|---|---|---|
| `client.rs` | `LaserStreamClient`, `XTokenInterceptor`, `connect()`, `build_subscribe_request()`, `run()`, `run_once()`, `tx_is_relevant()` | gRPC producer: TLS+`x-token` auth, reconnect w/ backoff (max 30s), replay from `last_slot`, re-subscribe on `pools_changed` (250ms debounce). 64MB max msg. Per-tx work is now only the cheap `tx_is_relevant` log pre-filter; forwards `Arc<SubscribeUpdateTransaction>` (no `Value` build). |
| `adapter.rs` | `update_tx_to_value()`, `build_raw_blob()`, `compiled_ix()`, `inner_ix()`, `token_balances()` | Protobuf → Helius JSON blob. `build_raw_blob` = the synthesis (run off-thread in the DbWriter for persisted txs); `update_tx_to_value` = `build_raw_blob` gated by the log pre-filter (used by the cold replay path, which needs a decodable `Value`). Account-key order: static ++ loaded_writable ++ loaded_readonly. |
| `pipeline.rs` | `IngestPipeline` (`new`, `pool_index`, `pools_changed`, `channel_pair`, `run`), `run_pool_subscription_refresh()` | Hot path: stamp ingest `received_at` → `decode_protobuf` → filter (Mayhem/post-migration policy via `settings_rx`) → update `TokenCache` → fan out to DB/strategy/SSE. On `save_raw`, enqueues `DbWriteOp::Raw(RawBlobJob{ Arc<update>, signature, slot, block_time, received_at })`. Feeds trader live reserves; pre-warms AMM pool on first AMM trade. Registers/evicts pools in `pool_index`. |
| `db_writer.rs` | `DbWriter` (`new`,`run`,`flush`), `DbWriteOp` enum, `RawBlobJob`, `TokenMetricsWrite` | Batches (64 ops / 25ms), dedups, persists; **synthesises each raw blob via `build_raw_blob` here (off the ingest hot path)** with the ingest-time `received_at`. Signals `TradeSignals` per `(wallet,mint)` after trades land. Metrics upsert bounded to 8 concurrent. |
| `maintenance.rs` | `run_partition_maintenance()` | Every 6h: ensure current+2 future weekly `raw_transactions` partitions; drop > ~9 weeks. |
| `profile.rs` | `start()`, `record_adapter()`, `record_decode()` | Opt-in (`INGEST_PROFILE=1`) cumulative timing. Post-Tier-B the "adapter" stage times only the `tx_is_relevant` pre-filter (`value_build_avg` → ~0, the DoD signal); "decode" times `decode_protobuf`. Logs `target: ingest_profile` every 5000 decoded txs. Zero cost when off. |
| `live_parity.rs` | `live_parity_soak` (`#[cfg(test)]`) | **Opt-in `--ignored` soak** (successor to the retired in-hot-path `parity.rs`). Drives the real `client::run` ingest-only (no DbWriter/StrategyRunner/trader), decodes every live tx BOTH ways (`decode_protobuf` vs `update_tx_to_value`→`decode_result`), asserts zero money-field divergence + reports per-path timing. Confirmed clean (0 mismatches / 4200 live curve txs) at Tier B landing. Run: `cargo test --bin backend -- --ignored --nocapture live_parity_soak`. |
| `mod.rs` | re-exports + `proto` | module wiring |

### `DbWriteOp` variants
`Raw(RawBlobJob)` (typed protobuf + ingest scalars; blob built at flush) · `Token(Token)` · `Wallet(String)` (touch last_seen) · `Trade(Trade)` · `Metrics(TokenMetricsWrite)` · `Migration{mint}`.

### Pipeline event handlers
`on_token_created` (Token+Wallet+Metrics+ping+SSE) · `on_trade_executed` (Trade+Wallet+Metrics + feed trader reserves + pre-warm AMM + ping + SSE) · `on_token_migrated` (register pool + Migration op + ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only).

> `on_trade_executed` moves `Trade.instruction_labels` **out** of the in-memory Trade (leaving `Null`) and attaches it only to the cloned DB copy — so the per-trade label JSON array is deep-copied zero times (the DB copy keeps the labels for the trades API; the capped in-memory ring and strategy/exit logic read only Token-level labels). The trade is then applied to `TokenState`, the rugged-recompute throttle decided, and the first-AMM-trade pool-prewarm check-and-set resolved — all under a **single** `get_mut` guard (one shard write-lock per trade, not two).
>
> **Rugged-recompute throttle:** `recompute_rugged` is flagged at most once per `RUGGED_RECHECK_INTERVAL_SECONDS` (5 min) per mint via `TokenState.last_rugged_check_at`, not on every trade. The DbWriter only runs `compute_is_rugged` (up to 3 whole-history aggregate scans) when the flag is set, so a stale-but-still-trading mint no longer re-scans on every trade — the verdict only moves on the 1h `RUGGED_STALE_SECONDS` scale anyway.

## Decoder — `decoder/` (two parity-tested paths: `grpc/` hot path + `json/` for token_sync; shared leaves at root)

Layout: the two orchestrations live in their own folders (`grpc/`, `json/`); everything source-agnostic stays at the `decoder/` root and is shared by both.

| File | Key items |
|---|---|
| `mod.rs` (root, shared) | `HeliusDecoder` (`new`, `with_pool_index`, `decode_migrate`), `DecodeOutput{Transaction,Ignored}`, module wiring. `decode_protobuf` lives in `grpc/`, `decode_result`/`decode_pump_swap_result`/`decode_amm_live` in `json/`, but all are `HeliusDecoder` methods (call sites unchanged). |
| `trade.rs` (root, shared) | `decode_trade_events_from_logs`, `decode_pump_swap_trades_from_logs`, `build_amm_trade`, `compute_sol_change`; the Borsh `RawTradeEvent`/`DecodedTradeEvent` (+`from_raw`) machinery and `DecodedAmmTrade` reused by both inner-ix decoders (lamports→SOL normalize) |
| `instructions.rs` (root, shared) | `InstructionKind{Create,Buy,Sell,Migrate,Unknown}`, `determine_instruction_type`, and the leaves `classify_pump_ix`, `extract_compute_budget`, `label_instruction(program_id, parsed_type, data)` (both paths; protobuf passes `parsed_type=None`) |
| `create.rs` (root, shared) | `decode_create_events_from_logs`, `decode_create` (source-agnostic: takes raw `create_data` + `pump_ix_datas` bytes), creator-wallet precedence (CreateEvent.creator → ix arg → signer idx) |
| `grpc/mod.rs` | **Tier B protobuf-native live decode.** `decode_protobuf(&SubscribeUpdateTransaction, received_at)` mirrors `json`'s `decode_result` orchestration but reads the protobuf directly (no `Value`, no per-ix base58 round-trip); `PbIx` (borrowed program-id + `accounts`/`data` bytes) + protobuf-native key/ix/label/account helpers + `decode_amm_live_pb`. Null-data raw-tx carrier (events embed but never read `raw_tx`). `#[cfg(test)]` parity tests decode synthetic txs both ways and assert identical events. |
| `grpc/trade.rs` | protobuf-native trade helpers: `decode_trade_events_from_inner_pb`, `compute_token_change_pb`, `decode_trade_from_balances_pb` (typed `scb::TokenBalance` lists) |
| `json/mod.rs` | **`Value` decode path.** `decode_result`, `decode_pump_swap_result`, `decode_amm_live` (Helius `jsonParsed`) |
| `json/parse.rs` | `extract_logs`, `extract_account_keys`, `find_pump_ixs_anywhere`, `is_pump_create_ix`, `resolve_pump_accounts`, `compute_token_change`, `extract_balances` (`Value` only) |
| `json/instructions.rs` | `Value` instruction adapters: `prepare_instructions`, `collect_instruction_kinds`, `resolve_instruction_program_id`, `instruction_data_bytes`, `build_instruction_labels` |
| `json/trade.rs` | `Value` trade helpers: `decode_trade_events_from_inner_ixs`, `decode_trade_from_balances` |

> **Fork boundary.** `grpc/` serves the live gRPC path (curve **and** AMM-live). `json/` (`decode_result` + `decode_pump_swap_result` + `decode_amm_live`) stays for `services::token_sync` (RPC + gRPC replay, which still go through `adapter::update_tx_to_value`). The root leaves (Borsh decoders incl. `RawTradeEvent`/`from_raw`, `classify_pump_ix`, `label_instruction`, `determine_instruction_type`, `build_amm_trade`, `compute_sol_change`, `decode_create`, `decode_migrate`) are shared, so only the **source of the bytes** differs between the two orchestrations.

## Codegen — `generated/` + `proto/`
- Committed prost/tonic bindings (**no build-time `protoc`**): `generated/geyser.rs`, `generated/solana.storage.confirmed_block.rs`, `generated/mod.rs`.
- `.proto` sources in `proto/` (`geyser.proto` w/ local `from_slot=11`, `solana-storage.proto`). Regen only when `.proto` changes — see `proto/README.md` (Docker-based).

## Notes for edits
- **`trades` table = this feed.** TPSL exit loop confirms fills by polling `trades` (gRPC feed), not a separate RPC — account for index lag.
- No blocking I/O / `.await`-on-lock / unbounded alloc per event in `pipeline.rs`; DB+SSE go through channels, never inline.
