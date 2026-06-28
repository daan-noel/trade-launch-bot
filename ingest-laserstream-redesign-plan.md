# Redesign: `ingest-laserstream` as a standalone, reusable crate

## Context

Goal: make `ingest-laserstream` a **totally isolated, drop-in crate** you can lift into other
projects: *fetch live Helius LaserStream data → decode → return the result*. Nothing more.

Today it does far more than that, and every extra job welds it to this app:

- **Transport + decode** (the genuinely reusable core) — clean.
- **Persistence** — `db_writer.rs` writes to *your* Postgres via `backend_core` repos.
- **State mutation** — mutates `TokenCache`, pings `TradeSignals`.
- **Fan-out** — SSE broadcast, strategy pings, `TraderHook` calls.
- **Process control** — `ingest_health.rs` calls `std::process::exit(1)`; `maintenance.rs` rolls *your* table partitions.

Every module imports `backend_core::{models, state, storage::repositories, config::constants,
services::pda}`, and the protocol constants are scattered across `pump-constants` **and**
`backend_core::config::constants::{discriminators,protocol,tuning}`, plus dozens of hardcoded timing
literals inside `client.rs` / `pipeline.rs` / `db_writer.rs`.

**Goal:** a crate with **zero dependency on `backend-core` / `pump-constants`**, that emits decoded
events out a bounded channel and owns its own constants/config. The host keeps every sink (DB, cache,
SSE, strategy, trader, watchdog, partitions) on its side of a thin adapter.

Decisions locked:
1. **Events-only core** — crate emits decoded events; host owns all sinks.
2. **Bounded `mpsc::Receiver<IngestEvent>`** delivery + a control handle.
3. **Minimal deps, feature-gated** — own lightweight `Pubkey`; `solana-sdk`/`serde_json` behind features; typed `IngestError`.

---

## Design principles

1. **One job: source decoded events.** The crate is a *producer*. It never persists, never mutates app
   state, never calls `process::exit`. A reusable library must be inert toward its host.
2. **Own your types, own your constants.** Decode output structs and all protocol/timing constants live
   *in the crate*. No shared-model coupling.
3. **Static constants → data, not scattered `const`s.** A single `Protocol` descriptor (program IDs +
   discriminators) with a built-in `pump_fun()` default, pre-decoded to bytes once at construction.
4. **Dynamic constants → one `IngestConfig` with `Default`.** Every timeout/interval/cap is a field; the
   crate **never reads env** (host builds the struct).
5. **Preserve the hot-path budgets.** Two-task topology (transport read off the decode path),
   classify-once, lazy base58, bounded channels, no per-event alloc/lock.

---

## Target crate layout

```
ingest-laserstream/
├── Cargo.toml          # deps below; no backend-core, no pump-constants
└── src/
    ├── lib.rs          # builder + Ingest + IngestHandle + re-exports
    ├── config.rs       # IngestConfig (all tunables) + Default
    ├── protocol.rs     # Protocol / Programs / Discriminators + ::pump_fun(); pre-decoded bytes
    ├── pubkey.rs       # lightweight Pubkey ([u8;32] + cached base58)  [minimal-deps choice]
    ├── error.rs        # IngestError (thiserror) — public boundary errors
    ├── event.rs        # IngestEvent + owned domain structs (Trade, TokenCreated, …)
    ├── health.rs       # HealthSnapshot via atomics + watch::Receiver (NO process::exit)
    ├── transport/
    │   ├── mod.rs
    │   ├── client.rs    # connect, subscribe, reconnect/backoff, idle-timer, classify-once
    │   └── request.rs   # SubscribeRequest builder
    ├── decode/
    │   ├── mod.rs       # Decoder, DecodeOutput, TxRelevance
    │   ├── grpc.rs      # protobuf-native decode entry (+ LazyKeys)
    │   ├── trade.rs     # RawTradeEvent borsh, curve + AMM trade leaves
    │   ├── create.rs    # Create/CreateEvent, creator resolution
    │   └── instructions.rs
    ├── pool.rs          # self-contained pool→mint index + PumpSwap pool PDA derivation
    ├── raw_json.rs      # build_raw_json() Helius-shape blob          [feature "raw-json"]
    ├── backfill.rs      # RPC base64 → proto adapter                  [feature "rpc-backfill"]
    └── proto/           # generated prost/tonic bindings (unchanged)
```

**Stays (core):** `transport/`, `decode/`, `pool.rs`, `protocol.rs`, `config.rs`, `event.rs`,
`health.rs`, `proto/`, plus `raw_json.rs` / `backfill.rs` behind features.
**Leaves to host:** `db_writer.rs`, `ingest_health.rs` (the watchdog/`process::exit`), `maintenance.rs`,
`trader_hook.rs`, and all `TokenCache`/`TradeSignals`/`SseEvent`/`StrategyPing` wiring.

---

## Public API (concrete)

```rust
// ---- construction (builder) ----
let ingest = Ingest::builder()
    .endpoint(url)                       // String
    .api_key(key)                        // String
    .protocol(Protocol::pump_fun())      // or a custom descriptor
    .config(IngestConfig::default())     // all tunables (override fields as needed)
    .build()?;                           // -> Result<Ingest, IngestError> (decodes program IDs once)

// ---- run: returns the event stream + a control handle ----
let (mut events, handle): (mpsc::Receiver<IngestEvent>, IngestHandle) = ingest.start();

// ---- consume (host owns everything downstream) ----
while let Some(ev) = events.recv().await {
    match ev {
        IngestEvent::TokenCreated(c) => { /* persist / cache / SSE … */ }
        IngestEvent::Trade(t)        => { /* persist / feed trader / signal … */ }
        IngestEvent::TokenMigrated(m)=> { /* … */ }
        IngestEvent::Liquidity(l)    => { /* … */ }
        IngestEvent::CreatorActivity(a) => { /* … */ }
        #[cfg(feature = "raw-json")]
        IngestEvent::RawTx(r)        => { /* persist Helius-shape blob */ }
    }
}

// ---- control handle ----
handle.set_live(false);                       // pause/resume (watch<bool> under the hood)
handle.track_pools(pairs);                     // warm-restart pool seeding (host-driven, replaces DB poll)
handle.untrack_pools(pools);
let h: HealthSnapshot = handle.health();        // cheap atomic read
let mut hw = handle.health_watch();             // watch::Receiver<HealthSnapshot> for a host watchdog
handle.shutdown().await;                         // cancels tasks, drains, joins
```

`IngestHandle` owns: the `live` `watch::Sender<bool>`, the internal pool index registration, the health
reader, and the `JoinHandle`s (so the host shuts down gracefully instead of receiving raw handles like
today's `IngestHandles`).

**Gone from the signature** vs today's `spawn(...)`: `db`, `token_cache`, `sse_tx`, `settings_rx`,
`trader`, `trade_signals`, `strategy_rx`. Those are host concerns now.

---

## Static constants → `protocol.rs`

A descriptor, decoded to bytes once at `build()` so the hot path compares `[u8;32]`/`[u8;8]`, never base58.

```rust
pub struct Protocol {
    pub programs: Programs,
    pub discriminators: Discriminators,
    pub lamports_per_sol: f64,         // unit scale used by trade decode
    pub min_trade_lamports: u64,       // dust filter (was MIN_TRADE_LAMPORTS)
}

pub struct Programs {           // each holds base58 input + pre-decoded [u8;32] + cached String
    pub pump_fun: Pubkey, pub pump_swap: Pubkey,
    pub token: Pubkey, pub token_2022: Pubkey,
    pub associated_token: Pubkey, pub system: Pubkey, pub compute_budget: Pubkey,
}

pub struct Discriminators {     // [u8;8] each — instruction + event
    pub buy: [u8;8], pub sell: [u8;8],
    pub buy_exact_sol_in: [u8;8], pub buy_exact_quote_in: [u8;8],
    pub buy_v2: [u8;8], pub buy_exact_quote_in_v2: [u8;8],
    pub create_ix: [u8;8], pub create_v2_ix: [u8;8],
    pub migrate_ix: [u8;8], pub migrate_v2_ix: [u8;8],
    pub trade_event: [u8;8], pub create_event: [u8;8], pub anchor_event_cpi: [u8;8],
    pub pump_swap_buy_event: [u8;8], pub pump_swap_sell_event: [u8;8],
}

impl Protocol { pub fn pump_fun() -> Self { /* current literal values, baked in */ } }
```

Decoder modules stop importing `backend_core::config::constants::*` and read from `&Protocol` instead
(carried in the `Decoder` struct, alongside today's `pump_program_id_bytes`).

**Source-of-truth split (clean, accepts a few duplicated literal program-ID strings):**
- **Discriminators** are ingest-only → move fully into the crate; delete from `backend-core`.
- **Program IDs** → crate owns its copy in `Programs`; the trader keeps `pump-constants`. Minor, deliberate
  duplication is the price of true isolation. (If you later want one source, the *app* can build its
  consts from `ingest::Protocol::pump_fun()`, not the reverse.)
- **Trading constants stay put** — CU budgets, Jito tips, confirm-retry schedule in `pump-constants` are
  *not* ingest concerns and don't move.

---

## Dynamic constants → `config.rs`

Every hardcoded literal from `client.rs` / `pipeline.rs` / `db_writer.rs` becomes a field with the
current proven value as its `Default`. The crate reads only this struct — never env.

```rust
pub struct IngestConfig {
    // transport / reconnect
    pub connect_timeout: Duration,          // 10s
    pub reconnect_base: Duration,           // 1s
    pub reconnect_max_backoff: Duration,    // 30s
    pub idle_reconnect_timeout: Duration,   // 10s   (silent-stall detector)
    pub idle_check_interval: Duration,      // 2s
    pub http2_keepalive: Duration,          // 30s
    pub tcp_keepalive: Duration,            // 30s
    pub max_decoding_message_size: usize,   // 64 MiB
    pub pipeline_send_timeout: Duration,    // 10s   (output full > this → drop conn & reconnect = shed)
    pub resubscribe_debounce: Duration,     // 250ms
    pub commitment: Commitment,             // Processed

    // channels
    pub update_channel_cap: usize,          // 4096  (transport → decode)
    pub event_channel_cap: usize,           // output mpsc to host

    // pool tracking (AMM, post-migration)
    pub track_amm: bool,                    // false → never subscribe pools, skip AMM decode entirely
    pub pool_refresh_interval: Duration,    // 120s  (host-driven re-seed cadence hint)
    pub pool_activity_window: Duration,     // 3h
}
impl Default for IngestConfig { /* the values above */ }
```

Optional `from-env` feature can add `IngestConfig::from_env()` for convenience, but the **core never
depends on it**. Live-tunable knobs you actually need at runtime (e.g. `set_live`) go through the
`IngestHandle`, not a settings struct — `AppSettings` coupling disappears.

---

## Owned domain types → `event.rs`

The crate defines its **own** output structs (no `backend_core::models`). Shapes mirror what decode
already produces, so the host's `From` impls are mechanical.

```rust
pub enum IngestEvent {
    TokenCreated(TokenCreated),
    Trade(Trade),
    TokenMigrated(TokenMigrated),
    Liquidity(LiquidityEvent),
    CreatorActivity(CreatorActivityEvent),
    #[cfg(feature = "raw-json")] RawTx(RawTx),   // Helius-shape blob for persistence
}

pub struct Trade {
    pub mint: String, pub wallet: String, pub side: Side,      // Buy | Sell
    pub sol: f64, pub tokens: f64, pub price: f64,
    pub signature: String, pub leg_index: u32, pub slot: u64,
    pub block_time: Option<DateTime<Utc>>, pub received_at: DateTime<Utc>,
    pub reserves: Reserves,                                    // virtual/real sol+token
    pub venue: Venue,                                          // Curve | Amm
    pub instruction_type: String, pub instruction_labels: Vec<String>,
}
// TokenCreated / TokenMigrated{ mint, pool } / LiquidityEvent / CreatorActivityEvent likewise.
```

The **host** owns the boundary mapping (keeps the crate clean), e.g. in `backend-core`:
`impl From<ingest::Trade> for backend_core::models::trade::Trade { … }`.

`instruction_labels` becomes `Vec<String>` (not `serde_json::Value`) so the core needs no `serde_json`;
the host serializes to JSON when persisting.

---

## Internal task topology (preserves hot-path budgets)

```
transport::client (gRPC read loop)
   • connect + reconnect/backoff + idle-timer
   • classify-once (cheap log scan) → TxRelevance (Curve|Amm)
        │  mpsc(update_channel_cap=4096): (Arc<SubscribeUpdateTransaction>, TxRelevance)
        ▼
decode task
   • Decoder::decode_relevant(update, relevance, &protocol)  (skips re-scan; lazy base58)
   • maintains self-owned pool→mint index; on TokenMigrated derives pool PDA → registers → resubscribe
   • (feature raw-json) synthesize RawTx off the read loop
        │  mpsc(event_channel_cap): IngestEvent
        ▼
host consumer (drains receiver) — owns DB/cache/SSE/strategy/trader/watchdog/partitions
```

- **Two tasks** keep decode off the socket read loop (a decode hiccup never stalls gRPC) — same split as
  today's client→pipeline.
- **Backpressure/shed:** a slow host fills `event_channel_cap`; the transport's `pipeline_send_timeout`
  fires → drop+reconnect (sheds the burst). The money-critical-vs-recomputable shedding *that used to
  live at the DB boundary* now lives in the **host's** consumer, where the DB is.
- **No `process::exit`.** `health.rs` publishes `HealthSnapshot { connected, last_event_at, last_slot,
  reconnects, events_total, dropped_on_full }` via atomics + a `watch`. The **host binary** implements
  "exit/restart if stalled" using `handle.health_watch()` (replaces `ingest_health.rs`).

---

## Pool tracking & migration loop (now self-contained)

- The crate owns its **own** `pool→mint` `DashMap` (separate from the host's rich `TokenCache`).
- `derive_pump_swap_pool` PDA logic **moves into `pool.rs`** (it's protocol math, not app logic) — drops
  the `backend_core::services::pda` dependency.
- On a decoded `TokenMigrated`, the crate derives the pool PDA, registers it, and pings the client to
  resubscribe — **no host involvement** for live discovery.
- For **warm restarts**, the host seeds known-active pools via `handle.track_pools(...)` (replacing the
  old DB-polling `pool_subscription_refresh`). DB stays entirely on the host side.

---

## Dependencies & features (`Cargo.toml`)

**Core:** `tonic`, `prost`, `tokio`, `tokio-stream`, `dashmap`, `bs58`, `base64`, `borsh`, `chrono`,
`tracing`, `thiserror`, `futures-util`.
**Removed:** `backend-core`, `sqlx`, `uuid`, `serde`/`serde_json` (from core), `solana-sdk`, `bincode`.

**Features:**
- `raw-json` → `serde_json`; enables `raw_json.rs` + `IngestEvent::RawTx`.
- `rpc-backfill` → `solana-sdk`, `bincode`; enables `backfill.rs` (base64 RPC → proto for token-sync).
- `from-env` (optional) → `IngestConfig::from_env()` helper.
- `default = []` — bare core compiles with **no** project or heavy deps.

`pubkey.rs` provides the lightweight `Pubkey` (`[u8;32]` + cached base58) so the core avoids
`solana-sdk`; `rpc-backfill` may convert to/from `solana_sdk::Pubkey` internally.

---

## What the host (backend-core / backend-deploy) gains

A thin **adapter/consumer** (greenfield wiring, replacing today's in-crate sinks):

1. Build `Protocol`/`IngestConfig` from `Settings` (env stays in the app).
2. `ingest.start()` → spawn a consumer task draining `events`.
3. In the consumer: `From`-map crate events → `backend_core::models`, then run the **moved** batching
   `db_writer` (dedup/batch logic is good — it just lives host-side now against your repos), update
   `TokenCache`/`TradeSignals`, broadcast `SseEvent`, derive `StrategyPing`, feed `TraderHook`
   (`update_live_reserves` reads straight off `Trade.reserves`; `prewarm_amm_pool` on first AMM trade).
4. Own the **watchdog** (via `handle.health_watch()`) and **partition maintenance** in the app.

---

## Verification

- **Standalone proof:** `cargo check -p ingest-laserstream --no-default-features` compiles with **no**
  `backend-core`/`pump-constants`/`sqlx` in the dep tree (`cargo tree -p ingest-laserstream` confirms).
  Also `cargo check -p ingest-laserstream --all-features`.
- **Decode unit tests (the big payoff):** record a few real `SubscribeUpdateTransaction` protobufs as
  fixtures; assert `Decoder::decode_relevant` yields the right `IngestEvent` for each case (curve buy/
  sell, AMM buy/sell, create, create_v2, migrate, liquidity). No network, fully deterministic.
- **Example binary:** `examples/dump.rs` — `Ingest::builder().endpoint().api_key().build()?.start()` and
  print events. Proves a new project needs only endpoint + key.
- **Host parity:** run `backend-deploy` against the new crate + adapter; confirm trades/tokens/raw_txs
  land in Postgres identically, SSE ticks, watchdog still trips on simulated stall.
- **Hot-path review:** classify-once preserved, lazy base58 intact, bounded channels at the documented
  caps, zero per-event alloc/lock on transport→decode→emit, no blocking I/O in either task.
