# LaserStream Ingest Workflow

LaserStream / Yellowstone gRPC is the **sole** live ingest transport (the old WebSocket path
was fully removed 2026-06-11). This doc describes how a transaction goes from the gRPC stream
to persisted trades, token metrics, and strategy pings.

Code lives in [backend/src/ingest_laserstream/](../../backend/src/ingest_laserstream/).

## Module layout

| File | Purpose |
|------|---------|
| [client.rs](../../backend/src/ingest_laserstream/client.rs) | gRPC connect, TLS + `x-token` auth, subscription, reconnect backoff, pool resubscribe; forwards the typed `Arc<SubscribeUpdateTransaction>` (no `Value` build) |
| [adapter.rs](../../backend/src/ingest_laserstream/adapter.rs) | Protobuf `SubscribeUpdateTransaction` → Helius-shaped JSON `Value` (`build_raw_blob` = persisted blob, off-thread in DbWriter; `update_tx_to_value` = that + pre-filter, for cold replay) |
| [pipeline.rs](../../backend/src/ingest_laserstream/pipeline.rs) | Main event loop: decode dispatch, cache updates, strategy pings, pool refresh |
| [db_writer.rs](../../backend/src/ingest_laserstream/db_writer.rs) | Batch queue, partition-by-type, dedup-keep-last, bulk inserts, signal notify; synthesises the raw blob via `build_raw_blob` off the hot path |
| [decoder/mod.rs](../../backend/src/ingest_laserstream/decoder/mod.rs) | `HeliusDecoder` API + `DecodeOutput`, `decode_migrate`; module wiring for the two forked paths |
| [decoder/grpc/](../../backend/src/ingest_laserstream/decoder/grpc/) | **Live hot path** — protobuf-native `decode_protobuf` (+ `grpc/trade.rs`), reads the Yellowstone structs directly, no `Value` |
| [decoder/json/](../../backend/src/ingest_laserstream/decoder/json/) | **token_sync (RPC + replay)** — `Value` path: `decode_result`/`decode_pump_swap_result`/`decode_amm_live`, plus `json/parse.rs` (account-key/balance/log/IX-walk extraction) + `json/instructions.rs` (Value ix adapters) + `json/trade.rs` |
| [decoder/trade.rs](../../backend/src/ingest_laserstream/decoder/trade.rs) | Shared: bonding-curve `TradeEvent` + PumpSwap `BuyEvent`/`SellEvent` Borsh decode, `build_amm_trade`, `compute_sol_change` |
| [decoder/create.rs](../../backend/src/ingest_laserstream/decoder/create.rs) | Shared: `Create`/`Create_v2` instruction + creator resolution (byte-source-agnostic) |
| [decoder/instructions.rs](../../backend/src/ingest_laserstream/decoder/instructions.rs) | Shared: instruction kinds/labels + compute-unit extraction (plain byte slices) |

The two decode paths are parity-tested siblings (a fix to a path-specific step usually belongs in both `grpc` and `json`); only the **byte source** differs — see [@docs/ingest.md](../../@docs/ingest.md) for the fork rationale.

## End-to-end data flow

```
LaserStream gRPC  ──Arc<protobuf>──▶  IngestPipeline  ──┬─DbWriteOp──▶  DbWriter ──▶ Postgres
(client, pre-filter)  (decode_protobuf + cache)         ├─StrategyPing─▶ Strategy runner
                                                └─SseEvent────▶ SSE /events (cold lane)
                                                                    │
                                          after commit: TradeSignals.notify(wallet,mint)
                                                                    │
                                                       wakes strategy buy/sell confirm
```

### 1. Stream (client + adapter)

- `connect(endpoint, api_key)` — TLS endpoint, `x-token` auth via `XTokenInterceptor`,
  64 MiB max message, 30 s HTTP/2 + TCP keepalive. ([client.rs](../../backend/src/ingest_laserstream/client.rs))
- `build_subscribe_request(account_include, from_slot)` — subscribes to the **Pump.fun program**
  plus currently-tracked **PumpSwap pool accounts**. Commitment = `processed`. On reconnect it
  replays from `from_slot` when recent enough, else falls back to a live subscription.
- `run(...)` main loop: `stream.message()` → `SubscribeUpdateTransaction` → cheap
  `tx_is_relevant(&tx)` log pre-filter → send the typed `Arc<SubscribeUpdateTransaction>` on
  `value_tx` (mpsc) to the pipeline. (Tier B: the heap-heavy Helius `Value` blob is no longer
  built here — `decode_protobuf` reads the protobuf directly and the persisted blob is built
  off-thread in the DbWriter via `adapter::build_raw_blob`.)
  - Highest slot tracked in an `AtomicU64`.
  - `pools_changed` notification coalesces pool-set changes (≈250 ms debounce) and re-subscribes
    **without** a full reconnect.
  - `live_rx` watch pauses the stream gracefully when live mode is toggled off.
- **Reconnect:** exponential backoff with 0–50% jitter (capped 30 s); resets to base once a
  connection delivers data. Jitter derives from subsecond nanos (no RNG).

### 2. Decode (`HeliusDecoder`)

`HeliusDecoder { pump_program_id, pool_index: Option<Arc<DashMap<pool, mint>>> }`.
Live gRPC path: `decode_protobuf(&SubscribeUpdateTransaction, received_at) -> DecodeOutput`
(`decoder/grpc/`) — protobuf-native, no `Value`. token_sync (RPC + replay) still uses
`decode_result(Value)`; both share the same byte-level leaves, so the decode semantics below
are identical for either entry point (Tier B fork — see `@docs/ingest.md`).

- **Bonding-curve trades** — preferred source is `Program data:` logs → base64 → Borsh
  `RawTradeEvent` (matched on `TRADE_EVENT_DISCRIMINATOR`). Fallback when logs are truncated:
  walk inner instructions for the pump.fun `emit_cpi!` self-CPI (never truncated by Solana).
  Lamports → SOL (÷1e9); token amounts stay raw.
- **PumpSwap (AMM) trades** — `BuyEvent`/`SellEvent` logs. Pool resolves to mint via the shared
  `pool_index`. **Reserve transformation:** events snapshot **pre-swap** reserves; the decoder
  rolls them forward to **post-swap** (this is the off-by-one fix recorded in
  [[amm-reserves-preswap-bug]]).
- **Token creation** — `Create`/`Create_v2` discriminators + `CreateEvent` logs. Creator resolved
  in priority order: on-chain `CreateEvent.creator` → instruction arg pubkey → user signer.
- **Migration** — `Migrate` IX; mint from `pump_accounts[2]` → `TokenMigratedEvent`.
- **Instruction labels + CU** — `build_instruction_labels` produces human-readable per-IX labels and
  extracts CU limit / CU price, stored as JSON on the `Trade`.
- **Dust filter** — `Trade::is_dust` drops trades below `MIN_TRADE_SOL` (≈10k lamports) before any DB row.

### 3. Pipeline (cache + dispatch)

`IngestPipeline::run(value_rx)` processes one `Arc<SubscribeUpdateTransaction>` at a time, calling
`decode_protobuf` (backpressure from the DB queue intentionally stalls gRPC reads to bound memory on
hot tokens). For each decoded `InternalEvent`:

- **TokenCreated** → enqueue `Token` + `Wallet` + `Metrics`; ping strategy; emit SSE.
- **TradeExecuted** → enqueue `Trade` + `Wallet` + `Metrics`; update live-reserves trader cache;
  prewarm AMM pool once per mint; ping strategy; emit SSE.
- **TokenMigrated** → `register_pool` (pool→mint index), notify `pools_changed` (wakes resubscribe),
  enqueue `Migration` + `Metrics`, ping strategy.
- **CreatorActivity** → ping strategy.

A separate task refreshes pool subscriptions on an interval: scans the token cache for migrated +
recently-active tokens, derives PumpSwap pool addresses, registers new pools (→ resubscribe via
`pools_changed`).

Runtime policy toggles read from a settings watch: `track_mayhem` gates Mayhem-mode tokens (see
[[mayhem-mode-supply-2x]]); `track_post_migration` gates AMM recording (clears/reseeds the live
pool set).

### 4. Persist (`DbWriter`)

`DbWriter { pool, trade_signals }`. Flush when the batch hits `BATCH_MAX = 64` ops **or** every
`FLUSH_INTERVAL_MS = 25` ms. Ops: `Raw`, `Token`, `Wallet`, `Trade`, `Metrics`, `Migration`.

**Partition-by-type then dedup-keep-last** (see [[ingest-dbwriter-batching]]):

- Trades → `HashMap<(tx_signature, leg_index), Trade>` (replay-idempotent).
- Metrics → `HashMap<mint, TokenMetricsWrite>` — **keep last; upsert is an absolute snapshot**
  (this invariant is what makes batching correct — do not make `upsert_metrics` relative).
- Wallets → `HashSet` (UPDATE touch). Migrations → `HashSet` (idempotent). Tokens/Raw → low-volume `Vec`.

**Commit order:** Tokens (FK first) → Raw txs → Wallets → Trades → **signals** → Metrics
(rugged recompute bounded by `METRIC_WRITE_CONCURRENCY = 8`) → Migrations.

Trade upsert conflict key `(tx_signature, leg_index)` updates only decoded columns
(`price_per_token`, the four reserve columns) and preserves `id` / `received_at`.

### 5. Signal (`TradeSignals` push-confirm)

`TradeSignals { slots: DashMap<(wallet, mint), Slot> }` (see [[buy-push-confirm]]). After trade rows
are committed and queryable, the DbWriter calls `notify(wallet, mint)` for each trade. The map stays
tiny — only `(wallet, mint)` pairs this bot is actively executing are watched, so the publish side
short-circuits for the vast majority of trades.

Strategy side: `register(wallet, mint)` → `notified()` future → await wake → re-read DB for the
filled trade. The old polling loop is kept as a fallback (never worse than poll-only). Guard drop
decrements waiters and removes empty slots (no leak).

## Key invariants

- **Single hot-path owner** — one tx at a time; DB backpressure throttles the stream by design.
- **Dedup-keep-last is replay-safe** — multiple trades for one `(tx_signature, leg_index)` in a flush
  coalesce deterministically.
- **`pool_index` is shared** (Arc<DashMap>) across decoder (resolve AMM swaps), pipeline (register
  migrations), and the resubscribe task.
- **Metrics upsert must stay an absolute snapshot** — batching correctness depends on it.

## Related

- [[laserstream-ingest-migration]] — WS removal history; ingest now keeps only decoder + `TokenMetricsWrite`, shared with token_sync.
- [[laserstream-vs-ws-latency]] — the two transports were effectively tied; removal was a reliability/cost call.
- [[token-trades-cap]] — `TokenState.trades` 50k cap + absolute exit cursor (money-path invariant).
- See also: [token_sync-workflow.md](./token_sync-workflow.md) (shares the decoder + `TokenMetricsWrite`).
