# Ingest — LaserStream (the sole live transport)

File-level map of `backend/src/ingest_laserstream/`. Helius LaserStream (Yellowstone gRPC) is the **only** live ingest path (WebSocket transport removed).
Logic explainer: `@project_plans/ingest/laserstream-workflow.md`, `@project_plans/ingest/token_sync-workflow.md`.

## Data flow
```
client.rs (gRPC)  --SubscribeUpdateTransaction-->  adapter.rs  --serde_json::Value-->
pipeline.rs (decode + TokenCache update + fan-out)  -->  { db_tx, strategy_tx, sse_tx }
db_writer.rs  --batched-->  Postgres  -->  TradeSignals.notify(wallet,mint)
```
Channels: `value_tx` cap 16 (`Value`) · `db_tx` cap 4096 (`DbWriteOp`) · `strategy_tx` cap 512 (`StrategyPing`) · `sse_tx` broadcast (`SseEvent`). Pipeline enqueues with `send().await` (real backpressure — never silently drops).

## Files
| File | Key items | Responsibility |
|---|---|---|
| `client.rs` | `LaserStreamClient`, `XTokenInterceptor`, `connect()`, `build_subscribe_request()`, `run()`, `run_once()` | gRPC producer: TLS+`x-token` auth, reconnect w/ backoff (max 30s), replay from `last_slot`, re-subscribe on `pools_changed` (250ms debounce). 64MB max msg. |
| `adapter.rs` | `update_tx_to_value()`, `compiled_ix()`, `inner_ix()`, `token_balances()` | Protobuf → Helius JSON shape; pre-filters on pump + AMM program logs. Account-key order: static ++ loaded_writable ++ loaded_readonly. |
| `pipeline.rs` | `IngestPipeline` (`new`, `pool_index`, `pools_changed`, `channel_pair`, `run`), `run_pool_subscription_refresh()` | Hot path: decode → filter (Mayhem/post-migration policy via `settings_rx`) → update `TokenCache` → fan out to DB/strategy/SSE. Feeds trader live reserves; pre-warms AMM pool on first AMM trade. Registers/evicts pools in `pool_index`. |
| `db_writer.rs` | `DbWriter` (`new`,`run`,`flush`), `DbWriteOp` enum, `TokenMetricsWrite` | Batches (64 ops / 25ms), dedups, persists; signals `TradeSignals` per `(wallet,mint)` after trades land. Metrics upsert bounded to 8 concurrent. |
| `maintenance.rs` | `run_partition_maintenance()` | Every 6h: ensure current+2 future weekly `raw_transactions` partitions; drop > ~9 weeks. |
| `profile.rs` | `start()`, `record_adapter()`, `record_decode()` | Opt-in (`INGEST_PROFILE=1`) cumulative timing of the Value build (`update_tx_to_value`) vs `decode_result`; logs `target: ingest_profile` every 5000 decoded txs. Zero cost when off. |
| `mod.rs` | re-exports + `proto` | module wiring |

### `DbWriteOp` variants
`Raw(Arc<RawTransaction>)` · `Token(Token)` · `Wallet(String)` (touch last_seen) · `Trade(Trade)` · `Metrics(TokenMetricsWrite)` · `Migration{mint}`.

### Pipeline event handlers
`on_token_created` (Token+Wallet+Metrics+ping+SSE) · `on_trade_executed` (Trade+Wallet+Metrics + feed trader reserves + pre-warm AMM + ping + SSE) · `on_token_migrated` (register pool + Migration op + ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only).

> `on_trade_executed` moves `Trade.instruction_labels` **out** of the in-memory Trade (leaving `Null`) and attaches it only to the cloned DB copy — so the per-trade label JSON array is deep-copied zero times (the DB copy keeps the labels for the trades API; the capped in-memory ring and strategy/exit logic read only Token-level labels). The trade is then applied to `TokenState`, the rugged-recompute throttle decided, and the first-AMM-trade pool-prewarm check-and-set resolved — all under a **single** `get_mut` guard (one shard write-lock per trade, not two).
>
> **Rugged-recompute throttle:** `recompute_rugged` is flagged at most once per `RUGGED_RECHECK_INTERVAL_SECONDS` (5 min) per mint via `TokenState.last_rugged_check_at`, not on every trade. The DbWriter only runs `compute_is_rugged` (up to 3 whole-history aggregate scans) when the flag is set, so a stale-but-still-trading mint no longer re-scans on every trade — the verdict only moves on the 1h `RUGGED_STALE_SECONDS` scale anyway.

## Decoder — `decoder/`
| File | Key items |
|---|---|
| `mod.rs` | `HeliusDecoder` (`new`, `with_pool_index`, `decode_result`, `decode_pump_swap_result`, `decode_amm_live`), `DecodeOutput{Transaction,Ignored}` |
| `parse.rs` | `extract_logs`, `extract_account_keys`, `find_pump_ixs_anywhere`, `is_pump_create_ix`, `resolve_pump_accounts`, `compute_sol/token_change`, `extract_balances` |
| `instructions.rs` | `InstructionKind{Create,Buy,Sell,Migrate,Unknown}`, `prepare_instructions`, `collect_instruction_kinds`, `determine_instruction_type`, `build_instruction_labels` |
| `trade.rs` | `decode_trade_events_from_logs/_inner_ixs`, `decode_pump_swap_trades_from_logs`, `build_amm_trade`, `DecodedTradeEvent`, `DecodedAmmTrade` (Borsh layouts; lamports→SOL normalize) |
| `create.rs` | `decode_create_events_from_logs`, `decode_create`, creator-wallet precedence (CreateEvent.creator → ix arg → signer idx) |

## Codegen — `generated/` + `proto/`
- Committed prost/tonic bindings (**no build-time `protoc`**): `generated/geyser.rs`, `generated/solana.storage.confirmed_block.rs`, `generated/mod.rs`.
- `.proto` sources in `proto/` (`geyser.proto` w/ local `from_slot=11`, `solana-storage.proto`). Regen only when `.proto` changes — see `proto/README.md` (Docker-based).

## Notes for edits
- **`trades` table = this feed.** TPSL exit loop confirms fills by polling `trades` (gRPC feed), not a separate RPC — account for index lag.
- No blocking I/O / `.await`-on-lock / unbounded alloc per event in `pipeline.rs`; DB+SSE go through channels, never inline.
