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
    --create lane-->  decode task (Create)
    --normal lane-->  decode task (Curve/Amm)
    --IngestEvent-->  host event channel (both lanes merge)

Host adapter (live/src/ingest/):
  consumer.rs  (translate IngestEvent -> trading_core types, fan-out)
    --> { db_tx, create_tx, strategy_tx, sse_tx, trader, trade_signals }
  db_writer.rs  (batch persistence) --> Postgres --> TradeSignals.notify
  watchdog.rs  (OS thread, DbHeartbeat)
```

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

**Liveness watchdog:** `db_writer.rs` stamps `DbHeartbeat` at the end of a `flush()` **only when it persisted ≥1 row** (`any_ok`) — an all-failed flush leaves it stale. A dedicated OS thread (via `watchdog::spawn_watchdog`) force-exits (`exit(1)`) when live mode is on AND no successful write landed within `watchdog_stall_timeout_secs`. It no longer gates on DB-queue depth: that proxy missed upstream stalls (a dead transport drains the queue empty), so `live + stale` alone now catches both a wedged downstream and a dead upstream. (2026-07-22: stamping unconditionally kept the heartbeat fresh through a pool-exhaustion wedge, so the watchdog never fired for 7h.)

**It only polices the steady state (`BootGate`).** The watchdog is armed by `boot_gate.mark_ready()`, latched by the strategy decision loop immediately before it starts consuming; while unset the heartbeat is stamped each check. Startup work competes with `DbWriter` on 2 vCPU, so a slow boot otherwise reads as a wedged pipeline — and killing a *booting* process turns a slow boot into an unbreakable crash loop rather than a recovery. (2026-07-30: force-exited at ~90 s during event-log recovery **70 times in a row**; the engine loop was never reached once, so every strategy ping was shed and no rule ran for 14 h. See `@arch/strategies.md` "Boot recovery is bounded at both ends".) A boot that genuinely hangs now surfaces as a stuck process — check for the absence of `strategy engine loop running`.

## `ingest-laserstream/src/` — crate modules

| File | Responsibility |
| --- | --- |
| `lib.rs` | `Ingest` builder + `IngestHandle`; spawns transport + decode tasks; `start(live)` → `(Receiver<IngestEvent>, IngestHandle)` |
| `event.rs` | Crate-owned output types: `IngestEvent`, `Trade`, `TokenCreated`, `TokenMigrated`, `LiquidityEvent`, `CreatorActivityEvent`, `BuyInstructionArgs`, `Side`, `Venue`, `Reserves`; `RawTx` under `raw-json` feature |
| `protocol.rs` | `Protocol` descriptor: pre-decoded program IDs (`ProgramId` w/ bytes + base58) + discriminators decoded once at build time |
| `config.rs` | `IngestConfig` (all tunables, no env reads), `Commitment` enum |
| `error.rs` | `IngestError`, `Result<T>` alias |
| `pool.rs` | PDA derivation (`derive_pool`), `register_pool`, `pool_for_mint`; `PoolIndex = Arc<DashMap<String, String>>` |
| `transport/mod.rs` | gRPC producer: TLS auth, reconnect w/ backoff, gap replay from a retained `ReplayAnchor` (`resolve_from_slot` is the ONE decider — the anchor OUTLIVES a no-progress attempt, bounded by `MAX_REPLAY_ATTEMPTS`; resume is `slot + 1` because nothing dedups by signature before the strategy fold), idle-reconnect timer, backpressure guard; `connect`, `build_subscribe_request`, `TransportConfig`. Rules + the 2026-07-27 blackout RCA: `@plans/ingest/reconnect-restart-flow.md` |
| `decode/mod.rs` | `Decoder` (+ `HeliusDecoder` back-compat alias), `TxRelevance`, `DecodeOutput` |
| `decode/grpc.rs` | `decode_protobuf` (self-classify), `decode_relevant_pb` (hot path), `decode_amm_protobuf` (backfill); `LazyKeys`. **Curve TradeEvents: read "Program data:" logs first, but the validator truncates logs past a byte limit, so a multi-buy bundle can lose trailing legs — when logs are empty OR carry "Log truncated", re-decode from the complete inner-instruction self-CPI events and take the larger set. AMM path is still log-only (latent same risk).** |
| `decode/trade.rs` | Borsh `RawTradeEvent`, trade helpers |
| `decode/instructions.rs` | `InstructionKind`, `label_instruction` (per-ix human label) |
| `decode/program_registry.rs` | `program_friendly_name` — program-id → name table (SSOT for naming), backed by a `OnceLock<HashMap>`. Grow via the `unknown-programs` harvest |
| `decode/create.rs` | `decode_create_events_from_logs` |
| `raw_tx.rs` (ingest-core) | `encode_payload` (protobuf wire bytes), `build_raw_tx_event` — **`raw-tx` feature** |
| `backfill.rs` | `rpc_to_protobuf` (RPC result → protobuf) — **`rpc-backfill` feature** |

### Feature gates

| Feature | Unlocks |
| --- | --- |
| `raw-tx` | `IngestEvent::RawTx` (carries protobuf `payload` bytes), `raw_tx::encode_payload` |
| `rpc-backfill` | `serde_json` dep, `backfill::rpc_to_protobuf` |

`live` enables both. `IngestHandle` exposes `set_live`, `track_pools`, `untrack_pools`, `pool_index`, `pools_changed`. (Liveness is tracked host-side by `live`'s own `DbHeartbeat`, not an ingest health channel.)

**Push hooks (`ingest_core::PushHooks`, optional):** the same subscription can carry a `blocks_meta` filter and an `accounts` filter for host-chosen pubkeys; the two callbacks run on the transport task (cheap parse + store only). Hunter's `main.rs` bridges block metas → `Engine::set_cached_blockhash` (blockhash cache, 0 steady-state `getLatestBlockhash`) and nonce-account updates → `Engine::on_nonce_account_update` (durable-nonce push re-arm). Hosts that don't opt in (forge) get a byte-identical subscription. Push updates deliberately don't feed the idle watchdog — it guards the *transaction* stream.

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

**Instruction labeling (`ix_labels`, one string per top-level ix).** `label_instruction` names each ix by a ladder: known program + known discriminator → `"Pump.Fun: Buy"`; known program, unknown disc → `"Pump.Fun: Unknown"`; program in the `program_registry` table → its instruction is decoded too when the program is a covered open (Anchor) one (`"Jupiter Aggregator V6: Route"`), else `"Axiom Trade: Unknown"`; nothing matches → `"Unknown (<full program id>)"`. The unknown fallback carries the **full** program id (not a truncated suffix) so unknowns are self-identifying in the persisted `trades.ix_labels` — the durable label record, since **this deployment does not persist `raw_txs`** (migration 0002 promoted `ix_labels` onto `trades` for exactly this reason). Only *top-level* instructions are labeled (inner CPIs are used for trade recovery, not labels).

*Instruction-level decode (Phase 4)* lives in `program_registry.rs` (`ANCHOR_IX` table + `program_instruction_name`). Anchor discriminators are **computed** from the instruction name (`sha256("global:<snake_name>")[..8]` via `solana_sdk::hash`), never hard-coded — so a wrong name is fail-safe (no match → `: Unknown`, never a wrong label). The mechanism is pinned against pump.fun's known discriminators in a unit test. Non-Anchor programs (Raydium AMM v4's 1-byte tag) and closed bots are intentionally left at `: Unknown`.

To shrink the `Unknown (...)` tail, run the harvest and add the top program IDs to `program_registry.rs`:

```powershell
cargo run -p hunter-live -- unknown-programs [--days N] [--top N]
```

It aggregates `trades.ix_labels`, ranks the still-`Unknown (<id>)` programs by frequency, and prints each with a Solscan link (full-id and legacy-suffix rows shown separately). DB-only (no keys/Helius). Prefer *no* registry entry over a guessed one — a wrong label is worse than `Unknown`.

## Key rules

- `trades` table = this feed. TPSL exit loop confirms fills from this feed (not a separate RPC).
- No blocking I/O / `.await`-on-lock / unbounded alloc per event on the ingest hot path; DB+SSE go through channels.
- `ingest-laserstream` has **zero workspace deps** — standalone drop-in crate.
- **The transport must be in the log filter.** `ingest_core` + `ingest_laserstream` are
  named explicitly in `live/main.rs`'s default `EnvFilter`. They were not until
  2026-08-05, and the box sets no `RUST_LOG` — so every `LaserStream: connecting` /
  `stream error` / `no transaction update … forcing reconnect` /
  `pipeline backpressured` line was dropped, and the sole live transport ran
  invisibly in production. The cost of that: **seven watchdog process kills in one
  day with no evidence of why the feed stopped** — the log showed a healthy process,
  then a kill. Every one of those lines is per-connection, never per-message, so
  there is no hot-path cost to keeping them on. `live::ingest` (host adapter +
  watchdog) was always covered by `live=info`; the crates *underneath* it were not,
  which is exactly the layer that knows why a feed died.

## Watchdog kills are real outages, not false positives (2026-08-05)

`live::ingest::watchdog` force-exits when no **successful** DB write lands for
`watchdog_stall_timeout_secs` (90 s) while live+booted. Seven fired on 2026-08-05.
Checked against the data rather than assumed: `trades` shows a genuine hole at each
one — 13:44 has **zero** rows in a feed averaging ~1500/min, and 12:08→12:09 decayed
1538 → 225 → 103 before the 12:09:47 kill. So the watchdog is reporting a true fault.

Two things follow, neither yet fixed:

- **A kill costs 1–2 min of feed data, permanently.** In-process reconnect replays
  from the last slot (`ReplayAnchor`), but a process exit discards that anchor and
  the restart comes up live — 13:44 is simply absent from `trades` and was never
  backfilled. The killer destroys the state the healer needed.
- **The transport self-heals in ~12 s** (`idle_reconnect_timeout` 10 s +
  `idle_check_interval` 2 s), far inside the 90 s watchdog window, so a plain feed
  stall should never reach the watchdog at all. Something is defeating that
  reconnect for 90 s+. Diagnosing it needs the transport logs above — enable-then-
  wait, do not guess.
