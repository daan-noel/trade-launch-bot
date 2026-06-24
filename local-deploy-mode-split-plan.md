# Plan: Split `backend` into isolated `deploy` and `local` crates

> **How to use this file.** Each `### T<n>` block is one self-contained refactoring task
> sized for a single Sonnet session. Hand Sonnet: (1) this file, (2) the task ID.
> Every task ends with a **Verify** that must pass before moving on. Tasks are ordered;
> do not skip. The **Design reference** at the bottom is the source of truth tasks cite.
>
> **Strategy:** extract `pump-constants` (Phase 0) → split `AppState` *in-place* (Phase 1,
> still one `backend` crate, identical runtime) → extract `backend-core` (Phase 2) → stand up
> the two bins (Phases 3–4) → delete old crate (Phase 5). The app compiles and behaves
> identically until Phase 5.

---

## Conventions (apply to EVERY task)

1. **No behavior change.** This is a pure restructuring. Routes, responses, logging, and
   runtime behavior stay byte-identical until the crates are physically separate.
2. **Green gate.** A task is done only when its **Verify** command passes with **no new
   warnings**. Use `--target-dir target-check` if `backend.exe` is running.
3. **Import-fix pattern when moving a module to another crate:** move the file(s); add
   `pub mod X;` to the destination crate's `lib.rs`; in the old crate's `main.rs`/`lib.rs`
   replace `mod X;` with `pub use <dest_crate>::X;` so existing `crate::X::…` paths still
   resolve. Move modules **leaf-first** (never move a module that still references a module
   left behind).
4. **State during Phase 1:** the three new state structs hold **clones of the same handles**
   as `AppState` (every field is an `Arc`/`PgPool`/`watch::Sender` — clone = cheap refcount
   bump, shares the same underlying object), so all four states stay consistent. `AppState`
   is removed only after every handler is migrated off it (T4f).
5. **Scope discipline.** Touch only the files named in the task. If a task reveals an
   unlisted coupling, stop and report it rather than expanding scope.
6. Commit after each task (`git add -A && git commit`) so a bad task is one `git revert`.

---

## Phase 0 — Extract `pump-constants` (do this FIRST)

### T0 — Extract a zero-dep `pump-constants` crate

- **Why:** `backend-local`'s sweep cost model imports the **tunable** `COMPUTE_UNIT_*`
  constants ([sweep/strategy.rs:13](backend/src/sweep/strategy.rs#L13)) that the live trader
  also uses — so they must stay **single-source** (duplicating them would let the backtest's
  fee model silently drift from live whenever you retune CU). Extracting them lets core/local
  read the constants **without depending on `pump-trader`** at all.
- **Action:**
  1. Add `pump-constants` to `[workspace] members` — a new crate with **no dependencies**.
  2. Move [pump-trader/src/constants.rs](pump-trader/src/constants.rs) verbatim →
     `pump-constants/src/lib.rs` (it's already pure `&str`/`u64`/`f64` literals).
  3. In `pump-trader`, replace the `constants` module with a re-export:
     `pub use pump_constants as constants;` (+ `pump-constants = { path = "../pump-constants" }`).
     Every existing `pump_trader::constants::X` keeps resolving — **no caller changes**.
- **Verify:** `cargo check -p pump-trader` + `cargo check --bin backend` (whole workspace still
  builds; `pump_trader::constants::*` paths unchanged).
- **Done when:** constants live in `pump-constants`; pump-trader re-exports them; nothing else
  changed. (External consumers of pump-trader are unaffected.)

---

## Phase 1 — In-place splits (single `backend` crate, identical runtime)

### T1 — Split `system/system.rs` handlers by concern

- **Goal:** separate live-mode handlers from settings/price handlers (prep for state split).
- **File:** [backend/src/api/handlers/system/system.rs](backend/src/api/handlers/system/system.rs)
- **Action:** within the file, group functions and add a comment banner: `// --- CORE
  (settings/price) ---` over `get_sol_price`, `get_settings`, `update_settings`; `// ---
  DEPLOY (live mode) ---` over `get_live_mode`, `set_live_mode`. No logic change yet.
- **Verify:** `cargo check --bin backend`.
- **Done when:** check is green; the two groups are clearly delimited.

### T2 — Extract a `list_tokens` core helper

- **Goal:** isolate the swing-dependent branch so deploy can later pass `None`.
- **File:** [backend/src/api/handlers/tokens/tokens.rs](backend/src/api/handlers/tokens/tokens.rs) (`list_tokens`, ~L361–494)
- **Action:** extract the filter→sort→page→ETag body (everything inside `web::block`) into a
  free fn `build_tokens_list(state, q, limit, offset, tracked_only, swing_stats:
  Option<&HashMap<String, ChainStats>>) -> (Vec<u8>, String)`. `list_tokens` keeps the swing
  block ([L414-433](backend/src/api/handlers/tokens/tokens.rs#L414-L433)) that computes
  `swing_stats`, then calls the helper. Keep `ChainStats`, `is_swing_sort_col`,
  `TokenQuery::sort_refs` where they are (they stay **core** later).
- **Verify:** `cargo check --bin backend` + `cargo test --bin backend`.
- **Done when:** `/api/tokens` responses are unchanged (helper is behavior-neutral).

### T3 — Refactor `tpsl1.rs` + `tpsl2.rs` into domain helper + edges

- **Goal:** make the three-way split explicit (clones — apply identically to both files).
- **Files:** [strategies/tpsl1.rs](backend/src/api/handlers/strategies/tpsl1.rs),
  [strategies/tpsl2.rs](backend/src/api/handlers/strategies/tpsl2.rs)
- **Action:**
  1. Extract the pure write+DTO core of `create`/`update`/`delete` into helper fns that
     touch only the rule repo + validation (no `tpsl_cache`, no RPC). Put them in a new
     `strategies/tpsl_rules_core.rs` (shared by both, generic over the repo if trivial; else
     one per strategy). These are the **core** pieces.
  2. In each `tpslN.rs`, the existing CRUD handlers become thin: call the core helper, then
     (deploy concern) `tpslN_cache.reload_rules`. Banner them `// --- DEPLOY ---`.
  3. Banner `simulate_*`, `cancel_simulate_*`, `paper_result_*`, `clear_paper_result_*` as
     `// --- LOCAL ---`. In `clear_paper_result_*`, note the live-cache `is_live` guard
     ([tpsl2.rs:1172](backend/src/api/handlers/strategies/tpsl2.rs#L1172)) — leave it for now;
     it's removed in the local crate (T14).
- **Verify:** `cargo check --bin backend` + `cargo test --bin backend`.
- **Done when:** all tpsl routes behave identically; domain logic lives in `tpsl_rules_core`.

### T4a — Define `CoreState` / `DeployState` / `LocalState`

- **Goal:** introduce the three structs without removing `AppState` yet.
- **File:** [backend/src/state/app_state.rs](backend/src/state/app_state.rs) (+ new
  `state/core_state.rs`, `state/deploy_state.rs`, `state/local_state.rs`).
- **Action:** create the three structs per **Design reference → State field map**.
  `DeployState`/`LocalState` each hold `core: Arc<CoreState>` plus their own fields. Move the
  repo-accessor methods onto `CoreState`; add `Deref`-style passthroughs or `state.core.…`
  call convention. Keep `AppState` intact for now.
- **Verify:** `cargo check --bin backend` (new structs compile, unused-warnings allowed only
  on the three new structs this one task).
- **Done when:** structs exist and compile.

### T4b — Construct + register all states in `main.rs`

- **Goal:** make `CoreState`/`DeployState`/`LocalState` available to handlers alongside `AppState`.
- **File:** [backend/src/main.rs](backend/src/main.rs) (HTTP server setup) + `api/mod.rs`.
- **Action:** build `Arc<CoreState>` once from the existing handles; build `DeployState` and
  `LocalState` from it + the existing Arcs. `app_data` all of them
  (`web::Data::new(...)`) next to the existing `AppState`.
- **Verify:** `cargo check --bin backend`; app boots and serves identically.
- **Done when:** all four states are injected; no route changes.

### T4c–T4e — Migrate handlers off `AppState`, by group

- **Goal:** switch each handler's extractor to the narrow state type.
- **Action (one sub-task per group; verify after each):**
  - **T4c (core handlers → `web::Data<Arc<CoreState>>`):** `stream_events`; `get_sol_price`,
    `get_settings`, `update_settings`; all of `system/wallets.rs`; `tokens/creation_stats.rs`;
    `tokens/batch.rs`; `tokens/tokens.rs` reads (`get_token`, `get_trades`, `list_creators`,
    `get_creator`) + the `build_tokens_list` helper.
  - **T4d (deploy handlers → `web::Data<DeployState>`):** `get_live_mode`, `set_live_mode`;
    `tokens/sync.rs`; `trading/solana.rs`; `trading/cashback.rs`; `tpsl1_positions.rs`,
    `tpsl2_positions.rs`; the **deploy** parts of `tpsl1.rs`/`tpsl2.rs` (CRUD+lifecycle+matched).
  - **T4e (local handlers → `web::Data<LocalState>`):** `strategies/grouped_sweep.rs`;
    `tokens/swing.rs`; `tokens/analysis.rs`; `system/jobs.rs`; the **local** parts of
    `tpsl1.rs`/`tpsl2.rs` (simulate/paper-result); `list_tokens` (keeps swing computation).
- **Verify (each):** `cargo check --bin backend` + `cargo test --bin backend`.
- **Done when:** no migrated handler references `AppState`.

### T4f — Delete `AppState`

- **Goal:** remove the fat struct now that nothing uses it.
- **File:** `state/app_state.rs`, `state/mod.rs`, `main.rs`.
- **Action:** delete `AppState` + its `new()` + the now-duplicated registration. Keep
  `SyncGate` (move to `deploy_state.rs`) and `SweepCorpusCache` (move to `local_state.rs`).
- **Verify:** `cargo check --bin backend` + `cargo test --bin backend`; app boots identically.
- **Done when:** `AppState` no longer exists; runtime behavior unchanged.

---

## Phase 2 — Extract `backend-core` (lib crate)

### T5 — Scaffold `backend-core` + workspace wiring

- **File:** root [Cargo.toml](Cargo.toml), new `backend-core/Cargo.toml`, `backend-core/src/lib.rs`.
- **Action:** add `backend-core` to `[workspace] members`. Create the lib crate with the
  **core** deps only (Design reference → Dep partition). Empty `lib.rs`. `backend` adds
  `backend-core = { path = "../backend-core" }`.
- **Verify:** `cargo check` (workspace builds; core is empty).

### T6 — Move leaf modules to core: `config`, `models`

- **Action:** move `backend/src/config/`, `backend/src/models/` → `backend-core/src/`; add
  `pub mod config; pub mod models;` to `lib.rs`; in `backend/src/main.rs` replace `mod config;
  mod models;` with `pub use backend_core::{config, models};` (Convention 3). Repoint
  `config/constants/protocol.rs`'s re-export from `pump_trader::constants` → `pump_constants`
  (so core depends on `pump-constants`, **not** `pump-trader`).
- **Verify:** `cargo check --bin backend`.

### T7 — Move `storage` to core

- **Action:** move `backend/src/storage/` (pools + all repos) → core; `pub mod storage;`;
  alias in `backend`. Repos reference only `models`/`config` (already in core) → clean.
- **Verify:** `cargo check --bin backend` + `cargo test --bin backend`.

### T8 — Move shared `services` + core `state` to core

- **Action:** move `services::{clients, helius_rpc, http, sol_price}` and
  `state::{token_cache, token_list_cache, token_metrics, core_state}` → core. Leave
  `services::{laserstream_replay, token_sync, wallet_reconcile, wallet_tokens}` and
  `state::{trade_signals, deploy_state, local_state, backtest_trade_cache,
  job_progress, sim_results, swing_results, swing_run_cache}` in `backend`. Alias moved ones.
- **Verify:** `cargo check --bin backend`.
- **Note:** if a moved service references a not-yet-moved module, that's a leaf-order
  violation — report it (shouldn't happen for this set).

### T9 — Move api framework + core handlers to core

- **Action:** move the `App`/route-builder framework, auth middleware, SSE render bridge, and
  the **core handlers** (T4c list) into `backend-core/src/api/`. Add a core
  `configure_core_routes(cfg)` that registers only the core-read routes. Keep deploy/local
  handlers + the full `configure()` in `backend` for now (they call the core one).
- **Verify:** `cargo check --bin backend` + `cargo test --bin backend`; routes unchanged.

---

## Phase 3 — Stand up `backend-deploy` (bin crate)

### T10 — Extract `ingest-laserstream` crate

- **Why:** LaserStream is one of potentially several ingest sources (WebSocket RPC is next).
  Each ingest source becomes its own workspace crate with a common `spawn(…)` fn signature;
  `backend-deploy` depends on whichever ingest crate(s) it needs at compile time. No shared
  trait until runtime switching is required.
- **Files:** root `Cargo.toml`; new `ingest-laserstream/{Cargo.toml, src/lib.rs}`;
  `backend/src/ingest_laserstream/*`; `backend/src/state/ingest_health.rs`.
- **Action:**
  1. Add `ingest-laserstream` to `[workspace] members`. Deps: `backend-core` +
     `tonic`/`prost`/`tokio-stream` + `tokio`/`tracing`/`anyhow`.
  2. Move `backend/src/ingest_laserstream/*` + `backend/src/state/ingest_health.rs` →
     `ingest-laserstream/src/`. (`IngestHeartbeat` and `spawn_watchdog` live here.)
  3. Expose a top-level `pub fn spawn(pool, heartbeat, live_rx, settings_rx, trade_tx, …) ->
     Vec<JoinHandle<()>>` wrapping the client→pipeline→db_writer chain + `spawn_watchdog`.
     `IngestHeartbeat` is created by the caller (`main.rs`) and passed in.
  4. In `backend`, replace the inline ingest wiring with `ingest_laserstream::spawn(…)`.
- **Verify:** `cargo check -p ingest-laserstream` + `cargo check --bin backend`.
- **Done when:** ingest code lives in the new crate; `backend` calls `spawn(…)` and compiles
  clean; no behavior change.
- ¹ Skip moving `maintenance.rs` if the TimescaleDB plan has already landed (it deletes it).

### T11 — Scaffold `backend-deploy` and move deploy modules

- **Files:** root Cargo.toml; new `backend-deploy/{Cargo.toml,src/main.rs}`.
- **Action:** add member; deploy deps: `backend-core` + `ingest-laserstream` + `pump-trader` +
  `bs58`/`base64`/`borsh`/`bincode`/`rand` + spl-* token crates. (`tonic`/`prost`/`tokio-stream`
  come transitively via `ingest-laserstream` — **not** direct deploy deps.) Move `strategies/*`,
  `trader/*`, deploy `services` + `state` modules, deploy handlers, `DeployState`, `SyncGate`
  from `backend` → `backend-deploy`. Depend on `backend-core` + `ingest-laserstream`.
- **Verify:** `cargo check -p backend-deploy` (will fail until T12 wires main — acceptable
  *only* for this scaffolding task; note what's missing).

### T12 — Port the deploy `main.rs` (`tokio::select!`) + probe

- **Action:** move the long-lived task wiring (call `ingest_laserstream::spawn(…)`,
  pool-refresh, `run_partition_maintenance`¹, `strategy_task`, wallet reconcile, SOL cache,
  HTTP via `configure_core_routes` + `configure_deploy_routes`) and the `probe` subcommand
  into `backend-deploy/src/main.rs`. Build `DeployState`; serve.
- **Verify:** `cargo check -p backend-deploy`; `cargo run -p backend-deploy` ingests, trades,
  and serves live routes exactly as the old `backend`.
- ¹ Remove `run_partition_maintenance` if the TimescaleDB plan has landed (Design reference → Timescale).

---

## Phase 4 — Stand up `backend-local` (bin crate)

### T13 — Scaffold `backend-local` and move local modules

- **Action:** add member; local deps (`rayon`/`arrow`/`parquet`/`memory-stats` +
  `pump-constants` for the sweep CU cost-model; **no `pump-trader`**). Repoint
  `sweep/strategy.rs`'s `use pump_trader::constants::{…}` → `pump_constants::{…}`. Move
  `sweep/*`, `analyzers/swing_analyzer`, local `state` modules
  (`backtest_trade_cache`, `job_progress`, `sim_results`, `swing_results`, `swing_run_cache`,
  `local_state`, `SweepCorpusCache`), local handlers from `backend` → `backend-local`. Depend
  on `backend-core`.
- **Verify:** `cargo check -p backend-local` (fails until T14 — scaffolding only).

### T14 — Port the local `main.rs` + local route wrappers

- **Action:** thin `main.rs`: build pools, SOL price poller, token-cache seed, build
  `LocalState`, serve `configure_core_routes` + new `configure_local_routes`. Add the
  **local `GET /tokens` wrapper** (computes `swing_stats` from `LocalState.swing_runs`, calls
  the core `build_tokens_list`). In `clear_paper_result_*`, **drop** the live-cache `is_live`
  guard (always "not live" locally). Add a deploy `GET /tokens` wrapper in T12's deploy routes
  that passes `swing_stats = None`.
- **Verify:** `cargo check -p backend-local`; `cargo run -p backend-local` boots with **no**
  trading keys / **no** HELIUS gRPC; rule create/edit + `POST /api/strategies/sweeps` +
  per-rule `simulate` work; live trading routes 404.

---

## Phase 5 — Finalize

### T15 — Delete the old `backend` crate

- **Action:** remove `backend/` and its workspace member entry once both bins own all code.
- **Verify:** `cargo check -p backend-deploy` + `cargo check -p backend-local` + both test
  suites + `cargo check -p backend-core` + `cargo check -p ingest-laserstream`.
  `cargo tree -p backend-deploy` shows **no** `arrow`/`parquet`/`rayon`;
  `cargo tree -p backend-local` shows **no** `tonic`/`prost` **and no `pump-trader`**.

### T16 — Frontend capability gating

- **Action:** add `GET /api/system/capabilities` → `{ has_live_trading, has_analysis }` to
  **both** route configs (deploy: live true/analysis false; local: inverse). Frontend
  ([frontend-react/src/App.tsx](frontend-react/src/App.tsx)) fetches once at boot; gate nav +
  lazy routes. Local keeps the rule editor.
- **Verify:** `cd frontend-react; npm run build`; nav gates correctly against each backend; no
  extra re-render on SOL/USD tick or live-trade stream.

### T17 — Docs

- **Action:** update **CLAUDE.md** (five workspace crates; per-crate commands; ship
  `backend-deploy` + `ingest-laserstream` to EC2 only), **@arch/architecture.md** (crate
  boundaries, the three states, two mains, ingest crate interface, strategy layering), and add
  **@plans/modes/crate-split.md** (design, `pump-constants` extraction, ingest crate pattern,
  Timescale coordination).
- **Verify:** docs match the shipped structure.

---

## Design reference (source of truth for tasks)

### Crate map

| Crate | Kind | Contents | Depended on by |
| --- | --- | --- | --- |
| `pump-constants` | lib | pure literals (`constants.rs`) | core, pump-trader, local |
| `backend-core` | lib | config, models, storage, core services/state, api framework, core handlers, strategy domain | deploy, local |
| `ingest-laserstream` | lib | LaserStream client→pipeline→db_writer, `IngestHeartbeat`, watchdog; exposes `pub fn spawn(…)` | deploy |
| `backend-deploy` | bin | strategies, trader, deploy services/state, deploy handlers, `DeployState` | — |
| `backend-local` | bin | sweep, swing analyzer, local state, local handlers, `LocalState` | — |

**Module → crate assignment:**

| → **core** | → **ingest-laserstream** | → **deploy** | → **local** |
| --- | --- | --- | --- |
| `config`, `models`, `storage` | `ingest_laserstream/*` (incl. `maintenance.rs`¹) | `strategies/*`, `trader/*` | `sweep/*` |
| `services::{clients,helius_rpc,http,sol_price}` | `state::ingest_health` | `services::{laserstream_replay,token_sync,wallet_reconcile,wallet_tokens}` | `analyzers::swing_analyzer` |
| `state::{token_cache,token_list_cache,token_metrics}` | | `state::{trade_signals}` | `state::{backtest_trade_cache,job_progress,sim_results,swing_results,swing_run_cache}` |
| api framework + auth + SSE bridge | | | |
| strategy domain (`tpsl_rules_core`, rule repos, validation/DTO) | | | |

### Core handlers (→ `CoreState`)

`stream_events`; `get_sol_price`/`get_settings`/`update_settings`; `system/wallets.rs` (profiles/tags); `tokens/creation_stats.rs`; `tokens/batch.rs`; `tokens/tokens.rs` reads + `build_tokens_list`.

### Deploy handlers (→ `DeployState`)

`get_live_mode`/`set_live_mode`; `tokens/sync.rs`; `trading/{solana,cashback}.rs`; `tpsl{1,2}_positions.rs`; deploy parts of `tpsl{1,2}.rs` (CRUD+cache reload, lifecycle, matched); deploy `GET /tokens` wrapper (`swing_stats = None`).

### Local handlers (→ `LocalState`)

`grouped_sweep.rs`; `tokens/swing.rs`; `tokens/analysis.rs`; `system/jobs.rs`; local parts of `tpsl{1,2}.rs` (simulate, paper-result — no live-cache guard); local `GET /tokens` wrapper (computes swing stats).

### State field map

- **CoreState:** `db`, `batch_db`, `helius_rpc_url`, `helius_laserstream_url`, `helius_api_key`, `pump_program_id`, `token_cache`, `token_list`, `sse_tx`, `sse_frame_tx`, `settings`, `sol_price` + all repo accessors.
- **DeployState:** `core` + `trader`, `tpsl1_cache`, `tpsl2_cache`, `pool_index`, `pools_changed`, `trade_signals`, `sync_gate`, `live_mode`.
- **LocalState:** `core` + `backtest_trade_cache`, `sweep_running`, `sweep_cancel`, `sim_cancels`, `backtest_sem`, `sweep_progress`, `sim_progress`, `sim_results`, `swing_cancels`, `swing_progress`, `swing_results`, `swing_runs`, `sweep_corpus_cache`.

### Dep partition

- **pump-constants:** zero deps (new leaf; the moved `constants.rs`). `pump-trader` re-exports it.
- **core:** tokio, actix-web/cors, sqlx, serde(+json), dotenvy, tracing(+subscriber), dashmap, async-trait, uuid, chrono, thiserror, anyhow, url, reqwest, solana-client/sdk + `pump-constants`. **No `pump-trader`.**
- **ingest-laserstream:** `backend-core` + `tonic`/`prost`/`tokio-stream` + tokio/tracing/anyhow. **No `pump-trader`.**
- **deploy:** `backend-core` + `ingest-laserstream` + `pump-trader` (pulls `pump-constants` transitively) + `bs58`/`base64`/`borsh`/`bincode`/`rand` + spl-* token crates. (`tonic`/`prost`/`tokio-stream` come transitively via `ingest-laserstream` — not direct deps.)
- **local:** `backend-core` + `pump-constants` (sweep CU cost-model) + `rayon`/`arrow`/`parquet`/`memory-stats`. **No `pump-trader`. No `ingest-laserstream`.**

### Strategy layering principle (current + future strategies)

Three layers per strategy: **Domain** (rule repo + validation + DTO) → **core**, written once;
**Runtime edge (deploy)** (live runner, `cache.reload_rules`, live-count enrichment, lifecycle,
positions) → cloned in deploy; **Runtime edge (local)** (simulate, paper-result) → cloned in
local. CRUD handler = core helper (write+DTO) + a ~5-line wrapper whose **only** difference is
the runtime side effect (deploy appends cache reload; local appends nothing). Clones carry
**only runtime wiring** — never business logic. Add-a-strategy: (1) core repo+helpers, (2) deploy
runtime clone, (3) local analysis clone, (4) register routes in each crate.

### Ingest crate pattern (current + future ingest sources)

Each ingest source is a workspace lib crate (`ingest-laserstream`, `ingest-websocket`, …) with:

- Deps: `backend-core` + its own transport crate(s).
- Public surface: `pub fn spawn(pool, heartbeat, live_rx, settings_rx, trade_tx, …) -> Vec<JoinHandle<()>>`.
- `IngestHeartbeat` created by the caller and passed in (so `main.rs` controls the watchdog lifecycle).

`backend-deploy` adds whichever ingest crate(s) it needs; unused ingest crates are never compiled.
No shared trait needed until runtime switching between ingest sources is required.

### TimescaleDB coordination ([timescaledb-plan.md](timescaledb-plan.md))

Orthogonal; adds no Rust deps. Overlap on 4 files: `maintenance.rs` (ingest-laserstream; Timescale
**deletes** it — ¹skip moving it if Timescale landed first), `main.rs` `select!` (don't edit in
both at once), `trade_repo.rs` (core; Timescale's `DO NOTHING` is a content edit only),
watchdog/`ingest_health.rs` (ingest-laserstream; Timescale may simplify). Synergy: per-box
retention (deploy 7-day+compression / local long corpora) is natural once boxes are separate;
OHLC continuous-aggregate candle endpoint → core. **Sequence: crate split first, Timescale second.**

### Verification matrix (final state)

1. `cargo check`/clippy clean on all five crates (`pump-constants`, `backend-core`, `ingest-laserstream`, `backend-deploy`, `backend-local`).
2. Deploy: ingest+trading+live as today; `POST /api/strategies/sweeps` 404; `cargo tree -p backend-deploy` shows no `arrow`/`parquet`/`rayon`.
3. Local: boots without keys/gRPC; rule authoring + sweeps + simulate work; live routes 404; `cargo tree -p backend-local` shows no `tonic`/`prost` **and no `pump-trader`**.
4. Binary-size + `cargo tree` diff confirm the dep partition.
5. Frontend builds; nav gates per backend; no extra re-render on tick/stream.
