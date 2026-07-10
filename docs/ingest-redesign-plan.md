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

## Phase H — `IngestVenue` seam + generic transport/`Ingest` + provider-as-config — ✅ DONE (2026-07-09)
The design-heavy phase; still behavior-preserving (identical wire + identical events).
- [x] Added `ingest-core/src/venue.rs`: `IngestVenue` trait + `DecodeOutput` + `PoolIndex` (moved
      out of `pool.rs`/`decode/mod.rs`). Moved the whole transport (connect mechanics + reconnect
      loop) into `ingest-core/src/transport/` and made `connect`/`run`/`run_once`/
      `build_subscribe_request` **generic over `V: IngestVenue`** — `classify_tx` → `venue.classify`,
      `account_includes` → `venue.subscription_accounts`, the `"pumpfun"` key → `venue.filter_key`.
      Moved `Ingest`/`IngestHandle`/`start` into core (`session.rs`) as `Ingest<V>`/`IngestHandle<V>`
      (decode task calls `venue.decode` + the `raw-tx` append; `track_pools`/`untrack_pools` call
      `venue.derive_pool`). The moved transport reason-labels test rode along and passes.
- [x] **Deviation (venue-owns-pools):** the trait design's `subscription_accounts(&self, pools:
      &PoolIndex)` became `subscription_accounts(&self)` + added `pool_index(&self)` /
      `pools_changed(&self)` methods — the venue owns the shared `PoolIndex` + resubscribe `Notify` so
      its `Decoder` and the transport share exactly one instance (an auto-discovered pool becomes a
      subscription account with no cross-task hand-off). `Ingest::start` reads them off the venue
      instead of creating them. Behavior identical to the old `start()` (`with_pools_changed` always,
      `with_pool_index` only when `track_amm`).
- [x] `ingest-pumpfun/src/venue.rs`: `PumpFunVenue { protocol: Arc<Protocol>, decoder, pool_index,
      pools_changed }` implements `IngestVenue` (`Relevance = TxRelevance`; `classify` = the old
      `classify_tx`; `decode` wraps `Decoder::decode_relevant_pb`; `subscription_accounts` = pump id +
      pools; `derive_pool` = `pool::derive_pool`; `filter_key` = `"pumpfun"`). `Decoder`/`TxRelevance`
      stay pumpfun-owned; `DecodeOutput`/`PoolIndex` re-exported from core at their old paths.
- [x] Provider-as-config: added `Auth { XToken(String) | None }` to core config; `XTokenInterceptor`
      generalized to insert the `x-token` header only when present (`Auth::None` = self-hosted geyser,
      no header). Default path = Helius x-token (byte-identical). `Ingest::new(endpoint, auth, venue,
      config)` takes the `Auth`. Guarded by unit tests (`interceptor_inserts_x_token_only_when_present`,
      `provider_swap_is_config`, `build_subscribe_request_honors_venue_filter_key`).
- [x] **Façade (deviation — newtype, not `pub type`):** `Ingest` is a thin newtype over
      `ingest_core::Ingest<PumpFunVenue>` (a bare type alias can't carry the pump-specific
      `.protocol()` builder method — orphan rule), `IngestHandle = ingest_core::IngestHandle<PumpFunVenue>`
      (alias), `PoolIndex` re-exported from core. `IngestBuilder` shim maps `.protocol(Protocol)` →
      `PumpFunVenue::new` + `.api_key(k)` → `Auth::XToken(k)`. The standalone replay path keeps a
      `transport` shim (`connect(endpoint, api_key, cfg)` = core `connect` + `Auth::XToken`;
      `build_subscribe_request(accounts, from_slot, commitment)` = core keyed `"pumpfun"`). **Zero
      consumer `.rs` edits.** Pruned now-unused `tonic`/`prost`/`tokio-stream` from `ingest-pumpfun`.
- **Gate H:** ✅ `cargo check -p ingest-core -p ingest-pumpfun` green (default + `raw-tx,rpc-backfill`);
      `cargo build -p hunter-live -p forge-live` green **via the façade, zero source edits**;
      `cargo check --workspace` = 0 errors; `cargo test -p ingest-core -p ingest-pumpfun` green (4
      transport/seam tests); provider-config swap (endpoint/Auth) type-checks with **no crate change**
      (`provider_swap_is_config`); `cargo tree -i` partition holds — `hunter-lab`/`forge-lab` link
      **neither** `ingest-core` nor `ingest-pumpfun`, both appear in the `*-live` trees. Zero SOL, zero
      network. ⚠ decoder **fixture** regression tests (curve/AMM/truncated-log) + a live-feed smoke
      test are deferred to Phase I (the decoder code is a byte-unchanged move here, so behavior is
      preserved; Phase I is where the truncation regression tests are explicitly scoped) / EC2-gated.

## Phase I — Confirm per-product `live/src/ingest/` bridges read core+pumpfun directly — ✅ DONE (2026-07-09)
- [x] Confirmed `forge/live/src/ingest/` (`consumer.rs`/`map.rs`/`pumpfun.rs`/`mod.rs`, folded from the
      former `ingest-host` crate in Phase 1.3) + `hunter/live/src/ingest/` consume `ingest-core` +
      `ingest-pumpfun` **through the façade** with no behavior change — both `*-live` bins build green
      with **zero `.rs` edits**; `cargo tree -p forge-live`/`-p hunter-live` show `ingest-core` +
      `ingest-pumpfun` and **no `ingest-host` crate** (it was folded into `forge/live/src/ingest/` in
      Phase 1.3 and never re-created).
- [x] **Log-truncation dropped-legs** (curve + AMM): carried forward and now **guarded + generalized**.
      The recovery gate was extracted to the pure SSOT `should_consult_inner_events(log_count,
      logs_truncated)` fn (shared by both paths) with a regression test
      (`inner_recovery_fires_on_partial_and_full_truncation`) that pins the exact bug: recovery must
      fire on **partial** truncation (3-of-4 bundle: non-empty yet incomplete), not only on empty logs.
      The gate stays cheap (length + bool) so the hot path still decodes inner instructions only when
      it must.
- [x] **AMM-path truncation CLOSED** (was the standing gap): added `trade::decode_pump_swap_trades_from_inner`
      — the AMM twin of the curve `decode_trade_events_from_inner_pb`, recovering PumpSwap `Buy/Sell`
      events from the pump_swap program's anchor self-CPI **inner instructions** (not subject to the log
      byte limit). Generalized `find_pump_pb_ixs` → `find_program_pb_ixs` (any program id) so both paths
      share the ix-finder, and wired `decode_amm_live_pb` to consult inner events through the same
      `should_consult_inner_events` gate + "take whichever yields more" rule. A truncated multi-swap AMM
      bundle no longer under-counts legs.
- **Gate I:** ✅ `cargo tree -p forge-live`/`-p hunter-live` = `ingest-core` + `ingest-pumpfun`, no
      `ingest-host`; `cargo tree -i` from both `*-lab` = neither; the truncation regression test passes
      (`cargo test -p ingest-pumpfun`, warning-free). ⚠ a live-feed no-regression smoke on both `*-live`
      bins (incl. a captured truncated-AMM-bundle replay) is EC2/network-gated.

## Phase J — WebSocket transport stub — ✅ DONE (2026-07-09)
- [x] Kept `shared/ingest/websocket` as the stub second-transport sibling to `core` (its real dep is
      `hunter-core`, not the ingest stack). Refreshed its stale `description` to point at the
      `ingest-core` `IngestVenue`/`IngestEvent` seam it will consume if/when gRPC is swapped out;
      **not** fleshed out (no move off gRPC).
- **Gate J:** ✅ `cargo check -p ingest-websocket` green (compiles as a workspace member); no consumer
      depends on it — the only external mention is a doc-comment in `hunter/core/src/ingest.rs`, not a
      Cargo dep.

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
