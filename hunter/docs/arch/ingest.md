# Ingest — the read-stack: engine, wires, venue

File-level map of `shared/ingest/` (four crates) and `live/src/ingest/` (host adapter).

**Four crates on three axes.** One folder per concern, so which part is which is
readable off the tree:

| Crate | Axis | Owns |
| --- | --- | --- |
| `ingest-core` | the **engine** | the `Feed` seam, the one reconnect/route supervisor, the `IngestVenue` seam, the session + decode lanes, `IngestEvent`, the single JSON→protobuf adapter, proto **messages** |
| `ingest-laserstream` | a **wire** | Yellowstone gRPC: connect, subscribe, resubscribe in place, replay from a slot |
| `ingest-nats` | a **wire** | the broadcast relay: NATS core client, shed-on-full reader, envelope unwrap |
| `ingest-pumpfun` | the **venue** | pump.fun classify + decode + pool PDAs, **and** `assembly.rs` — the only module that knows which wires exist |

`ingest-core` carries no `tonic` and no wire; each feed crate carries exactly one and
knows no venue; the venue crate knows every wire but no policy. **Adding a transport is
a fifth crate implementing `Feed` plus one arm in `assembly.rs`** — nothing in the
engine or the venue moves.

**Scope, not mode.** Each feed runs its own supervisor against its own `StreamScope`
(`program` = bonding curve, `pools` = tracked AMM PDAs). AMM pools always ride gRPC,
because you cannot subscribe to one pool PDA on a broadcast subject. The curve rides
whichever feed `ingest.curve_source` selects, switchable at runtime with no restart.
Both wires converge on `SubscribeUpdateTransaction` before anything decodes, so
everything below the feed is source-agnostic.

Logic explainers: `@plans/ingest/laserstream-workflow.md`, `@plans/ingest/nats-relay-transport.md`, `@plans/ingest/token_sync-workflow.md`, `@plans/ingest/backpressure-watchdog.md`, `@plans/ingest/reconnect-restart-flow.md`.

## Architecture

The read-stack has **no workspace deps** — it emits decoded `IngestEvent`s out a bounded
mpsc channel and the host (`live`) owns all sinks.

```text
ingest-pumpfun: Ingest::builder()...build()?.start(live)
  -> (Receiver<IngestEvent>, IngestHandle)

  assembly.rs                    picks the wires, owns watch<FeedKind>, hands each
                                 feed a watch<StreamScope>
        |                                   |
        v                                   v
  ingest-laserstream               ingest-nats
  GrpcFeed  (scope ALL | POOLS)    NatsFeed  (scope CURVE | NONE)
  replay - server filter -         no replay - no filter -
  in-place resubscribe             frame.rs -> ingest_core::convert -> protobuf
        |                                   |
        +-------------> FeedUpdate <--------+
                            |
  ingest-core: supervisor::run<V, F>  (ONE loop, per feed)
      reconnect ramp - ReplayAnchor - idle guard - dedupe - classify - lane pick
                            |
    --create lane-->  decode task (Create)
    --normal lane-->  decode task (Curve/Amm)
    --IngestEvent-->  host event channel (both lanes merge)

Host adapter (live/src/ingest/):
  consumer.rs  (translate IngestEvent -> trading_core types, fan-out)
    --> { db_tx, create_tx, strategy_tx, sse_tx, trader, trade_signals }
  db_writer.rs  (batch persistence) --> Postgres --> TradeSignals.notify
  watchdog.rs  (OS thread, DbHeartbeat)
```

**`FeedCaps` is what keeps the supervisor wire-blind.** Three flags — `replay`,
`server_filter`, `in_place_resubscribe` — and every decision that differs per wire reads
one of them instead of naming a transport:

| Decision | Reads |
| --- | --- |
| Send a `from_slot`, keep a `ReplayAnchor` | `replay` |
| Stalled pipeline: reconnect, or shed and stay up | `replay` (a reconnect only helps if it can re-request what the stall cost) |
| An empty account set means "idle", not "watch the chain" | `server_filter` |
| A pool-set change: resubscribe, or reconnect | `in_place_resubscribe` |
| What silence is judged by (`idle_basis`) | `server_filter` + the scope |

**Switching the curve is a hand-over, not a cut-over.** `assembly.rs` widens the feed
*gaining* the curve first, holds both for `HANDOVER` (5 s), then narrows the one losing
it. Cutting over on the spot cost 7-10 slots of trades in each direction, measured on
mainnet: a relay connect + subscribe runs ~2.2 s, and the old owner had already dropped
the program id. The window must stay inside `IngestConfig::dedupe_window` (30 s) — the
ring is what makes the overlap free — which a unit test pins.

Channels: create update cap `max(64, update_cap/8)` · normal update cap 4096 ·
`event_rx` 4096 · `db_tx` 16384 · `create_tx` 256 · `strategy_tx` 512 · `sse_tx` broadcast.

**Classify (`Decoder::classify_accounts`) reads the message, not the logs.** The
pre-filter runs on the single transport task that gates every create's arrival, so
it must be cheap: it does 32-byte program-key compares over
`account_keys ++ loaded_writable ++ loaded_readonly`, not a substring scan of every
log line for a 44-char base58 program id. The scan it replaced re-derived what the
subscription had already proven — `account_include` is the pump program + tracked
pool PDAs. Same pass decides `Create` (see `TxRelevance` below). Zero alloc:
`key_at` walks the three key slices by index instead of collecting like `LazyKeys`
(guarded equal by `key_at_matches_lazykeys_ordering`). Two properties: **Curve wins
over Amm** (a pool PDA can deliver an AMM tx, so the distinction is still made), and
it is **strictly more complete** than the log scan — dropped/truncated logs hide the
program id from `classify_logs` but not from the keys, so such a tx now reaches
decode (which returns `Ignored` if there's nothing in it). `classify_logs` survives
as the backfill classifier and as the parity reference in
`account_key_classify_agrees_with_the_log_scan`.

**`TxRelevance = Create | Curve | Amm`.** `Create` is `Curve` **plus a routing
hint** — both arms run `decode_curve_pb`, single-sourced through
`TxRelevance::is_curve()`, so the tag can never change what gets decoded and a
missed create costs only the hint. Detection is the `create`/`create_v2`
discriminator on a pump-program instruction, top-level **or** inner CPI (a bundled
launch invokes `create` as a CPI, and those are exactly the creates worth routing
first), via the same `is_create_disc` predicate the decode path uses. The tag drives
the **create fast lane**: `IngestVenue::is_create_lane` routes creates onto a
dedicated transport→decode channel + decode task (AMM/curve volume cannot delay a
create decode), and the host consumer sends `TokenCreated` onto a dedicated
`create_tx` drained by the decision loop *above* the general trade-ping arm.

**Backpressure:** money-critical writes (`Trade`/`Migration`/`Token`/`Wallet`) use
non-blocking `try_send` on the hot `db_tx` (cap 16384) so a PG stall never
awaits on the ingest consumer (create/trade pings stay unblocked). On Full the
op is deferred into a bounded retry buffer (cap 4096) drained by a background
task with `send().await`; only when that buffer is also full is the write shed
(counted). Recomputable writes (`Metrics`/`Raw`) use `try_send` (dropped on
Full). Own-wallet trades also call `TradeSignals::observe_own_leg` before the
durable enqueue so buy/sell confirm can resolve without waiting on DbWriter.
See `@plans/ingest/backpressure-watchdog.md`.

**Liveness watchdog:** `db_writer.rs` stamps `DbHeartbeat` at the end of a `flush()` **only when it persisted ≥1 row** (`any_ok`) — an all-failed flush leaves it stale. A dedicated OS thread (via `watchdog::spawn_watchdog`) force-exits (`exit(1)`) when live mode is on AND no successful write landed within `watchdog_stall_timeout_secs`. It does **not** gate on DB-queue depth: that proxy misses upstream stalls, because a dead transport drains the queue empty — the healthiest-looking reading there is. `live + stale` alone catches both a wedged downstream and a dead upstream. The heartbeat must stay a measure of *work completed*, never of the flush loop iterating.

**It only polices the steady state (`BootGate`).** The watchdog is armed by `boot_gate.mark_ready()`, latched by the strategy decision loop immediately before it starts consuming; while unset the heartbeat is stamped each check. Startup work competes with `DbWriter` on 2 vCPU, so a slow boot otherwise reads as a wedged pipeline — and killing a *booting* process turns a slow boot into an unbreakable crash loop rather than a recovery (see `@arch/strategies.md` "Boot recovery is bounded at both ends"). A boot that genuinely hangs surfaces as a stuck process — check for the absence of `strategy engine loop running`.

## `shared/ingest/*/src/` — crate modules

| File | Responsibility |
| --- | --- |
| **pumpfun** `lib.rs` | `Ingest` builder + `IngestHandle` façade; `start(live)` → `(Receiver<IngestEvent>, IngestHandle)` |
| **pumpfun** `assembly.rs` | `FeedKind`, `scope_for` (the whole selection rule), `spawn_feeds`, the widen-before-narrow hand-over. The ONE module that names a wire |
| **core** `feed.rs` | The seam: `Feed`, `FeedConn`, `FeedCaps`, `StreamScope`, `Subscription`, `FeedUpdate`, `FeedError` |
| **core** `supervisor.rs` | `run<V, F>` — the one reconnect/route loop: live gate, scope gate, `resolve_from_slot`, `idle_basis`, dedupe → classify → lane pick; `FeedPolicy` |
| **core** `session.rs` | `Ingest<V>`, `IngestHandle<V>`, `FeedLanes<V>`, the two decode lanes |
| **core** `push.rs` | `PushHooks` — block-meta + watched-account callbacks |
| **laserstream** `lib.rs` | `GrpcFeed`/`GrpcConn`, `GrpcConfig`, `Auth`, `connect`, `CAPS`. TLS auth; `ResourceExhausted` maps to `FeedError::Exhausted`, the one reason that forbids a replay |
| **laserstream** `subscribe.rs` | `Subscription` → `SubscribeRequest`. An empty account set omits the filter entirely — Yellowstone reads an empty `account_include` as *every* transaction on chain |
| **laserstream** `client.rs` | The generated tonic `geyser_client`, split from core's messages-only `geyser.rs` |
| **nats** `lib.rs` | `NatsFeed`/`RelayConn`, `NatsConfig`, `CAPS`, the shed-on-full reader, wire stats |
| **nats** `frame.rs` | One relay frame → one `FeedUpdate`: envelope unwrap, failure screen, delegate to core's converter |
| **nats** `client.rs` | Hand-rolled NATS core client (connect / SUB / MSG / PING) on tokio TCP. No crate dependency: every NATS crate hard-depends on `nkeys`, whose dalek 4.x `zeroize` bound conflicts with the curve25519-dalek 3.2.1 solana 1.17.27 pins |
| `event.rs` | Crate-owned output types: `IngestEvent`, `Trade`, `TokenCreated`, `TokenMigrated`, `LiquidityEvent`, `CreatorActivityEvent`, `BuyInstructionArgs`, `Side`, `Venue`, `Reserves`; `RawTx` under `raw-json` feature. Also `fee_lamports_opt` — the ONE reader of the `meta.fee` sentinel (see below) |

**`Trade.fee_lamports`** — the transaction's on-chain network fee (base signature fee
+ priority fee) from `TransactionStatusMeta.fee`, which the decoder already holds in
scope; capturing it costs one field read and **zero** RPC/Helius credits. Stamped by all
three trade paths (`decode_curve_pb`, `decode_amm_live_pb`, the balance-delta fallback)
and by the RPC backfill (`backfill::meta_from_json` — it must set the field, because
leaving the protobuf at its `0` default reads as "free", not "unknown").

Two invariants a consumer must not break: it is charged **once per transaction**, so
every leg decoded out of one tx repeats the same value (collapse by `signature` before
summing); and `0` is impossible on a landed tx, so `fee_lamports_opt` folds the
protobuf's ambiguous zero to `None` at the source rather than letting each decode site
invent its own rule. It excludes the Jito tip (a transfer instruction, not a fee) and
the venue's protocol/LP fee (already inside `sol`).

**`TokenCreated.uri`** — the token's off-chain metadata pointer, read from whichever of
the `create` instruction args or the `CreateEvent` log is present (log first, matching
`name`/`symbol`). Like `fee_lamports` it comes from bytes the decoder already holds, so
it costs zero RPC/Helius credits. An empty uri maps to `None`, because absence is a fact
a token filter reads rather than a default to substitute. The create transaction is the
only place it appears on the wire and `raw_txs` drops after 3 days, so a uri not captured
live is gone; the host persists it into `tokens.meta` — see
[@plans/database/token-storage.md](../plans/database/token-storage.md).
| `protocol.rs` | `Protocol` descriptor: pre-decoded program IDs (`ProgramId` w/ bytes + base58) + discriminators decoded once at build time |
| `config.rs` (core) | `IngestConfig` — **wire-neutral tunables only**, no env reads; `Commitment`. A dial timeout, an HTTP/2 keepalive or a subject name belongs to its own feed crate's config, so a new transport cannot grow this struct |
| `error.rs` (core) | `IngestError`, `Result<T>` alias. Carries no provider error type — a wire reports `FeedError` instead |
| `pool.rs` | PDA derivation (`derive_pool`), `register_pool`, `pool_for_mint`; `PoolIndex = Arc<DashMap<String, String>>` |
| `venue.rs` | `PumpFunVenue`: `subscription_accounts(StreamScope)`, classify, decode, `derive_pool` |
| `convert.rs` (ingest-core) | `json_tx_to_protobuf` — the ONE JSON→protobuf adapter, shared by the RPC backfill and every JSON live feed. Auto-detects `base64` vs `jsonParsed`; splits jsonParsed's inlined ALT keys back into `account_keys` + `loaded_*` so both sources produce identically-shaped updates — **`json-tx` feature** |
| `dedupe.rs` (ingest-core) | `SignatureDedupe` — lock-free fixed ring over signature prefixes. Absorbs the hand-over overlap and the migration tx that matches both feeds. Armed only when more than one feed runs (`Ingest::start(live, feeds)`) |
| `decode/mod.rs` | `Decoder` (+ `HeliusDecoder` back-compat alias), `TxRelevance`, `DecodeOutput` |
| `decode/protobuf.rs` | `decode_protobuf` (self-classify), `decode_relevant_pb` (hot path), `decode_amm_protobuf` (backfill); `LazyKeys`. **Curve TradeEvents: read "Program data:" logs first, but the validator truncates logs past a byte limit, so a multi-buy bundle can lose trailing legs — when logs are empty OR carry "Log truncated", re-decode from the complete inner-instruction self-CPI events and take the larger set. AMM path is still log-only (latent same risk).** |
| `decode/trade.rs` | Borsh `RawTradeEvent`, trade helpers |
| `decode/instructions.rs` | `InstructionKind`, `label_instruction` (per-ix human label) |
| `decode/program_registry.rs` | program names, `ANCHOR_IX`/`EXPLICIT_IX`, `IxKey` widths |
| `decode/program_registry.rs` | `program_friendly_name` — program-id → name table (SSOT for naming), backed by a `OnceLock<HashMap>`. Grow via the `unknown-programs` harvest |
| `decode/create.rs` | `decode_create_events_from_logs` |
| `raw_tx.rs` (ingest-core) | `encode_payload` (protobuf wire bytes), `build_raw_tx_event` — **`raw-tx` feature** |
| `backfill.rs` (ingest-core) | `rpc_to_protobuf` (RPC result → protobuf) + the JSON-RPC pager — **`rpc-backfill` feature** |

### Feature gates

| Feature | Unlocks |
| --- | --- |
| `raw-tx` | `IngestEvent::RawTx` (carries protobuf `payload` bytes), `raw_tx::encode_payload` |
| `json-tx` | `serde_json` dep, `convert::json_tx_to_protobuf` — implied by both feature gates below |
| `rpc-backfill` | `backfill::rpc_to_protobuf` (a thin wrapper over `convert`) + the JSON-RPC pager |
| `nats` | links the `ingest-nats` crate (`NatsFeed`, `NatsConfig`, its client) |

`hunter-live` enables all four; `forge-live` takes `rpc-backfill` only and **never links
the relay crate at all** — the feature now gates a dependency, not a module. `IngestHandle`
exposes `set_live`, `is_live`, `set_curve_feed`, `curve_feed`, `set_gap_replay`,
`track_pools`, `untrack_pools`, `pool_index`, `pools_changed`. (Liveness is tracked
host-side by `live`'s own `DbHeartbeat`, not an ingest health channel.)

**Push hooks (`ingest_core::PushHooks`, optional):** the same subscription can carry a `blocks_meta` filter and an `accounts` filter for host-chosen pubkeys; the two callbacks run on the supervisor task (cheap parse + store only). Hunter's `main.rs` bridges block metas → `Engine::set_cached_blockhash` (blockhash cache, 0 steady-state `getLatestBlockhash`) + `ingest::feed_lag::FeedLagGauge` (below), and nonce-account updates → `Engine::on_nonce_account_update` (durable-nonce push re-arm). Hosts that don't opt in (forge) get a byte-identical subscription. Push updates do not feed the idle watchdog on a scope that carries the program — there it guards the *transaction* stream, which a firehose never leaves quiet. On a pools-only scope they are the only liveness signal there is, so `idle_basis` judges by any frame instead; see `@plans/ingest/reconnect-restart-flow.md`.

**Chain time lives on block metas only.** A transaction frame carries an exact slot but **no block time**, so `decode_relevant_pb` stamps `block_time: received_at` — the `trades.block_time` column is this bot's receive clock, and `received_at - block_time` is identically zero everywhere downstream. `on_block_meta`'s third argument (`block_time_unix_secs`) is therefore the ONE chain-clock reference on the stream, and `FeedLagGauge` is the one consumer of it: it accumulates `now - block_time` and logs a windowed `feed_lag` line (mean / max / stale-slot count) every ~150 slots, at WARN once any slot in the window is ≥2 s behind. Resolution is whole seconds, so a sample bounds lag rather than timing it — it separates a healthy feed from a backlogged or replaying one, which no other counter shows. Sub-second stage timing belongs to the `snipe_latency` / `exit_latency` lines instead ([trade-execution.md](trade-execution.md)).

## `live/src/ingest/` — host adapter

| File | Responsibility |
| --- | --- |
| `mod.rs` | `spawn_ingest(...)` — builds `Ingest`, starts it, spawns consumer + db_writer tasks, starts watchdog thread; returns `IngestSpawnResult` |
| `held_pools.rs` | `HeldPoolGate` — keeps PumpSwap pools subscribed for unsettled **real** positions even when `track_post_migration` is off (feed harvest + sell-confirm) |
| `consumer.rs` | `IngestConsumer` — translates `IngestEvent` → `trading_core` types; fans out to token_cache, DB, strategy, SSE, trader; handles `track_mayhem` / `track_post_migration` policy transitions |
| `db_writer.rs` | `DbWriter` — batches (1000 ops / 150ms), dedups, persists; stamps `DbHeartbeat` after a flush **only if ≥1 row persisted** (`any_ok`); signals `TradeSignals` per `(wallet,mint)`; `DbWriteOp` variants: `Raw(RawBlobJob)` · `Token` · `Wallet` · `Trade` · `Metrics` · `Migration` |
| `watchdog.rs` | `DbHeartbeat` (atomic ms stamp), `BootGate` (latched by the engine loop; disarms the watchdog during startup), `spawn_watchdog` (OS thread); force-exits when live, booted, and no successful write within `watchdog_stall_timeout_secs` (no queue-depth gate — catches upstream stalls too) |

### Consumer event handlers

`on_token_created` (Token+Wallet+Metrics+cache+ping+SSE) · `on_trade` (Trade+Wallet+Metrics+reserves+inline AMM account-list harvest+ping+SSE) · `on_token_migrated` (pool gate+Migration+ping) · `on_creator_activity` (ping) · `on_liquidity` (SSE only)

AMM `Trade` events may carry `amm_swap_accounts` (the top-level PumpSwap swap's resolved account list, harvested by `decode_amm_live_pb`, one per pool per tx). `on_trade` feeds it to `TraderHook::observe_amm_swap_accounts` inline (pure CPU — replaces the old spawned RPC `prewarm_amm_pool`); `amm_pool_prewarmed` still means "trader cache warm for this mint", and a rejected parse just retries on the next swap.

**Held-position pool retention:** `track_post_migration` only gates *all* AMM history recording. Unsettled real positions always keep their pool on the gRPC filter (`HeldPoolGate`, noted by the engine sink + boot seed) so harvest + feed-confirm stay warm. `on_token_migrated` / `clear_pools` must not untrack a held mint — that path was reintroducing the `getSignaturesForAddress` + `getTransaction` cold burst on every AMM exit.

## Decoder — `decode/`

Protobuf-native only (no Value/JSON path in decode). Both live ingest and `token_sync.rs` feed `decode_protobuf`.

Pool→mint index (`PoolIndex`) is shared: the decode task auto-registers pools on `TokenMigrated`; the consumer un-registers when `track_post_migration` is off.

Codegen: committed prost/tonic bindings in `generated/`; `.proto` sources in `proto/`. Regen only when `.proto` changes.

**Instruction labeling (`ix_labels`, one string per top-level ix).** Every label is
`"<program>: <instruction>"`, and each half is resolved independently — knowing a
program did `SellBondingCurvePercentage` does not require knowing who owns it.
The program half comes from `program_registry::REGISTRY`, falling back to
`"Unknown (<full program id>)"`. The instruction half comes from
`program_instruction_label`, which returns a **name** when the registry can prove
one, otherwise the instruction's **stable key** (`ix#af051981a0d8389d`,
`ix#01`), and only says `Unknown` when the feed delivered no instruction data at
all. The unknown-program fallback carries the **full** program id (not a
truncated suffix) so unknowns are self-identifying in the persisted
`trades.ix_labels` — the durable label record, since **this deployment does not
persist `raw_txs`** (migration 0002 promoted `ix_labels` onto `trades` for
exactly this reason). Only *top-level* instructions are labeled (inner CPIs are
used for trade recovery, not labels).

A key is not a name and does not pretend to be one; it is an identity. Before it
existed, every Axiom instruction collapsed to one string and a router's buy was
indistinguishable from its sell. The key width per program is a **cardinality**
decision, held in `IxKey`: eight bytes for a dispatch value that carries no
arguments, one byte for a tag followed by a `u64` amount. Reading eight bytes off
a tag program would fork one instruction into thousands of labels, which would
make `ix_hash` unique per trade and dissolve every fingerprint grouping built on
it.

Memo is the one program whose data is deliberately *not* read into the label:
a memo's bytes are its text, and that text is per-transaction unique often enough
to do exactly that damage. Memos label as `"Memo Program: Memo"`; the payloads
are reported in aggregate by `decode-harvest` instead.

*Where names come from* is documented in
[plans/ingest/instruction-decoding.md](../plans/ingest/instruction-decoding.md).
Two tables in `program_registry.rs`: `ANCHOR_IX` stores the instruction NAME and
**computes** the discriminator (`sha256("global:<snake_name>")[..8]` via
`solana_sdk::hash`), so a wrong name is fail-safe — it matches nothing and the
label degrades to a key, never to a wrong name. `EXPLICIT_IX` holds the few
programs that log a name but do not hash it that way; there the key bytes are
transcribed, so that table stays short and reviewed.

Two commands close the loop, neither on the trading path:

```powershell
cargo run -p hunter-live -- unknown-programs [--days N] [--top N]
cargo run -p hunter-live -- decode-harvest   [--days N] [--top N] [--txs N] [--program ID]
```

`unknown-programs` aggregates `trades.ix_labels` and ranks the programs the
labeler cannot name (DB-only, no keys/Helius). `decode-harvest` takes those
programs back to the chain: it pairs each `Program log: Instruction: <Name>` line
with the discriminator of the instruction that produced it and **verifies** the
pair by recomputing the hash, then prints paste-ready rows. One
`getTransactionsForAddress` per program, not one per transaction. A pair that
does not verify is reported, never emitted — prefer *no* entry over a guessed
one, because a wrong label is worse than a key.

## Key rules

- `trades` table = this feed. TPSL exit loop confirms fills from this feed (not a separate RPC).
- No blocking I/O / `.await`-on-lock / unbounded alloc per event on the ingest hot path; DB+SSE go through channels.
- The read-stack has **zero workspace deps** — standalone drop-in crates.
- **A new wire is a fifth crate, not a branch.** Implement `Feed` + `FeedConn`, declare
  its `CAPS`, add one arm to `assembly.rs`. If it needs an arm in `supervisor.rs` or
  `venue.rs`, the capability it needs is missing from `FeedCaps` — add it there rather
  than naming the wire downstream.
- **Every feed crate must be in the log filter.** `ingest_core`, `ingest_pumpfun`,
  `ingest_laserstream` and `ingest_nats` are named explicitly in `live/main.rs`'s default
  `EnvFilter`, and the box sets no `RUST_LOG` — without them, every connect / stream
  error / idle reconnect / backpressure line is dropped and that wire runs invisibly in
  production. `live::ingest` (host adapter + watchdog) is covered by `live=info`, but the
  crates *underneath* it are the layer that knows why a feed died. Every one of those
  lines is per-connection, never per-message, so there is no hot-path cost.

**A watchdog kill means a real feed outage** — verified against `trades` row counts, not
assumed. Two consequences are still open (a kill loses 1–2 min of feed permanently, and
something defeats the ~12 s self-heal): [`@roadmap/ingest-watchdog-kill-recovery.md`](../roadmap/ingest-watchdog-kill-recovery.md).
