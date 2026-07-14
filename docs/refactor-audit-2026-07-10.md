# Full-repo refactor plan — remaining work

Origin: seven parallel audits (2026-07-10) on `feat/restructure-hunter-forge`. This file has been
**pruned to only the not-yet-done work** and re-verified against the working tree on **2026-07-13**.
Completed and out-of-scope items are collapsed into the ledger below; do not redo them.

Constraints (unchanged): breaking changes OK, no behavior-preservation requirement.

---

## Status ledger — DONE / OUT (do not redo)

- ✅ **Phase 0 — all correctness/safety/security bugs B1–B17.** Bundle CAS, funding-pass
  serialization + TTL, AMM poison idiom, flush-retry, loud rail parsing, forge-lab auth, postgres
  required-password + loopback, e500 sanitization, strat-label match, constant-time token compare,
  PositionResponse collapse, etc. Verified green when fixed (`cargo check` across 10 crates; tests:
  orchestrator 24, http-auth 5, hunter-core position 12 + table_eval 6).
- ✅ **C3 — dead code (C3-1…C3-6).** Deleted `shared/ingest/websocket`, orchestrator `dryrun.rs` +
  funding graph, legacy `Position` mutators, forge/lab `run_export` stub, `db-incremental-sync.ps1`
  stub; renamed `transfer_with_seed`→`system_transfer`.
- ⛔ **OUT OF SCOPE (agreed with user):**
  - **C1 — strategy triplication** (tpsl1/tpsl2/swing1 clones, the old "Phase 2"). Intentional
    clones; keep them.
  - **C2 — forge↔hunter infra dedup** (the old "Phase 3": `shared/db`, `shared/units`,
    `shared/sol-price`, ingest-consumer extraction, http-auth bootstrap, `task_fault`). forge was
    copied from hunter and is still WIP; do not extract to shared crates yet.

### Standing user follow-ups (not code changes — must be done before deploy)
- **B12:** the committed bcrypt `.htpasswd` hash is compromised (still in git history). Regenerate a
  UNIQUE per-gate `.htpasswd` (`htpasswd -B -C 12 -c …`) for hunter-live / hunter-lab / forge-live;
  consider scrubbing the hash from history.
- **B3:** compose now REQUIRES `POSTGRES_PASSWORD` (no default) — sync `.env` before `compose up`.

---

## Remaining work (re-verified 2026-07-13, evidence = current `file:line`)

Three tranches remain: **P1 hygiene/docs** (cheap, do first), **P4 venue seam** (design-bearing,
deferred until venue #2 is real), **P5 perf/quality** (background/piecemeal).

### Phase 1 — Hygiene & docs (½–1 day, deletion-heavy)

All verified NOT-DONE unless noted.

- [ ] **Delete build-cache dirs** (~21 GB reclaimable, tracked-clean to remove): `target-check/` at repo
   root **plus** stray copies `forge/target-check/` and `forge/frontend/target-check/`.
- [ ] **Delete `_monorepo-backup/`** at repo root (post-merge leftover, dir dated Jul 9). *(2026-07-14: the
   stale SLP wallet-unlinkability plan copy inside it was removed; the `.bundle` git-history archives remain.)*
- [ ] **forge reqwest → rustls** (avoid native-tls/openssl in real-money bins): add
   `default-features = false, features = […,"rustls-tls"]` at
   `forge/live/Cargo.toml:35` and `forge/launcher/Cargo.toml:38` (hunter already does this).
- [ ] **Doc sweep** — stale names after the hunter/forge restructure:
   - `hunter/CLAUDE.md`: `frontend-react` → `frontend`; `pump-trader`/`ingest-laserstream` +
     stale paths `shared/pump-trader`/`shared/ingest-laserstream` → `shared/executor/pumpfun` /
     `shared/ingest/pumpfun`; `cargo run -p lab …` → `-p hunter-lab`.
   - `forge/CLAUDE.md`: `crates/forge-live`/`crates/forge-lab` (lines 110-111) → `forge/live`/
     `forge/lab`; `pump-trader`/`ingest-laserstream` refs (95,108,117,118,158,191).
   - `forge/docs/roadmap-plan.md:96` and `forge/frontend/src/features/launch/LaunchConsolePage.tsx:194`:
     `cargo run -p live` → `-p forge-live`.
   - Add a **root `README.md` (or `CLAUDE.md`)** tying the monorepo together (only `RUN.md` exists;
     hunter has no README).
5. **DEFERRED-BY-DECISION (leave as-is, documented in root `Cargo.toml`):**
   - hunter deps stay direct (not `.workspace = true`) — the split is intentional (`Cargo.toml:55-56`).
   - façade **call-site** rename (`pump_trader`→`executor_pumpfun`, ~112 refs/46 files;
     `ingest_laserstream`→`ingest_pumpfun`, ~36 refs/22 files) — crates/packages already moved to
     `shared/executor/pumpfun` + `shared/ingest/pumpfun`; only the cosmetic `use`-name rename
     remains and was deliberately deferred. Do only if a dedicated cleanup commit is wanted.
   - solana `resolver = "1"` pin story — comment in `Cargo.toml:2-8` already explains it.

### Phase 4 — Venue seam (design-bearing; do only before venue-#2 code)

The seam is mostly aspirational. Only catalog *types* have moved out; everything else still bakes
pump.fun in. Acceptance test for the whole phase: a mock "venue #2" (stub trait impls) compiles both
products with zero edits outside the new crate + dispatch tables.

- [ ] **4d forge** (smallest, highest-leverage):
  - Operation constructors must take `venue: VenueId` — `create()` `plan.rs:237`, `buy()`
    `plan.rs:268`, `sell_with()` `plan.rs:313`, `transfer_sol_as()` `plan.rs:362` all hardcode
    `VenueId::PumpFun`; the `Operation.venue` field already exists (`plan.rs:196`).
  - Give the launcher a venue trait for launch/dev-buy/bundle legs. Today `LaunchpadAdapter`
    (`forge/core/src/venue.rs:51`) is used **only by ingest** (`forge/live/src/ingest/*`); the
    launcher never consults it.
  - One SSOT for wallet roles: `Role` (`orchestrator/plan.rs:43`: Dev/Bundler/Volume/Treasury/
    External) vs `WalletRole` (`forge/core/src/models/status.rs:106`: Dev/Bundler/Treasury/Trading)
    are hand-mirrored across a string boundary (`plan.rs:67-69`) and have already diverged.
  - Re-export `VariantKind`/`VenueId` from executor-core, not pump_trader.
- [ ] **4a executor-core de-pumping:**
  - Move pump revert-code retry taxonomy `shared/executor/core/src/retry.rs:1-55`
    (`SwapRoute::{Curve,Amm}`, concrete Anchor codes) into the pumpfun venue crate.
  - Replace `curve_buy_cu`/`amm_cu`-shaped `ComputeBudgetCfg` (`core/src/config.rs:18-44`, consumed
    `engine.rs:205-226`) with venue-supplied path-keyed CU profiles.
  - ✅ Catalog types (`VariantSpec`/`VariantKind`/`Stage`/`Denom`) already live in the venue crate
    (`shared/executor/pumpfun/src/catalog.rs`).
- [ ] **4b hunter/core Venue enum:** kill stringly `"curve"/"amm"` + `is_amm: bool` (`ingest.rs:58-59`;
  usages in `models/trade.rs`, `state/token_cache.rs:960`, `storage/repositories/*`); bundle pump
  economics into a per-venue `CurveEconomics` (30-SOL baseline `constants/token_math.rs:15` +
  `tuning.rs:72`, supply, dead thresholds); lift `CostModel::pumpfun_default` out of the strategy
  kernel (`strategies/kernel.rs:109`); thread a `Trader` trait through `StrategyService`.
- [ ] **4c ingest event generalization:** `IngestEvent` is pump-flavored (`Venue{Curve,Amm}` bakes the
  curve→AMM lifecycle; pump `Reserves`; `TokenCreated` carries `bonding_curve`/`is_mayhem_mode`;
  SOL-only f64). Rename to a market-stage concept + typed venue payload extension.

### Phase 5 — Perf & quality polish (background, piecemeal)

- [ ] **Perf (targeted):**
  - Share one `reqwest::Client` for Jito bundle submit — fresh `reqwest::Client::new()` per submit
    on the latency-critical path `forge/launcher/src/bundle_execute.rs:494`. (Cold-path fresh clients
    at `wallet_sweep.rs:168,229`, `bundle_simulate.rs:188`, `forge/live/src/sol_price.rs:31,44` are
    lower priority.)
  - Reuse global-account state in `forge/launcher/src/manage/execute.rs:308` `build_wallet_trader` —
    currently `PumpFunTrader::new` + `.initialize()` (~8–12 RPC) **per leg**; volume bot pays this
    every cycle.
  - Precompile `TokenQuery::matches` (`hunter/core/…/tokens/tokens.rs:1093`): operands re-`.parse`d
    (`:1357,1364`) + re-`.to_lowercase()`d (`:1387,1259`) per row over a 100K+ universe; make
    `mint_in` a `HashSet` (currently `Vec`, linear scan `:1114`).
  - Batched JIT balances (`getMultipleAccounts`), `Arc<[String]>` instruction labels, reorder the
    `Token` clone before the `rules.is_empty()` check, `unique_wallets()` off the interner count,
    warm-corpus probe in the single-rule sim path.
- [ ] **Quality:**
  - Run-status enum: replace stringly `"Running"` (`hunter/core/…/runtime_cache.rs:762,902,921,942,958`).
  - God-file splits (no logic change) — current sizes: `token_sync.rs` 2069, `tokens/tokens.rs` 1937,
    `storage/repositories/strategy_repo.rs` 1915, `lab/…/grouped_sweep.rs` 1492 (extract the
    ~500-line sweep runner to `sweep/job.rs`), `forge/live/http.rs` 1203 (grew),
    `forge/core/storage/repositories/own_launch.rs` 1216 (7 repos in one file).
  - Positional-arg constructors → builders/struct-literal defaults (adjacent same-typed Options
    transpose silently).
- [ ] **Frontend (needs version alignment first):**
  - No ESLint/Prettier anywhere; add a baseline.
  - No npm workspace root; major drift — hunter React 19 / TS 6.0 / Vite 8 vs forge React 18 /
    TS 5.6 / Vite 6. Align, then extract a Tier-1 pure format/link/baseApi kit (~400 lines,
    `formatUsd`/`formatAge` already diverged) and Rust→TS DTO codegen (ts-rs or schemars).
- [ ] **Deploy/ops (§G):**
  - Add `/health` to all four bins (hunter has none) + compose healthchecks with
    `condition: service_healthy`.
  - Compose resource limits (cap lab-api cpu/mem so sweeps can't starve the live hot path), `logging:`
    rotation. Mirror `CARGO_BUILD_JOBS=2` OOM guard to forge Dockerfiles; fix stale `EXPOSE` ports;
    `deploy/DOCKER.md` still documents six compose files.

---

## Suggested order

P1 first (cheap, unblocks a clean tree + honest docs). P5 perf items (Jito client, trader reuse,
TokenQuery) are independent one-off wins — pick off opportunistically. P4 is a deliberate design
push; don't start it until a second venue/launchpad is actually on the roadmap, since without a
venue #2 it only adds indirection.
