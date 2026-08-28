# Full-repo refactor plan - remaining work

Open items from the seven-audit sweep of the monorepo. Every item here has been checked
against the tree and is genuinely open; anything found already done is removed rather
than left ticked. `file:line` anchors drift fast — re-verify before acting on one.

Constraints: breaking changes OK, no behavior-preservation requirement.

The completed/out-of-scope ledger and the audit's own status narrative are in
[history/refactor-audit-ledger.md](history/refactor-audit-ledger.md) - do not redo those items.

### Standing user follow-ups (not code — do before deploy)
- **B12:** the committed bcrypt `.htpasswd` hash is compromised (still in git history). Regenerate a
  UNIQUE per-gate `.htpasswd` (`htpasswd -B -C 12 -c …`) for hunter-live / hunter-lab / forge-live;
  consider scrubbing the hash from history.
- **B3:** compose REQUIRES `POSTGRES_PASSWORD` (no default) — sync `.env` before `compose up`.

---

## Hygiene & docs (½–1 day)

- [ ] **forge reqwest → rustls** (avoid native-tls/openssl in real-money bins). Both crates still on
  the default OpenSSL stack: `forge/live/Cargo.toml:37`
  (`reqwest = { version = "0.11", features = ["json"] }`) and `forge/launcher/Cargo.toml:38`
  (`… ["json", "multipart"] }`). Add `default-features = false` + `"rustls-tls"` to match hunter.
- [ ] **Add a root `README.md`** tying the monorepo together (only `RUN.md` + root `CLAUDE.md` exist).
- [ ] **Ingest deep-dives name symbols that no longer exist.** `scripts/check-docs.sh` only
  validates cited *paths*, so these passed after their file maps were repointed at
  `shared/ingest/{core,pumpfun}` + `live/src/ingest/`. Still stale in the prose:
  `IngestHeartbeat` (the type is `DbHeartbeat`, a single atomic — `backpressure-watchdog.md`
  still shows a two-field struct), and the constants `PIPELINE_SEND_TIMEOUT` /
  `RECONNECT_INTERVAL`, which have no definition anywhere
  (`reconnect-restart-flow.md` tabulates both with values). Verify each against
  `live/src/ingest/watchdog.rs` + `shared/ingest/core/src/supervisor.rs` before trusting
  a number in those three docs.
- [x] **Ingest façade `use`-name freed** — lib target and dep key are both
  `ingest-pumpfun` / `ingest_pumpfun`, and `ingest-laserstream` now names the gRPC
  wire crate it always claimed to. Landed with the read-stack split; see
  [../hunter/docs/arch/ingest.md](../hunter/docs/arch/ingest.md).
- [ ] **Executor façade `use`-name rename (dedicated cleanup commit)** —
  `pump_trader`→`executor_pumpfun` (38 files). The crate already moved to
  `shared/executor/pumpfun`; only the cosmetic `use`-name remains. Rename the lib target
  (`shared/executor/pumpfun/src/lib.rs`) + every call site and update the dep-key mapping
  notes in `hunter/CLAUDE.md` and `forge/CLAUDE.md`, in one isolated commit (no logic
  change). Until then `pump-trader` is the **current** Cargo dep key, not a stale path —
  do not "fix" it as such (the "false premise" entry in
  [../hunter/docs/roadmap/venue-quote-portability.md](../hunter/docs/roadmap/venue-quote-portability.md)).

---

## Extensibility (NEW — the generic engine is the primary extension surface)

The redesign made "add a metric" and "add a venue" the two axes new work runs along. Documenting and
smoothing these is now higher-leverage than any one-off cleanup.

### E1 — Metric-engine extension path (highest leverage)
The engine reads metrics from pluggable modules; the stated design goal is "I'll add more metrics
later, so extensibility is very important." Make adding one a **single, documented seam** instead of a
scavenger hunt across layers.

- [ ] **Write `hunter/docs/plans/strategy/adding-a-metric.md`** — the end-to-end checklist a new metric
  touches: the metric module (`metric/` folder logic), the registry/grammar, the JSONB
  `{entry,exit}` param shape + per-operator parsing, the sweep column + summary aggregation, and the
  FE authoring grammar + metric pane. This is the SSOT that keeps static vs dynamic metrics
  (immediate-on-trade vs rule-parametrized like `window_size_sec`) from drifting.
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
  `wallet_sweep.rs:216,337`. (`jito_leader.rs:88` sits behind a TTL cache and `sol_price.rs` behind a
  60s poller ⇒ both moot.)

### P4 — `TokenQuery::matches` over the 100K+ universe (one-off win, independent)
- [ ] `hunter/core/src/api/handlers/tokens/tokens.rs` — `matches` (`:1081`) re-`.parse::<f64>()`s
  numeric operands per row (`range_f64` `:1324` / `opt_f64` `:1342`, ~25×/row) and
  re-`.to_lowercase()`s needles per row (`text_match` `:1352`). `mint_in` is a `Vec` with a
  linear-scan membership test (`:829,1102`) — make it a `HashSet`. Precompile operands into a typed
  filter plan once per query, not once per row.

---

## Quality (background, piecemeal)

- [ ] **Run-status enum** — replace stringly `status: String` on `StrategyRun`
  (`hunter/core/src/models/strategy.rs:67`, compared and written as `"Running"` at
  `hunter/live/src/strategies/engine/sinks.rs:994,1006`) with a typed enum
  (`Running|Finished|Stopped|Cancelled`).
- [ ] **God-file splits (no logic change)** — current sizes:
  `hunter/core/src/storage/repositories/strategy_repo.rs` **3736**,
  `hunter/lab/src/api/handlers/strategies/grouped_sweep.rs` **2924** (extract the sweep runner to
  `sweep/job.rs`), `hunter/live/src/services/token_sync.rs` **2094**,
  `hunter/core/src/api/handlers/tokens/tokens.rs` **1896**. The top two roughly doubled since the
  audit, so this is growing faster than it is being paid down. `forge/live/src/http.rs` is not a
  target.
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

---

## Suggested order

1. **Hygiene & docs** (reqwest→rustls, doc sweep, root README) — cheap, unblocks a clean tree +
   honest docs.
2. **E1 metric seam** (write `adding-a-metric.md`, then the registry-SSOT guard test) — highest
   leverage; the engine is where new product value now lands.
3. **P2–P4 perf one-offs** (RPC deferrals, Jito client, TokenQuery) — independent, pick off
   opportunistically; **P1-sweep** just needs its one measurement run.
4. **Quality + Frontend + Deploy** — background/piecemeal.
5. **E2 venue seam** — deliberate design push; do **not** start until a second launchpad is real.
