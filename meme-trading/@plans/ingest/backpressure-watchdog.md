# Ingest — Backpressure, Watchdog & Decoder Detail

Deep-dive on `ingest-laserstream/src/` internal mechanics. See [@arch/ingest.md](@arch/ingest.md) for the file-level map and channel caps.

## Backpressure — `pipeline.rs`

Two tiers of backpressure separate money-critical ops from recomputable ones:

**Tier A — blocking send (never dropped)**

```rust
db_tx.send(DbWriteOp::Trade(...)).await       // blocks pipeline until DB writer drains
db_tx.send(DbWriteOp::Token(...)).await
db_tx.send(DbWriteOp::Wallet(...)).await
db_tx.send(DbWriteOp::Migration { mint }).await
```

If `db_tx` (cap 16 384) is full, the pipeline task parks here. This is intentional: we prefer gRPC backpressure over dropping a trade.

**Tier B — try_send (dropped on full)**

```rust
db_tx.try_send(DbWriteOp::Metrics(...)).ok()   // best-effort, recomputable
db_tx.try_send(DbWriteOp::Raw(...)).ok()
```

`Metrics` can be recomputed from `trades`; `Raw` is low-value audit data. Dropping them under load is the correct trade-off.

**SSE fan-out** uses a `broadcast::Sender` with `send_if_there_are_receivers()`. No backpressure to the pipeline — lagging SSE clients fall behind or get `RecvError::Lagged`.

**Strategy channel** (`strategy_tx`, cap 512) uses `try_send` to the StrategyRunner — a slow strategy runner should not stall ingest.

## Watchdog — `ingest_health.rs`

`IngestHeartbeat` is a shared struct stamped by `db_writer.rs` at the end of every `flush()`:

```rust
pub struct IngestHeartbeat {
    pub last_flush_at: AtomicU64,   // Unix timestamp (secs)
    pub queue_len: AtomicUsize,     // snapshot of db_rx.len() at flush time
}
```

A dedicated **OS thread** (not tokio task — must not be blocked by a stalled runtime) polls every `WATCHDOG_INTERVAL_SECS`:

| `last_flush_at` staleness | `queue_len` | Live mode | Action |
|---|---|---|---|
| > `WATCHDOG_STALE_SECS` | > 0 | on | **force-exit(1)** (supervisor restarts process) |
| > `WATCHDOG_STALE_SECS` | 0 | on | log warn only (quiet period, pipeline idle) |
| any | any | off | no action |

Constants (in `tuning.rs`): `WATCHDOG_STALE_SECS = 60`, `WATCHDOG_INTERVAL_SECS = 15`.

The force-exit is deliberate: a stale-but-busy DbWriter means the tokio runtime is stuck (lock, panic, OOM). Logging inside a stalled runtime won't work. Hard exit is the only reliable recovery path.

## `on_trade_executed` — detailed flow

Called by `pipeline.rs` for every decoded trade event. Hot path; must not block.

1. **Decode** — trade fields already in `Trade` struct from `decode_protobuf`
2. **TokenCache update** — `cache.update_trade(&trade)`: update `last_price`, `reserves`, volume accumulators, `last_trade_at`; price-series rolling append (bounded `MAX_TRADES_RETAINED`)
3. **Metrics write** — `try_send(DbWriteOp::Metrics(...))` — drops on full
4. **AMM prewarm** — if `venue == Amm` and the pool is not yet in `ReserveCache`, enqueue a one-shot RPC fetch (spawned, off hot path)
5. **TradeSignals ping (mint lane)** — `signals.notify_mint(&trade.mint)` — wake any mint-scoped waiter (no allocation, just atomic notify)
6. **DB write (trade)** — `db_tx.send(DbWriteOp::Trade(...)).await` — blocking
7. **DB write (wallet)** — `db_tx.send(DbWriteOp::Wallet(wallet.clone())).await`
8. **Strategy fan-out** — `strategy_tx.try_send(StrategyEvent::Trade { mint })` — non-blocking
9. **SSE** — `sse_tx.send(SseEvent::Trade(...))` — broadcast, no wait
10. **Raw write** — `db_tx.try_send(DbWriteOp::Raw(...)).ok()` — best-effort

Steps 6–7 block; everything else is lock-free or channel-non-block.

## Decoder detail — `decoder/`

### Entry points

`decode_protobuf(tx: Arc<SubscribeUpdateTransaction>) -> Option<DecodedTx>` — used by both live ingest and `token_sync` replays.

`decode_relevant_pb(tx: ...) -> Option<(DecodedTx, TxRelevance)>` — adds relevance classification (saves CPU for irrelevant txs in live pipeline).

Both share one decode body; `LazyKeys` defers base58 encoding until the field is actually read.

### `TxRelevance`

```rust
pub enum TxRelevance {
    TokenCreate,
    Trade { venue: Venue },
    Migration,
    CreatorActivity,
    LiquidityEvent,
    Irrelevant,
}
```

`client.rs` pre-filters by account key set (no full decode for irrelevant txs). `pipeline.rs` matches on `TxRelevance` to route to the correct handler.

### Trade decode path

`grpc/mod.rs` → `grpc/trade.rs::decode_trade_from_pb(inner_ix, accounts, meta)`:

1. Match `InstructionKind` from `instructions.rs` (Buy/Sell/AmmBuy/AmmSell)
2. `build_amm_trade` or direct curve parsing: extract `sol_amount`, `token_amount`, `is_buy`, `virtual_sol_reserves`, `virtual_token_reserves`
3. `compute_sol_change(meta, accounts, wallet_pubkey)` — walks inner instructions to find the net SOL delta for the trading wallet (not the gross `sol_amount` field, which is the bonding curve's delta)
4. Assemble `Trade { mint, wallet, sol_amount: actual_change, token_amount, price, ... }`

### Codegen

Committed prost/tonic bindings live in `decoder/grpc/generated/`. `.proto` sources in `proto/`. These are **not** regenerated at build time. To regenerate: change `.proto` → run `build.rs` locally → commit the updated `generated/` files. This avoids making the build depend on `protoc`.

## DbWriter batching — `db_writer.rs`

Flush triggers: **256 ops accumulated** OR **25ms timer** (whichever comes first).

Inside `flush()`:

1. Drain `db_rx` into a local `Vec<DbWriteOp>` (no allocation per item — buffer is reused across flushes)
2. Group by variant: deduplicate `Wallet` inserts (keyed by address), deduplicate `Token` inserts (keyed by mint)
3. Bulk-insert each group in a single `INSERT ... ON CONFLICT DO NOTHING` / `ON CONFLICT DO UPDATE` call
4. For `Trade`: bulk insert `floor(65535 / binds_per_trade)` rows per batch (sqlx 0.6 bind-param ceiling)
5. For `Raw`: build blob off-thread via `adapter::build_raw_blob` (CPU-bound), then bulk-insert
6. Stamp `IngestHeartbeat.last_flush_at` and `queue_len` at end of flush
7. Notify `TradeSignals` per `(wallet, mint)` pair seen in this flush batch

Dedup rationale: the gRPC feed can send the same wallet address in multiple transactions in one flush window. Dedup prevents unnecessary ON CONFLICT churn.
