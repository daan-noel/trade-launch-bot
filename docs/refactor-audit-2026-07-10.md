# Full-repo refactor plan — remaining work

Origin: seven parallel audits (2026-07-10) on `feat/restructure-hunter-forge`. **Re-verified
against the working tree on 2026-07-19** (branch `strategy-redesign`) by a four-agent sweep;
every `file:line` below is current as of that date. Completed and out-of-scope items are
collapsed into the ledger; do not redo them.

Constraints (unchanged): breaking changes OK, no behavior-preservation requirement.

## What changed since the last re-verify (2026-07-13 → 2026-07-19)

The **strategy redesign fully landed** (commits `40965acd`…`07592d19`, FE `c114693c`…`e077a361`,
Phase 7 `b274512e`…`07592d19`). This reshaped the ground the old plan stood on:

- **Legacy strategy triplication is gone.** Phase 7 *deleted* the named tpsl1/tpsl2/swing1
  decision stack (`live`, `lab`, and the FE swing feature). There is now **one generic
  fold** — `hunter-engine::reduce`, driven live by `hunter/live/src/strategies/engine/` and in
  analysis by `lab`'s replay/sweep. The audit's old "C1 — keep the intentional clones (OUT OF
  SCOPE)" ledger entry is **reversed and moot**; do not look for the clones.
- **A generic metric/fingerprint engine is the new extensibility surface** — metrics are pluggable
  modules, fingerprints are shared DB rows, rule params are JSONB `{entry, exit, tp/sl}` with
  per-operator grammar. See the new **Extensibility** section — this is where new-feature work now goes.
- **Structure drift:** `forge` orchestrator is its own crate (`forge/orchestrator/`, was
  `forge/launcher/src/orchestrator/`). The hunter ingest event model moved out of `hunter/core`
  into a **neutral** crate `shared/ingest/core` and grew a real `IngestVenue` trait — the venue seam
  is now half-real, not purely aspirational.
- **Perf work landed** on the sweep/sim path (AVX-512 exit scan behind a per-run toggle `a8766d54`;
  phase-split sim timers `6fdd5746`; fold hot-path fixes) and on billed RPC (`3911d070`, `01f5f782`,
  `63a455df`). Several Phase-5 perf items were explicitly *deferred* by those commits, not done —
  they remain below.

---

## Status ledger — DONE / OUT (do not redo)

- ✅ **Phase 0 — all correctness/safety/security bugs B1–B17** (bundle CAS, funding serialization,
  AMM poison idiom, loud rail parsing, forge-lab auth, postgres required-password + loopback, e500
  sanitization, constant-time token compare, etc.). Verified green when fixed.
- ✅ **C3 — dead code (C3-1…C3-6)** deleted (ingest `websocket`, orchestrator `dryrun.rs` + funding
  graph, legacy `Position` mutators, forge/lab `run_export` stub, `db-incremental-sync.ps1` stub;
  `transfer_with_seed`→`system_transfer`).
- ✅ **Strategy triplication removed** (was "C1 — OUT OF SCOPE / keep clones"). Phase 7 deleted the
  tpsl/swing stack; one generic engine now. Ledger reversal noted above.
- ✅ **`crates/forge-live` / `crates/forge-lab` doc refs** — gone (paths are `forge/live` / `forge/lab`).
- ✅ **`deploy/DOCKER.md` six→two compose files** — already documents the two merged compose files.
- ✅ **forge `/health` route** — present on forge-live (`forge/live/src/http.rs:62`) and forge-lab
  (`forge/lab/src/http.rs:19`). (hunter still lacks it — see Deploy below.)
- ⛔ **OUT OF SCOPE (agreed with user):**
  - **C2 — forge↔hunter infra dedup** (`shared/db`, `shared/units`, `shared/sol-price`,
    ingest-consumer extraction, http-auth bootstrap, `task_fault`). forge was copied from hunter and
    is still WIP; do not extract to shared crates yet.
  - **hunter deps stay direct** (not `.workspace = true`) — intentional split (`Cargo.toml:55-56`).
  - **solana `resolver = "1"` pin** — explained in `Cargo.toml:2-8`.

### Standing user follow-ups (not code — do before deploy)
- **B12:** the committed bcrypt `.htpasswd` hash is compromised (still in git history). Regenerate a
  UNIQUE per-gate `.htpasswd` (`htpasswd -B -C 12 -c …`) for hunter-live / hunter-lab / forge-live;
  consider scrubbing the hash from history.
- **B3:** compose REQUIRES `POSTGRES_PASSWORD` (no default) — sync `.env` before `compose up`.

---

## Phase 1 — Hygiene & docs (½–1 day, deletion-heavy)

- [ ] **Delete build-cache dir** (~21 GB reclaimable): `target-check/` at repo root. *(The stray
  `forge/target-check/` + `forge/frontend/target-check/` copies are already gone.)*
- [ ] **Delete `_monorepo-backup/`** at repo root (post-merge leftover; only `.bundle` git-history
  archives remain inside).
- [ ] **forge reqwest → rustls** (avoid native-tls/openssl in real-money bins). Both crates still on
  the default OpenSSL stack: `forge/live/Cargo.toml:37`
  (`reqwest = { version = "0.11", features = ["json"] }`) and `forge/launcher/Cargo.toml:38`
  (`… ["json", "multipart"] }`). Add `default-features = false` + `"rustls-tls"` to match hunter.
- [ ] **Doc sweep** — stale names after the hunter/forge restructure (only the *bare* stale paths;
  the dep-key/lib-name mapping notes in CLAUDE.md are intentional and stay):
  - `frontend-react` → `frontend`: `hunter/CLAUDE.md:25,55`, `hunter/docs/arch/frontend.md:1,8`,
    `hunter/docs/plans/deploy/api-auth-deploy-flow.md:41,42`, code comments
    `hunter/core/src/api/handlers/tokens/tokens.rs:498,1356`.
  - `pump-trader`/`ingest-laserstream` bare refs → `shared/executor/pumpfun` / `shared/ingest/pumpfun`:
    `forge/README.md` (23,24,33,46,47,63,64,72,84), `forge/docs/roadmap-plan.md`
    (37,57,76,87,103-105,128,181,188), `forge/docs/arch/architecture.md` (21,24,26,33,35,36,177,182,196).
  - `cargo run -p lab`/`-p live` → `-p hunter-lab` / `-p forge-live`: `hunter/docs/RUN-MODES.md:39-42`,
    `hunter/docs/arch/sweep.md:108`, `forge/docs/roadmap-plan.md:96`,
    `forge/frontend/src/features/launch/LaunchConsolePage.tsx:186` (UI text).
  - **Stale CLAUDE.md self-refs to fix while here:** hunter perf-budget says "read from
    `runtime_cache.rs`" but that file was deleted in the redesign — run state now lives in
    `hunter/core/src/models/strategy.rs` + the engine's in-memory caches. hunter CLAUDE.md's crate
    table still lists the retired tpsl/swing vocabulary in places.
- [ ] **Add a root `README.md`** tying the monorepo together (only `RUN.md` + root `CLAUDE.md` exist).
- [ ] **Façade `use`-name rename (dedicated cleanup commit)** — `pump_trader`→`executor_pumpfun`
  (86 refs/35 files) and `ingest_laserstream`→`ingest_pumpfun` (36 refs/19 files). Crates/packages
  already moved to `shared/executor/pumpfun` + `shared/ingest/pumpfun`; only the cosmetic `use`-name
  remains. Rename the lib targets (`shared/executor/pumpfun/src/lib.rs`,
  `shared/ingest/pumpfun/src/lib.rs`) + every call site, update the dep-key mapping notes in
  `hunter/CLAUDE.md:33-34` and `forge/CLAUDE.md:39-40`, in one isolated commit (no logic change).
- [x] **Prune stale planning docs** — done 2026-07-19: deleted `swing-detection-logic.md` (swing
  feature removed in Phase 7) and the landed design conversation `docs/strategy-redesign-{answer-1,
  new-plan}.md`. Surviving strategy invariants live in `hunter/docs/arch/strategies.md`.

---

## Extensibility (NEW — the generic engine is the primary extension surface)

The redesign made "add a metric" and "add a venue" the two axes new work runs along. Documenting and
smoothing these is now higher-leverage than any one-off cleanup.

### E1 — Metric-engine extension path (highest leverage; work is queued)
The engine reads metrics from pluggable modules; the stated design goal is "I'll add more metrics
later, so extensibility is very important." Make adding one a **single, documented seam** instead of a
scavenger hunt across layers.

- [ ] **Write `hunter/docs/plans/strategy/adding-a-metric.md`** — the end-to-end checklist a new metric
  touches: the metric module (`metric/` folder logic), the registry/grammar, the JSONB
  `{entry,exit}` param shape + per-operator parsing, the sweep column + summary aggregation, and the
  FE authoring grammar + metric pane. This is the SSOT that keeps static vs dynamic metrics
  (immediate-on-trade vs rule-parametrized like `window_size_sec`) from drifting.
- [ ] **Ship the first queued metric — volume/organic-flow split** (`m_flow_split` / `m_flow_window`,
  `fingerprints.metric_config` JSONB + discovery scoring). Design is settled (see the volume-flow
  design note) and was blocked on backend Ph5, which has now landed — this is the natural first
  exercise of the E1 seam and will surface any rough edges in it.
- [ ] **Registry as SSOT audit** — confirm metric keys, colors, and grammar are defined once in the
  Rust `REGISTRY` and flow to the FE (chips are already hue-driven from Rust). Add a no-DB guard test
  that the FE grammar and Rust registry can't silently diverge.

### E2 — Venue seam (was "Phase 4"; design-bearing, do before venue #2 code)
The seam is now **half-real**: catalog types + a neutral ingest crate + an `IngestVenue` trait landed,
but every write path still bakes pump.fun in. Acceptance test for completion: a mock "venue #2" (stub
trait impls) compiles both products with zero edits outside the new crate + dispatch tables. Don't
start until a second launchpad is actually on the roadmap — until then it only adds indirection.

- [ ] **E2-forge (smallest, highest-leverage):**
  - Operation constructors must take `venue: VenueId` — `create()` `forge/orchestrator/src/plan.rs:233`,
    `buy()` `:254`, `sell_with()` `:299`, `transfer_sol_as()` `:349` all hardcode
    `venue: VenueId::PumpFun` (`:237,268,313,362`); the `Operation.venue` field already exists (`:11`).
  - Give the launcher a venue trait for launch/dev-buy/bundle legs. `LaunchpadAdapter`
    (`forge/core/src/venue.rs:51`) is consulted **only by ingest** (`forge/live/src/ingest/*`,
    `forge/live/src/restore/backfill.rs:43`); the launcher never calls it.
  - One SSOT for wallet roles: `Role` (`forge/orchestrator/src/plan.rs:43`:
    Dev/Bundler/Volume/Treasury/External) vs `WalletRole` (`forge/core/src/models/status.rs:106`:
    Dev/Bundler/Treasury/Trading) are hand-mirrored across a string boundary (`plan.rs:37-40`) and
    have already diverged.
- [ ] **E2-executor-core de-pumping (`shared/executor/core`):**
  - Move the pump revert-code taxonomy `retry.rs:31-33,96-133` (`SwapRoute::{Curve,Amm}`, concrete
    Anchor codes `2006/6005/6024/…`, `RerouteMigrated`/`RefreshCoinCreator`) into the pumpfun venue crate.
  - Replace curve/amm-shaped `ComputeBudgetCfg` (`config.rs:18-40`: `curve_buy_cu`/`amm_cu`, consumed
    `engine.rs:221-242`) with venue-supplied path-keyed CU profiles.
  - ✅ Catalog types (`VariantSpec`/`VariantKind`/`Stage`/`Denom`) already live in the venue crate.
- [ ] **E2-hunter Venue enum:** the enum moved to `shared/ingest/core/src/event.rs:64` (good), but the
  binary curve/amm still pervades — `is_amm: bool` in the `TraderHook` contract
  (`hunter/core/src/ingest.rs:58`) and the `"curve"/"amm"` string mapping in
  `hunter/live/src/ingest/consumer.rs:513-516` + `token_sync.rs:1456`. Bundle pump economics into a
  per-venue `CurveEconomics` and **lift `CostModel::pumpfun_default` out of the strategy kernel**
  (`hunter/core/src/strategies/kernel.rs:108,117`) — the generic engine still defaults to it in ≥6
  call sites (`hunter/lab/src/sweep/generic/*`, `strategy_repo.rs:1316`).
- [ ] **E2-ingest event payloads:** the top-level `IngestEvent` is generic now
  (`shared/ingest/core/src/event.rs:9`) but payloads are still pump-shaped — `Venue{Curve,Amm}` (`:64`),
  bonding-curve `Reserves` (`:69-80`), `TokenCreated.bonding_curve`/`is_mayhem_mode` (`:91,97`),
  `BuyExactQuoteInV2` args (`:109-115`). Move venue-specific fields behind a typed payload extension.

---

## Performance (NEW framing — sweep hot path is now the lab's dominant cost)

### P1 — Sweep/sim hot path (in-flight; finish the measurement)
The AVX-512 exit scan (`a8766d54`) and phase-split sim timers (`6fdd5746`) landed. The remaining work
is **measurement + closing the real hot spot**: `resolve_exit` (not `prepare_token`) is the cost
center. One workstation lake run is needed to produce the measured A/B (scalar vs AVX-512) and the
`stage=sim_*` split numbers, then decide whether the AVX toggle defaults on. (See the sweep hot-path
and AVX notes.)

### P2 — Billed-RPC reduction, deferred pieces (from `01f5f782`)
R1+R2 landed and were smoke-verified; two pieces were explicitly deferred and remain:
- [ ] **`build_wallet_trader` per-leg init** — `forge/launcher/src/manage/execute.rs:352-375` still does
  `PumpFunTrader::new` + `.initialize()` (~8–12 RPC) for **every** sell leg; the volume bot pays this
  each cycle. Reuse global-account state across legs.
- [ ] **Restore-checkpoint + treasury/sweep Tier-3 batches** (`getMultipleAccounts` fan-in) — the
  remaining marginal, cache-gated read batches R2 left on the table.

### P3 — Forge Jito hot-path client reuse (one-off win, independent)
- [ ] Share one `reqwest::Client` for Jito submit — a fresh `reqwest::Client::new()` is built per call
  at `forge/launcher/src/bundle_execute.rs:515` (latency-critical) and `bundle_simulate.rs:188`,
  `wallet_sweep.rs:211,286`. (`jito_leader.rs:88` is now TTL-cached ⇒ largely moot; `sol_price.rs:31,44`
  sit behind a 60s poller ⇒ moot.)

### P4 — `TokenQuery::matches` over the 100K+ universe (one-off win, independent)
- [ ] `hunter/core/src/api/handlers/tokens/tokens.rs` — `matches` (`:1063`) re-`.parse::<f64>()`s
  numeric operands per row (`range_f64`/`opt_f64` `:1306-1332`, ~25×/row) and re-`.to_lowercase()`s
  needles per row (`text_match` `:1334-1338`, global search `:1229`). `mint_in` is a `Vec` with a
  linear-scan membership test (`:811,1084`) — make it a `HashSet`. Precompile operands into a typed
  filter plan once per query, not once per row.

---

## Quality (background, piecemeal)

- [ ] **Run-status enum** — replace stringly `status: String` on `StrategyRun`
  (`hunter/core/src/models/strategy.rs:56`, written `"Running".to_string()` at
  `hunter/live/src/strategies/engine/sinks.rs:304`) with a typed enum
  (`Running|Finished|Stopped|Cancelled`).
- [ ] **God-file splits (no logic change)** — current sizes: `hunter/live/src/services/token_sync.rs`
  **2089**, `hunter/lab/src/api/handlers/strategies/grouped_sweep.rs` **1951** (extract the ~500-line
  sweep runner to `sweep/job.rs`), `hunter/core/src/storage/repositories/strategy_repo.rs` **1945**,
  `hunter/core/src/api/handlers/tokens/tokens.rs` **1878**. (`forge/live/src/http.rs` dropped to 1231 ⇒
  no longer a target; `own_launch.rs` already split into small model + repo.)
- [ ] **Positional-arg constructors → builders/struct-literal defaults** (adjacent same-typed Options
  transpose silently on the trade path).

---

## Frontend (needs version alignment first)

- [ ] **Version drift + no workspace** — hunter React 19 / TS 6.0 / Vite 8 vs forge React 18 / TS 5.6 /
  Vite 6; no npm workspace root. Align versions, add a workspace, then extract a Tier-1 pure
  format/link/baseApi kit (`formatUsd`/`formatAge` already exist in both and have **diverged**
  signatures — `hunter/frontend/src/shared/utils/format.ts` takes seconds; `forge/frontend/src/shared/
  lib/format.ts` takes an ISO string) and add Rust→TS DTO codegen (ts-rs or schemars).
- [ ] **Lint baseline** — forge has no ESLint config or lint script at all; hunter's `eslint.config.js`
  is a module-boundary gate only (typescript/react-hooks rules off). Neither has Prettier. Add a real
  lint+format baseline (keep hunter's boundary gate).

---

## Deploy / ops (§G)

- [ ] **`/health` on the two hunter bins** — forge has it; hunter-live (`hunter/live/src/api/mod.rs`)
  and hunter-lab (`hunter/lab/src/api/mod.rs`) have none. Then wire container `healthcheck:` +
  `condition: service_healthy` for the app services (today only postgres is health-gated —
  `deploy/hunter.compose.yml:91-95`, `deploy/forge.compose.yml:93-97`).
- [ ] **Compose resource limits + log rotation** — neither compose file caps cpu/mem (a lab sweep can
  starve the live hot path) or bounds `logging:` (default unbounded json-file). Add both; cap lab-api
  hardest.
- [ ] **Mirror `CARGO_BUILD_JOBS=2` to forge Dockerfiles** — hunter has it
  (`deploy/hunter-{live,lab}/api.Dockerfile`), forge does not
  (`deploy/forge-{live,lab}/api.Dockerfile`) → OOM-during-build risk on the small box. Fix stale
  `EXPOSE` ports while there.

---

## Suggested order

1. **P1 hygiene** (target-check + `_monorepo-backup` delete, reqwest→rustls, doc sweep, prune stale
   plans, root README) — cheap, unblocks a clean tree + honest docs.
2. **E1 metric seam** (doc + ship the flow-split metric) — highest leverage; the engine is where new
   product value now lands.
3. **P2–P4 perf one-offs** (RPC deferrals, Jito client, TokenQuery) — independent, pick off
   opportunistically; **P1-sweep** just needs its one measurement run.
4. **Quality + Frontend + Deploy** — background/piecemeal.
5. **E2 venue seam** — deliberate design push; do **not** start until a second launchpad is real.
