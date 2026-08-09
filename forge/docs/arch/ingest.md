# Ingest — LaserStream (the sole live transport)

Forge's live read-side. Helius LaserStream (Yellowstone gRPC) is the only live
transport; it feeds pump.fun trades / token-creates / raw-txs into Postgres
(`raw_txs` / `trades` / `tokens`).

**Not a crate.** Ingest is the `forge/live/src/ingest/` **module** inside the
`forge-live` bin (folded in from the former `ingest-host` crate — the CLAUDE.md /
README crate table is stale on this point). It is a **host adapter** that bridges
the two standalone read-stack crates onto forge's schema:

- `shared/ingest/core` (`ingest-core`) — venue-agnostic engine: gRPC transport +
  reconnect, the `IngestVenue` seam, the generic `Ingest<V>` / `IngestHandle<V>`
  session, the neutral `IngestEvent` contract, `IngestConfig`, slot-anchor,
  raw-tx / rpc-backfill adapters. **No pump.fun coupling.**
- `shared/ingest/pumpfun` (`ingest-pumpfun`, dep-aliased `ingest_laserstream`) —
  the pump.fun `PumpFunVenue` (classify + decode + pool derivation) plus a
  back-compat façade (`Ingest::builder()…`) so callers compile unchanged.

Ingest ships to the LIVE box only; it must never appear in `forge-lab`'s dep graph.

## Architecture

```
Helius LaserStream (Yellowstone gRPC)
        │  SubscribeUpdateTransaction (protobuf)
        ▼
┌──────────────────────────────  shared/ingest/core + /pumpfun  ──────────────────────────────┐
│  transport::run<V>            (core/transport/mod.rs — one OS-independent tokio task)          │
│    • connect + Subscribe (x-token auth), exponential-backoff reconnect, idle-reconnect guard  │
│    • V::classify() pre-filter (log scan): Curve | Amm | ignore                                 │
│    • forward (Arc<update>, relevance, received_at)                                             │
│         └─ send_timeout(pipeline_send_timeout) ── Timeout ⇒ drop conn, reconnect (shed)        │
│        ▼  update_channel_cap = 4096                                                            │
│  decode task                  (core/session.rs)                                               │
│    • V::decode() = PumpFunVenue → Decoder.decode_relevant_pb → Vec<IngestEvent>               │
│    • + build_raw_tx_event (raw-tx feature) appended after the semantic events                 │
│    • try_send → on Full, send_timeout(100ms) retry → else DROPPED_EVENTS counter + warn       │
│        ▼  event_channel_cap = 4096   (mpsc::Receiver<IngestEvent>)                             │
└───────────────────────────────────────────────────────────────────────────────────────────────┘
        │  IngestEvent { TokenCreated | Trade | RawTx | (TokenMigrated|Liquidity|CreatorActivity) }
        ▼
┌──────────────────────────  forge/live/src/ingest/  (host adapter, this doc)  ──────────────────┐
│  run_consumer               (consumer.rs — hot recv loop, NO DB I/O)                            │
│    • tx-tracking state machine: only forward RawTx if its tx already produced a semantic event  │
│    • map IngestEvent → DbWriteOp, then  tx.send(op).await   (durable backpressure)              │
│        ▼  DbWriteOp channel, CHANNEL_CAPACITY = 16_384                                          │
│  DbWriter task              (db_writer.rs — ALL DB I/O, interning, mapping)                     │
│    • batch by FLUSH_EVERY=256 or FLUSH_INTERVAL=500ms                                           │
│    • map::* → NewToken / NewMarket / NewTrade / RawTx ; wallet_cache interns wallet_dict        │
│    • TokenRepo / MarketRepo / TradeRepo.insert_batch / RawTxRepo (UNNEST; bulk→per-row retry)   │
│    • heartbeat.stamp() + events.fetch_add(n)  each flush                                        │
│    • trades_notify.notify_one()  +  sse.trade_executed / token_created                          │
│        │                    │                          │                                       │
│        ▼ raw_txs/trades/    ▼ Arc<Notify>              ▼ SseHub → /api/stream                   │
│          tokens (PG)          (bundle-confirm watcher)   (browser push)                          │
│  watchdog (OS thread)       (watchdog.rs)                                                        │
│    • is_stalled(live && work_pending && idle≥120s) ⇒ std::process::exit(1)                       │
└──────────────────────────────────────────────────────────────────────────────────────────────┘

main.rs (always-on, feed-based — NO RPC poll):
  spawn_bundle_confirm_watcher  ── woken by trades_notify (fast) or 10s fallback tick ──▶
     TradeRepo::find_signatures_present(mint, leg_sigs) vs ingested `trades`
     → landed / dropped(→rebid) / partial
```

## Host adapter — `forge/live/src/ingest/`

| File | Responsibility |
| --- | --- |
| `mod.rs` | Module root + re-exports (`spawn_ingest`, `IngestHandle` from `ingest_laserstream`, `IngestMetrics`). Documents the pumpfun/map/consumer/db_writer/watchdog split. |
| `consumer.rs` | `spawn_ingest` (builds the borrowed transport paused, wires consumer→writer channel, spawns DbWriter + watchdog) and `run_consumer` — the hot recv loop. **No DB I/O.** Owns only the per-tx "did this tx produce a semantic event?" flag that gates `RawTx` persistence. Forwards durable work with `tx.send(op).await` (blocking backpressure). |
| `db_writer.rs` | `DbWriter` task: the sole DB-I/O owner. Drains the `DbWriteOp` channel (`CHANNEL_CAPACITY=16_384`), batches (`FLUSH_EVERY=256` / `FLUSH_INTERVAL=500ms`), maps via `map::*`, interns wallets (memoized `wallet_cache`), and bulk-inserts via `TokenRepo`/`MarketRepo`/`TradeRepo`/`RawTxRepo`. Stamps the heartbeat, bumps the events counter, fires `trades_notify` + SSE per flush. Bulk-insert failure falls back to per-row. |
| `map.rs` | Pure `IngestEvent` → platform-core row mappers (no DB/network, unit-testable). `trade_to_row` / `token_created_to_row` / `raw_tx_to_row`. Takes amounts from EXACT raw-`u64` lamport fields (`sol_lamports`, `virtual_sol_lamports`) — no f64 round-trip; decodes base58 sig → BYTEA. `PUMP_TOKEN_DECIMALS = 6`. |
| `pumpfun.rs` | `PumpFunAdapter` — the `LaunchpadAdapter` (venue) impl. Resolves interned `launchpads.key='pump_fun'` + `quote_assets.symbol='SOL'` ids from the DB at boot (never hardcoded); classifies curve (`CURVE_PROGRAM_ID`) vs PumpSwap AMM (`AMM_PROGRAM_ID`) → `MarketKind`. Everything pump.fun is SOL-quoted. |
| `watchdog.rs` | OS-thread process watchdog on the `DbHeartbeat`. `is_stalled(live && work_pending && idle ≥ STALL_TIMEOUT=120s)` ⇒ `std::process::exit(1)` so the supervisor restarts + gap-replay refills. Checks every 30s; a paused stream or a drained queue never trips it. Pure `is_stalled` predicate is unit-tested. |
| `metrics.rs` | `IngestMetrics` — lock-free health surface for `GET /api/ingest`: commit-idle millis (`DbHeartbeat`), committed-events total (`AtomicU64` the writer bumps), and channel `buffer_depth`/`buffer_capacity` via a `WeakSender` peek (weak so metrics never keep the channel alive). |
| `roundtrip_test.rs` | Test module (`#[cfg(test)]`). |

## Borrowed engine — `shared/ingest/core` + `shared/ingest/pumpfun` (the seam)

| File | Responsibility |
| --- | --- |
| `pumpfun/src/lib.rs` | Back-compat façade. `Ingest`/`IngestBuilder` are thin newtypes over `ingest_core::Ingest<PumpFunVenue>` so `Ingest::builder().endpoint().api_key().protocol().config().build().start(live)` resolves unchanged; `.protocol(Protocol::pump_fun())` selects the venue, `.api_key()` becomes `Auth::XToken`. Re-exports `config/event/proto/…` at their original paths. |
| `pumpfun/src/venue.rs` | `PumpFunVenue` — the `IngestVenue` impl. Owns one shared `PoolIndex` + `pools_changed` `Notify` that its `Decoder` and the transport both hold (auto-discovered pool ⇒ subscription account with no cross-task hand-off). `classify` = log scan for pump_fun vs pump_swap program id → `TxRelevance::{Curve,Amm}`; `decode` → `Decoder::decode_relevant_pb`; `derive_pool` = pool PDA; `track_amm` gates AMM attribution. |
| `pumpfun/src/decode/*`, `protocol.rs`, `pool.rs`, `transport/mod.rs` | pump.fun protobuf decoder (`grpc.rs`, `trade.rs`, `create.rs`, `instructions.rs`), program-id protocol constants, pool PDA derivation. `transport/mod.rs` here is a re-export shim; the real transport lives in core. |
| `core/src/venue.rs` | The `IngestVenue` trait (the seam) + `DecodeOutput` + `PoolIndex` type. Static dispatch (`Ingest<V>`, `transport::run<V>`) — no `Box<dyn>` on the hot path. Venue supplies: `filter_key`, `subscription_accounts`, `classify`, `decode`, `derive_pool`, `pool_index`, `pools_changed`. |
| `core/src/session.rs` | Generic `Ingest<V>` / `IngestHandle<V>`. `start(live)` spawns the transport task + the decode task and returns `(mpsc::Receiver<IngestEvent>, IngestHandle<V>)`. Owns the two internal channels (update + event), the live-mode `watch`, gap-replay `watch`, and the decode-side `try_send`→retry→`DROPPED_EVENTS` backpressure. Handle: `set_live`, `is_live`, `track_pools`/`untrack_pools`, `set_gap_replay`. |
| `core/src/transport/mod.rs` | `transport::run<V>` — the gRPC task. Connect (`x-token` interceptor, TLS), `Subscribe`, exponential-backoff reconnect w/ jitter, idle-reconnect guard, debounced resubscribe on `pools_changed`, optional gap-replay `from_slot`. Per update: `venue.classify()` then `send_timeout(pipeline_send_timeout)` to the decode task — a timeout forces a reconnect (sheds backpressure, avoids self-reinforcing billing storms on credit exhaustion). |
| `core/src/{config,event,error,slot_anchor,raw_tx,backfill}.rs`, `generated/` | `IngestConfig` (all tunables, no env reads) + `Auth`/`Commitment`; the neutral `IngestEvent` enum; error types; slot→time estimation; `raw-tx` passthrough builder (feature-gated) + `rpc-backfill` (feature-gated); generated Yellowstone/geyser protobuf. |

**`IngestEvent` variants** (core/event.rs): `TokenCreated`, `Trade`, `TokenMigrated`,
`Liquidity`, `CreatorActivity`, `RawTx`. Forge's consumer projects only
`TokenCreated`, `Trade`, and (conditionally) `RawTx`; the other three are decoded
but dropped in `run_consumer` (`_ => continue`, "not projected yet").

## Bundle-landing confirmation (feed-based, in `main.rs`)

Always-on, spawned unconditionally by `main.rs` via
`launcher::spawn_bundle_confirm_watcher` (`forge/launcher/src/confirm.rs`). It is
the reader half of forge's "sell/land-confirm stays feed-based, no RPC poll" rule:

- Woken by the same `Arc<Notify>` (`trades_notify`) the DbWriter fires after every
  trade commit (notify over poll); a `FALLBACK_INTERVAL = 10s` tick is only the
  backstop that still advances the time-driven `dropped`/re-bid path when no trades
  flow.
- For each `status='submitted'` bundle it checks whether all co-buy leg signatures
  are present in the ingested `trades` (`TradeRepo::find_signatures_present`). All
  present ⇒ `landed`; none after `CONFIRM_TIMEOUT=90s` ⇒ `dropped` (auto re-bid at
  an escalating Jito tip up to `bundle_max_retries`, else conceded); partial ⇒
  flagged anomaly (create is tx0 so the launch still exists).
- It never reads the chain over RPC — it reads only `bundles` + `trades` from the
  pool ingest already writes.

## Keystore restore (recovery backfill, `forge/live/src/restore/`)

The recovery path for a redeploy that came up with an **empty `forge_bot`** while
the operator copied only the keystore folder (`.enc` blobs). It rebuilds the DB
from the keystore + on-chain history by driving the **same decode+map path** as
live ingest from the RPC backfill pager instead of the gRPC feed — the decoder
can't tell the source apart, so the rows are identical.

```
POST /api/wallet_pool/restore ─▶ tokio::spawn run_restore (202 immediately)
  1 wallets.rs   read_dir keystore → decrypt each {role}-{uuid}.enc → address
                 → ManagedWalletRepo::insert_if_absent  (fresh ids, join by address)
  2 backfill.rs  getSignaturesForAddress (every wallet, union+dedup)
                 → getTransaction(base64) → rpc_to_protobuf → Decoder::decode_protobuf
                 → map::{trade_to_row,token_created_to_row} → TradeRepo::insert_batch /
                   TokenRepo::insert; creator ∈ dev wallets ⇒ mark_own_launch +
                   LaunchRepo::insert (gated on find_by_mint) + set_create_signature
  3 positions.rs launcher::reconcile_positions(mint) for every observed mint
```

Progress streams over `GET /api/stream` (`restore_progress` / `restore_complete`
SSE frames); the Wallet Pool page's "Restore from keystore" button renders it.

- **Real historical `block_time`** comes from `getTransaction.blockTime` and is fed
  in as the decoder's `received_at` — it's part of the `trades` dedup PK, so it must
  be exact, never "now".
- **Idempotent throughout** (managed_wallets by address, trades by
  `(block_time,tx_signature,leg_index)`, tokens by mint, launches gated on
  `find_by_mint`, positions upsert on `(mint,wallet)`) — safe to re-run.
- **RPC pager** lives in `shared/ingest/core` `backfill::` (`rpc-backfill` feature):
  `get_signatures_for_address` + batched `get_transactions_batch` + the existing
  `rpc_to_protobuf`. Additive-only for the hunter consumer.
- **Unrecoverable from keystore alone:** original wallet ids / labels / status /
  funding_source (fresh ids assigned) and `manage_actions` (no on-chain source).
- **Deploy caveat:** the server `LAUNCHER_KEK_PASSPHRASE` must match the one that
  encrypted the blobs, and the keystore dir (`WALLET_KEYSTORE`) must be bind-mounted
  into the `live-api` container — otherwise the `.enc` files are invisible.

## Key rules

- **Ingest is a module, not a crate.** `forge/live/src/ingest/`, inside `forge-live`.
  The `ingest-host` crate is gone (docs table stale). LIVE-only — never in `forge-lab`.
- **Spawns paused.** `spawn_ingest` calls `.start(false)`; no gRPC subscription opens
  until an operator flips `PUT /api/ingest {live:true}` → `handle.set_live(true)`.
  Ingest is optional: absent `HELIUS_LASERSTREAM_URL`/`HELIUS_API_KEY`, the box still
  boots + serves HTTP with ingest disabled.
- **Hot recv loop does NO DB I/O.** `run_consumer` only maps events to `DbWriteOp`
  and tracks the per-tx semantic-event flag; all mapping, wallet-interning, and
  inserts happen off the recv loop in the `DbWriter` task.
- **`raw_txs` is source-of-truth for what `trades` projects.** A `RawTx` event is
  persisted only for txs that already produced a semantic (`Trade`/`TokenCreated`)
  event — enforced by `tracked_in_tx` in the consumer, pure stream ordering (no DB).
- **Backpressure is layered and durable.** consumer→writer uses `send().await`
  (blocking, `CHANNEL_CAPACITY=16_384`): a wedged DB backs up this channel and, once
  full, backpressures the transport rather than dropping durable rows. The
  transport→decode and decode→host channels (`4096` each) use `send_timeout`/
  `try_send`+retry and may shed (reconnect / `DROPPED_EVENTS`) — only the
  pre-durable feed, never committed rows. There is **no** metric-shedding tier as in
  hunter (all forge ops are durable).
- **Watchdog force-exits, never stalls silently.** OS thread (so a starved runtime
  can't freeze it): queue backed up + no commit for ≥120s while live ⇒
  `std::process::exit(1)`; supervisor restart + gap-replay recover the missed slots.
- **Interned dimensions resolved at boot, never hardcoded.** `PumpFunAdapter::resolve`
  reads `launchpads`/`quote_assets` ids from the DB (fails loudly if the seed is
  missing); pump.fun is SOL-quoted throughout (curve + AMM).
- **Exact integers, no f64 round-trip.** `map.rs` takes amounts from raw-`u64`
  lamport fields; signatures decode base58 → BYTEA to match `raw_txs`.
- **The venue seam is static dispatch.** `Ingest<V>` / `transport::run<V>` — the core
  knows nothing about pump.fun; a new venue (or provider: Helius→Triton→self-hosted
  is just `endpoint` + `Auth`) is a config/impl swap, no transport change.
