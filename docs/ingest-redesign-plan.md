# Ingest redesign — reconciled plan (`ingest-laserstream` → `ingest-core` + `ingest-pumpfun`)

**Status:** authored 2026-07-09 on branch `feat/restructure-hunter-forge`. This is the *real* Part 3
plan — it **supersedes** the `⚠DERIVED` scaffold in `restructure-execution-plan.md` §PART 3. It was
reconciled against the actual `shared/ingest-laserstream` source, not inferred. Read side of the
restructure; **totally separate stack from Part 2** — no crate crosses between the two.

Read side splits the 20-file `shared/ingest-laserstream` crate into a venue-agnostic transport/
pipeline engine (`ingest-core`) + a pump.fun venue (`ingest-pumpfun`), makes the provider a config
axis, and keeps both products reading through their existing `live/src/ingest/` bridge modules.
**Every phase ends with a green gate; hunter + forge ingest behavior stays byte-identical.**

Target crates:
- **`ingest-core`** (`shared/ingest/core/`, pkg `ingest-core`, lib `ingest_core`) — third-party +
  `solana-sdk` only, venue-agnostic.
- **`ingest-pumpfun`** (`shared/ingest/pumpfun/`, pkg `ingest-pumpfun`, **lib stays `ingest_laserstream`**),
  deps `ingest-core`. The kept lib name + a back-compat façade = **zero consumer source edits** (mirrors
  Phase A keeping lib `pump_trader` on `executor-pumpfun`).
- **`ingest-websocket`** (`shared/ingest/websocket/`) — already positioned in Phase 1.1; stays a stub.

---

## Ground truth — what the source actually looks like (reconciliation notes)

Read before touching code; these are the facts the DERIVED scaffold got wrong or omitted.

**Consumers (only three; `lab` links neither):**
- `hunter/live` — `ingest-laserstream = { path = "…", features = ["raw-tx", "rpc-backfill"] }`.
- `forge/live` — `ingest-laserstream.workspace = true` (workspace key, `raw-tx` feature).
- `hunter/core` / `hunter/live` also have four **stale doc-comment** mentions
  (`ingest_laserstream::{TraderHook, client, adapter_rpc}`) — **not real code**, no façade needed.
- `forge/lab` names the crate **only in partition-rule comments** — it does *not* depend on it. Good.

**Real public surface consumers compile against** (the façade MUST preserve every path):
`ingest_laserstream::{Ingest, IngestBuilder, IngestConfig, IngestHandle, IngestEvent, Protocol,
PoolIndex, Commitment, Result, IngestError}` + modules `::event ::proto ::backfill ::slot_anchor
::decode ::transport ::config ::protocol ::pool` (+ `::raw_tx` under the `raw-tx` feature).

**The coupling reality (why this is NOT a pure `git mv`):** the transport is pump-coupled today —
- `transport::classify_tx` scans log messages for the `pump_fun` / `pump_swap` base58 IDs.
- `transport::build_subscribe_request` hardcodes the `"pumpfun"` filter key; `account_includes`
  seeds the list with the pump program id + tracked pool PDAs.
- `transport::run` / `run_once` take `Arc<Protocol>` and thread the two program IDs throughout.
- `Ingest::start` wires the concrete pump `Decoder` + `pool::register_pool` (PumpSwap PDA math) into
  the generic transport + `IngestHandle::track_pools`.
So "venue-agnostic core" requires a **trait seam** (`IngestVenue`), exactly analogous to how Phase A
introduced the `Engine`/`Venue` seam on the write side. That seam is **Phase H**, not G.

**Module → crate placement (verified against source):**

| module | lands in | why |
|---|---|---|
| `error.rs` (`IngestError`/`Result`) | core | generic; `InvalidProgramId` is just a bs58/len parse error |
| `config.rs` (`IngestConfig`/`Commitment`) | core | pure tunables, no pump refs |
| `proto/` (geyser + confirmed_block wire) | core | Yellowstone gRPC wire types — venue-neutral |
| `slot_anchor.rs` | core | pure slot→time math (`400 ms/slot`), no pump refs |
| `backfill.rs` (`rpc-backfill`) | core | RPC JSON → neutral `SubscribeUpdateTransaction`; generic `VersionedTransaction` decode |
| `raw_tx.rs` (`raw-tx`) | core | verbatim wire bytes → `RawTx`; no interpretation |
| `event.rs` (`IngestEvent` + subs) | core | host-facing neutral output (already "host maps via `From`") |
| transport connect mechanics (`connect`, `XTokenInterceptor`, `TransportConfig`, reconnect loop) | core | generic once the classifier/subscription come from the venue |
| **NEW** `venue.rs` (`IngestVenue` trait, `DecodeOutput`, `PoolIndex`) | core | the seam |
| `protocol.rs` (`Protocol`/`Programs`/`Discriminators`/IDs/`program_friendly_name`) | pumpfun | pump.fun constants |
| `pool.rs` (PumpSwap PDA derivation) | pumpfun | pump-swap PDA math |
| `decode/` (`grpc`,`create`,`trade`,`instructions`,`mod::Decoder`,`TxRelevance`) | pumpfun | pump decoders |
| **NEW** `PumpFunVenue` (impls `IngestVenue`) | pumpfun | wraps `Protocol`+`Decoder`+pool derivation |

> `IngestEvent`'s `Venue::{Curve,Amm}` + `BuyInstructionArgs::Buy*` are pump-flavored *names* but are
> the neutral data the host already consumes — they move to core as the crate's output contract
> (same call the executor split made keeping `IngestEvent` host-facing). If a second venue ever needs
> a different shape, generalize then; do **not** pre-abstract it now.

---

## The seam (Phase H design, stated up front so G can be reviewed against it)

```rust
// ingest-core/src/venue.rs
pub type PoolIndex = Arc<DashMap<String, String>>;   // pool PDA -> base mint (neutral)

pub enum DecodeOutput { Events(Vec<IngestEvent>), Ignored }

/// One venue = one program family's classify + decode + pool derivation.
pub trait IngestVenue: Send + Sync + 'static {
    /// Venue-owned relevance tag the transport carries opaquely (pump: Curve|Amm).
    type Relevance: Copy + Send + 'static;

    /// Accounts to put in the gRPC `account_include` filter, given tracked pools.
    fn subscription_accounts(&self, pools: &PoolIndex) -> Vec<String>;
    /// gRPC filter map key (was the hardcoded "pumpfun").
    fn filter_key(&self) -> &'static str;
    /// Cheap pre-filter on a raw update; `None` => ignore (don't forward to decode).
    fn classify(&self, update: &SubscribeUpdateTransaction) -> Option<Self::Relevance>;
    /// Full decode of a relevant update into neutral events.
    fn decode(&self, update: &SubscribeUpdateTransaction, r: Self::Relevance,
              received_at: DateTime<Utc>) -> DecodeOutput;
    /// Pool PDA for a mint (for `track_pools`); `None` if the venue has no pools.
    fn derive_pool(&self, mint: &str) -> Option<String>;
}
```

- **Static dispatch**, no `Box<dyn>`: `Ingest<V: IngestVenue>`, `transport::run<V>` — mirrors the
  executor's static `Venue`. Relevance is `V::Relevance` (pump = `TxRelevance`).
- **Provider = config** (second half of H): add `Auth` (today: `XToken(String)`; extensible to
  bearer/none) + `endpoint` + capability flags to `ingest-core` config, so Helius → Triton/Shyft
  (all Yellowstone gRPC) is a config change, **no new crate**. `XTokenInterceptor` generalizes to
  read the `Auth`.
- **Façade** (`ingest-pumpfun/src/lib.rs`): `pub use ingest_core::{…}` for every moved item +
  `pub mod protocol/pool/decode`, plus `pub type Ingest = ingest_core::Ingest<PumpFunVenue>` and an
  `IngestBuilder` shim whose `.protocol(Protocol)` constructs the `PumpFunVenue`. Consumers keep
  `Ingest::builder().endpoint(..).api_key(..).protocol(Protocol::pump_fun()).config(..).build()?`
  **unchanged**.

---

## Phase G — Reposition + create crates + move the venue-neutral **leaves** (no seam yet) — ✅ DONE (2026-07-09)
Pure structural move; the trait seam is deferred to H so G stays a true no-behavior-change step and
its gate is cheap to trust.
- [x] `git mv shared/ingest-laserstream shared/ingest/pumpfun` (history-preserving); pkg
      `ingest-pumpfun`, **lib kept `ingest_laserstream`**. Created `shared/ingest/core` (pkg
      `ingest-core`, lib `ingest_core`). Root `[workspace] members`: added `shared/ingest/core` +
      `shared/ingest/pumpfun`, dropped `shared/ingest-laserstream`. Workspace dep **key**
      `ingest-laserstream` kept via `package = "ingest-pumpfun"` + new path; added an `ingest-core`
      workspace entry.
- [x] Moved to **`ingest-core`** the zero-pump-coupling modules: `error.rs`, `config.rs`, `event.rs`,
      `proto/` (= `generated/` + the `include!`), `slot_anchor.rs`, `raw_tx.rs` (feat `raw-tx`),
      `backfill.rs` (feat `rpc-backfill`). Feature defs live in `ingest-core`; `ingest-pumpfun` forwards
      (`raw-tx = ["ingest-core/raw-tx"]`, `rpc-backfill = ["ingest-core/rpc-backfill"]`) so consumer
      flags are unchanged. Dep split: `serde_json`(opt)/`bincode` follow backfill to core; **`base64`
      stayed dual** (core backfill + pumpfun decoders `create.rs`/`trade.rs`); `solana-sdk`/`bs58`/
      `chrono` needed both sides; `borsh`/`dashmap`/`tokio`/`tokio-stream` stay pumpfun.
- [x] Transport + `Ingest`/`IngestHandle` + `decode/` + `protocol.rs` + `pool.rs` **stay in
      `ingest-pumpfun`**; still concrete pump (no generic seam yet — that's Phase H). They reach core's
      moved types via the façade's crate-root re-exports (internal `crate::proto`/`crate::config`/
      `crate::error` resolve through `pub use ingest_core::{…}`, so **zero edits** to those modules).
- [x] **Back-compat façade** in `ingest-pumpfun/src/lib.rs`: `pub use ingest_core::{config, error,
      event, proto, slot_anchor}` (+ cfg-gated `backfill`/`raw_tx`) + top-level `IngestConfig`/
      `IngestEvent`/`IngestError`/`Result`/`Commitment`. Repointed the **two** consumer Cargo deps
      (hunter/live path+package, forge/live via workspace) + the workspace entries; **zero `.rs` edits**
      in hunter/forge.
- **Gate G:** ✅ `cargo build -p hunter-live -p forge-live` green via façade (both feature sets);
      `cargo check --workspace` = 0 errors; `cargo test -p ingest-core -p ingest-pumpfun` green
      (`transport::tests::reason_labels` passes; core 0 tests); `cargo tree -i ingest-core`/`-i
      ingest-pumpfun` from `hunter-lab`/`forge-lab` = **no match** (partition holds), while both appear
      in the `*-live` trees; `git status` = clean history-preserving renames only, no strays/secrets.
- **Note:** `ingest-websocket` references the old crate name only in its `description` string (its real
      dep is `trading_core`) — left as-is; the doc-comment references in `hunter/core` + `hunter` CLAUDE.md
      are stale prose, folded into the Phase I/doc-reconciliation pass, not a compile concern.

## Phase H — `IngestVenue` seam + generic transport/`Ingest` + provider-as-config — NOT STARTED
The design-heavy phase; still behavior-preserving (identical wire + identical events).
- [ ] Add `ingest-core/src/venue.rs`: `IngestVenue` trait (above), `DecodeOutput`, `PoolIndex`
      (moved out of `pool.rs`). Move the transport connect mechanics + reconnect loop into
      `ingest-core/src/transport/` and make `connect`/`run`/`run_once`/`build_subscribe_request`
      **generic over `V: IngestVenue`** — `classify_tx` → `venue.classify`, `account_includes` →
      `venue.subscription_accounts`, the `"pumpfun"` key → `venue.filter_key`. Move `Ingest`/
      `IngestBuilder`/`IngestHandle`/`start` into core as `Ingest<V>` (the decode task calls
      `venue.decode`; `track_pools`/`untrack_pools` call `venue.derive_pool`).
- [ ] In `ingest-pumpfun`: add `PumpFunVenue { protocol: Arc<Protocol>, decoder-state }` implementing
      `IngestVenue` (`Relevance = TxRelevance`; `classify` = the old `classify_tx`; `decode` wraps the
      existing `Decoder::decode_relevant_pb` + the `raw-tx` append; `subscription_accounts` = pump id
      + pools; `derive_pool` = `pool::derive_pool`). `Decoder`/`TxRelevance` stay pumpfun-owned.
- [ ] Provider-as-config: add `Auth` + `endpoint`/capability fields to core config; generalize
      `XTokenInterceptor` to the `Auth`. Default path = Helius x-token (byte-identical to today).
- [ ] Façade: `pub type Ingest = ingest_core::Ingest<PumpFunVenue>` + `IngestBuilder` shim mapping
      `.protocol(Protocol)` → `PumpFunVenue`. Consumers still unchanged.
- **Gate H:** decoder unit tests over fixture logs (curve + AMM, incl. the truncated-log fallback)
      green; `cargo build -p hunter-live -p forge-live` green; a provider-config swap (endpoint/auth)
      compiles + type-checks with **no crate change**; `cargo tree` partition still holds (no `*-lab`
      linkage). Zero SOL, zero network for the unit portion; a live feed smoke test is EC2-gated.

## Phase I — Confirm per-product `live/src/ingest/` bridges read core+pumpfun directly — NOT STARTED
- [ ] Confirm `forge/live/src/ingest/` (`consumer.rs`/`map.rs`/`pumpfun.rs`/`mod.rs`, folded from the
      former `ingest-host` crate in Phase 1.3) + `hunter/live/src/ingest/` consume `ingest-core` +
      `ingest-pumpfun` **through the façade** with no behavior change; both products read identically
      (shared ingest stack + per-product `src/ingest/` bridge). No new crate.
- [ ] Carry forward the known decode fixes as guarded behavior + regression tests: **log-truncation
      dropped-legs** (use inner-ix CPI events on `"Log truncated"`, per the `log-truncation-dropped-legs`
      memory) and the still-exposed **AMM-path** truncation (verify/close).
- **Gate I:** `cargo tree -p forge-live` / `-p hunter-live` show `ingest-core` + `ingest-pumpfun` but
      no `ingest-host` crate; `cargo tree -p *-lab` link neither; the two truncation regression tests pass.

## Phase J — WebSocket transport stub — NOT STARTED
- [ ] Keep `shared/ingest/websocket` as the stub second-transport sibling to `core`; flesh out **only**
      if actually moving off gRPC, else leave documented. If touched, it consumes the same
      `IngestVenue`/`IngestEvent` seam so a venue is transport-agnostic.
- **Gate J:** stub compiles as a workspace member; no consumer forced to link it.

**Part 3 exit gate:** full-workspace `cargo build` green; `scripts/dep-partition-check.{sh,ps1}` passes;
both `*-live` bins ingest the live feed with no behavior regression vs. pre-split.

---

## Invariants to preserve (bug if violated)
- `IngestEvent` output shape byte-identical across the split (host `From` impls unchanged).
- Reconnect semantics unchanged: replay-from-last-slot on graceful/idle, **live fallback** on
  pipeline-backpressure / `ResourceExhausted` (anti-billing-storm), gap-replay off by default (300 s).
- Pre-filter still runs in the **transport** task (classify once), not re-scanned in decode.
- `raw-tx` / `rpc-backfill` features gate the same code; consumer feature flags unchanged.
- `lab` never links the ingest stack (`cargo tree -p *-lab` = none) — the partition rule the crate
  comments already assert.
- Decode fixes preserved: inner-ix CPI fallback on truncated logs.
```
