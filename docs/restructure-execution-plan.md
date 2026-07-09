# Restructure + Redesign — Execution Plan (do-one-by-one)

Single ordered checklist that sequences the two source plans into runnable steps:

- **Part 1** — folder/rename migration → `monorepo-structure-plan.md`
- **Part 2** — write-side executor redesign → `executor-redesign-plan.md`
- **Part 3** — read-side ingest redesign → `ingest-redesign-plan.md` *(you did not have this file;
  every Part 3 item is **DERIVED** by me from the structure plan's hints and marked `⚠DERIVED` —
  reconcile against the real plan before executing)*

**Global rules (all parts):**
- `git mv` every move so history follows. Update `[workspace] members`, `{ workspace = true }` /
  path deps, `[[bin]]` names, deploy `--bin` flags, and every doc `@`-import **in the same commit**
  as each move.
- One logical group = one commit. Do not proceed past a **Gate** until it passes.
- Hunter (meme-trading) behavior stays **identical** throughout; forge (SLP) cuts over last.
- `shared/` never depends on anything product-specific. `lab` never appears in a `live` dep graph.
- After every move phase: `git status` to catch a leaked key / stray file.

**Ordering note:** Part 1 must land green before Part 2 or Part 3. Part 2 (executor / `pump-trader`
split) and Part 3 (ingest / `ingest-laserstream` split) are **independent stacks** — no crate
crosses between them — so they can run in either order or in parallel. This plan lists executor
first because it carries the overflow-bug fix and the forge cutover.

---

## PART 0 — Prep & safety (do first, once)

- [ ] **0.1 Branch.** Create a working branch off `master` for the restructure (e.g.
      `feat/restructure-hunter-forge`). Do not do this on `master`.
- [ ] **0.2 Inventory.** Record current `[workspace] members`, every path-dep, every `[[bin]]`,
      and each deploy `--bin`/Dockerfile so the rename map has a checklist to diff against.
- [ ] **0.3 Security cleanup — move OUT of the repo, never re-commit:**
  - [ ] `meme-trading/aws-ec2-key.pem` → `~/.ssh/`
  - [ ] `solana-launch-platform/keystore/`, `wallet-backups/` → offline / OS keychain
  - [ ] Confirm `.gitignore` still covers these paths post-move; `git status` clean of secrets.
- **Gate 0:** working branch exists; no secret files tracked; inventory captured.

---

## PART 1 — Monorepo structure migration (`monorepo-structure-plan.md`)

Pure folder/rename reshuffle — **no build/behavior redesign**. Sequenced **shared-first**, then
hunter, then forge, then deploy/docs. The two big `shared/` crate *splits* (`pump-trader`,
`ingest-laserstream`) are **owned by Parts 2 & 3**, not here — Part 1 only positions the
`shared/ingest/` parent and moves the websocket stub.

### Phase 1.1 — `shared/` skeleton (shared-first)
- [ ] Move `meme-trading/ingest-websocket` → `shared/ingest/websocket` (pkg `ingest-websocket`),
      creating `shared/ingest/` as its parent folder.
- [ ] Leave `shared/pump-trader` and `shared/ingest-laserstream` **in place** (their split into
      `shared/executor/{core,pumpfun}` and `shared/ingest/{core,pumpfun}` happens in Parts 2 & 3).
- [ ] Update `[workspace] members` + any path-deps for the websocket move.
- **Gate 1.1:** `cargo build` of the whole workspace green; `git status` clean.

### Phase 1.2 — `hunter/` (was `meme-trading`)
- [ ] Move the three crates together and repoint workspace + path-deps + `[[bin]]`:
  - `meme-trading/trading_core` → `hunter/core` (pkg `hunter-core`)
  - `meme-trading/live` → `hunter/live` (pkg `hunter-live`)
  - `meme-trading/lab` → `hunter/lab` (pkg `hunter-lab`)
- [ ] Move `meme-trading/frontend-react` → `hunter/frontend`.
- [ ] Move `meme-trading/{@arch,@plans,*-plan.md}` → `hunter/docs/{arch,plans}` (drop `@` prefixes;
      fix `CLAUDE.md` `@`-import paths).
- [ ] Move `meme-trading/scripts` → `hunter/scripts` (e.g. `db-incremental-sync.ps1`).
- **Gate 1.2:** `cargo build -p hunter-live -p hunter-lab` green; hunter frontend build green;
      `cargo run -p hunter-live` self-describing; `git status` clean.

### Phase 1.3 — `forge/` (was `solana-launch-platform`)
- [ ] Move the crates and repoint workspace + path-deps + `[[bin]]`:
  - `.../crates/platform-core` → `forge/core` (pkg `forge-core`)
  - `.../crates/slp-live` → `forge/live` (pkg `forge-live`)
  - `.../crates/slp-lab` → `forge/lab` (pkg `forge-lab`)
  - `.../crates/launcher` → `forge/launcher` (pkg `forge-launcher`)
- [ ] Fold `.../crates/lake` → `forge/lab/src/lake` **module** (mirror `hunter/lab/src/lake`); the
      `live`/`lab` bin boundary — not a crate — keeps DuckDB/arrow/parquet off EC2.
- [ ] Fold `.../crates/ingest-host` → `forge/live/src/ingest/` **host-adapter module** (mirror
      `hunter/live/src/ingest/`). `forge/live` will depend on the shared ingest stack directly
      (wired for real in Part 3).
- [ ] Move `solana-launch-platform/frontend-launch` → `forge/frontend`.
- [ ] Move `forge/idl/`, `forge/migrations/`, and `.../docs` → `forge/docs/{arch,plans}`.
- **Gate 1.3:** `cargo build -p forge-live -p forge-lab` green; `cargo tree -p forge-live` shows
      **no** `forge-lab`/lake heavy deps (DuckDB/arrow/parquet); forge frontend build green.

### Phase 1.4 — `deploy/` + root cleanup + docs
- [ ] Create one folder per shipped artifact with self-contained Dockerfile + `compose.yml`
      (+ `nginx.conf` where used): `deploy/{hunter-live, hunter-lab, forge-live, forge-lab}`.
      Move the `{meme,slp}` Dockerfiles/compose/`nginx/` in.
- [ ] Each `compose.yml` carries both `image:` (ECR ref) and `build.context: ../..` +
      `dockerfile:` so it builds on the workstation and pulls on EC2 unchanged. `*-lab` compose
      omits the registry (local only).
- [ ] Root cleanup — nothing at root but products (`hunter/`,`forge/`), `shared/`, `deploy/`, and
      repo meta (`docs/`, `scripts/`, `Cargo.toml`, `CLAUDE.md`). Sweep loose `*-plan.md`,
      `nginx/`, `run.bat`, `*.pem`, etc.
- [ ] Cross-product docs (these plans, migration notes) → root `docs/`.
- **Gate 1.4 (Part 1 exit):** full-workspace `cargo build` green; both frontends build;
      `docker compose build` (ctx=root) succeeds for each `deploy/*-live`; `git status` clean;
      root contains only the four allowed kinds of thing.

---

## PART 2 — Executor redesign (`executor-redesign-plan.md`)

Write side. Splits `shared/pump-trader` (8,600-line god-object) into an engine + venue, makes
variants first-class, cures the dev-buy overflow, adds the `orchestrator` brain + personas +
auditor, then cuts forge over. **Every phase ends with a zero-SOL verify gate.**

Target crates: **`executor-core`** (`shared/executor/core/`, third-party deps only) →
**`executor-pumpfun`** (`shared/executor/pumpfun/`, deps `executor-core`) →
**`orchestrator`** (`forge/orchestrator/`, deps both).

### Phase A — Split `pump-trader` → `executor-core` + `executor-pumpfun` (no behavior change)
- [ ] `git mv shared/pump-trader shared/executor/pumpfun` (pkg `executor-pumpfun`); create
      `shared/executor/core` (pkg `executor-core`). Add both to root `[workspace]` members.
- [ ] Move to **`executor-core`** (venue-agnostic): `tx.rs`, `nonce.rs`, `blockhash.rs`,
      `jito_tip.rs`, `swap_retry.rs` (→ `retry/`, keep `classify_swap_revert` + error-code consts
      as SSOT), `sim.rs`, `pool.rs`, `error.rs` (`TradeError`), the `Arc<dyn Signer>` seam, and new
      `venue.rs` (`Venue` trait + `VenueId`, static dispatch — no `Box<dyn>`).
- [ ] Keep in **`executor-pumpfun`**: `protocol.rs`, `buy/sell/amm/create/bundle_buy/query/
      reserves/consolidate/claim/types/config`. Implement `Venue` for pump; curve/AMM are **stages
      inside** pump. Pump protocol consts stay self-contained here.
- [ ] Decompose `PumpFunTrader`: engine (signer+rpc+tx/nonce/tip/blockhash/send/confirm) behind
      `executor-core`; pump caches + ix builders + pricing stay in `executor-pumpfun`, consuming
      the engine via the `Venue` seam.
- [ ] **Back-compat façade:** keep `pump_trader::*` / `::constants` / `::protocol` re-exporting from
      both crates so `hunter/live` + `launcher` compile **unchanged**. Repoint only the two direct
      consumers' Cargo deps.
- [ ] Preserve every SSOT guard test (`protocol_constants_ssot`, `classify_swap_revert`,
      `jito_tip`, `fan_out`, `reserves`, min-out math), no-DB.
- **Gate A:** `cargo build -p hunter-live -p forge-live` green via façade;
      `cargo test -p executor-core -p executor-pumpfun` green;
      `cargo tree -p hunter-lab`/`-p forge-lab` link **none** of `executor-core`/`executor-pumpfun`.

### Phase B — Variant catalog = SSOT + structural overflow fix
- [ ] Add the const `VariantSpec` catalog to `executor-pumpfun/catalog.rs` — one row per on-chain
      ix (`buy`, `buy_v2`, `buy_exact_sol_in`, `buy_exact_quote_in`, `buy_exact_quote_in_v2`,
      `sell`, `sell_v2`, AMM `sell`, `create`, `create_v2`, `transfer_with_seed`, SPL
      `transfer`/`transfer_checked`) carrying `disc / venue / denom / needs_wsol`. `valid_*(venue)`
      returns the legal subset. Kills the three inconsistent variant paths.
- [ ] **Fix overflow (R4):** every buy `min_out` — **including the dev-buy launch leg** — computes
      from **live reserves** via the one shared slippage calc in `executor-pumpfun/price.rs`. Delete
      the hardcoded `INITIAL_VIRTUAL_*_RESERVES` path at `create.rs:199-208`.
- [ ] **Variant ⊥ amount:** builders take a canonical `Amount` + live `QuoteCtx`; choosing a
      variant changes encoding only, never the economic spend.
- **Gate B:** catalog test asserts `valid_*(venue)` lists only legal variants; two buy variants
      over the same `Amount` produce the same on-chain economic effect; dev-buy `min_out` derives
      from live reserves (unit test over `price.rs`). Zero SOL.

### Phase C — `orchestrator` crate: `Operation`/`Plan` + providers + dry-run
- [ ] Create `forge/orchestrator/` (deps `executor-core` + `executor-pumpfun`).
- [ ] `plan.rs`: `OpKind` (`Create`,`Buy`,`Sell`,`TransferSol`,`TransferToken`) with
      `Role`/`Intent`/`VenueId`/canonical `Amount` as **orthogonal fields**
      (`DevBuy ≡ {Buy, role:Dev, intent:Snipe}`). `Plan { ops, funding, schedule }` — `funding`
      and `schedule` are **typed inputs** from the existing SLP unlinkability/jit-funding
      workstream, not built here.
- [ ] **Providers:** each `Operation` → ixs by calling `executor-pumpfun` builders
      (enum-dispatched, no `Box<dyn>`); reject any variant not in `valid_*(venue)`.
- [ ] **Dry-run:** build every tx from a `Plan`, stop before submit → serialized txs + summary;
      `min_out` shown as computed-at-send.
- [ ] **Generalize, don't replace** SLP's `manage::ActionPlan`/`PlanLeg`
      (`launcher/src/manage/model.rs`) + `bundles.legs` JSONB + `launch_templates.params
      .leg_structures` — unify the two divergent leg types under `Operation`.
- **Gate C:** a hand-authored `Plan` (1 create + dev-buy + 2 bundler buys) → valid signed txs in
      dry-run with zero SOL; `simulate_ixs` confirms they'd land.

### Phase D — Macros + personas (disguise, forge-only)
- [ ] `orchestrator/macros.rs`: `fund`, `bundle_launch`, `volume_make`, `exit`, `consolidate` —
      each expands to `Vec<Operation>` with correct `deps`. Consolidate emits a typed `TransferSol`
      op, **not** raw `system_instruction`.
- [ ] `disguise.rs` + `personas.rs` (forge-only): per-wallet **sticky persona** → draw a landing
      `Disguise` (variant from `valid_*`, CU/tip jitter within persona ranges). Every disguise
      guaranteed to land (CU ≥ real consumption; `cu_price` never dropped on a snipe). **Personas,
      not independent per-field jitter.**
- [ ] **Offline persona derivation** = a `forge/lab` clustering job over real pump traffic →
      persona templates → `personas.config` shipped to `forge/live`. Never on a hot path. Consume
      `funding` + `schedule` from the existing unlinkability workstream. Sells default un-bundled /
      direct-send, `tip: None`.
- [ ] Keep disguise **forge-only** — hunter/live keeps its lean snipe path, no per-tx sampling.
- **Gate D:** `volume_make`/`exit` produce Plans where a wallet's ops share its persona but differ
      in jittered CU/tip/variant; sells default `tip: None`.

### Phase E — Fingerprint auditor (mandatory)
- [ ] `orchestrator/audit.rs` — each rule a unit-testable fn over the `Plan`: star funding, equal
      amounts, same-slot cluster, direct own-graph token edge, constant CU/tip, synchronized
      `Bundler` exit, nonce-account reuse, `DurableNonce` on a non-latency op, clustered
      `close_token_ata: true` exit-tell, and **account-shape integrity** (fee recipient at index 17
      — hard reject). Output score + findings; **fail ⇒ reject unless `--allow-fingerprint`.**
- **Gate E:** rejects a deliberately-naive Plan (star fund, equal 0.02 legs, same-slot sells, a
      direct token transfer, a reused nonce, a mangled account list); passes a properly
      scheduled/persona'd one. Zero SOL, zero network.

### Phase F — Wire forge flows onto `Plan` + cut over
- [ ] Repoint the launcher (`execute_launch`, `bundle_execute`, `compose_bundle_legs` —
      `service.rs`, `bundle.rs`, `bundle_execute.rs`) to `bundle_launch` → `Plan` →
      `executor-core`, replacing the inline leg builder + free-text `template.variant` string match.
      Preserve the `funder-target == launch-gate` SSOT (`dev_launch_required_lamports`,
      `funding_plan.rs`).
- [ ] Repoint `manage` (sell/buy/consolidate — `manage/execute.rs`) + `ladder`/`volume` onto
      `volume_make`/`exit`/`consolidate`. **Retire the three raw-transfer paths**
      (`wallet_funding.rs:824`, `dust_sweep.rs`, `manage/execute.rs:341`) in favor of typed
      `TransferSol` ops (funding may keep its plain-transfer strategy behind a `TransferSol` op —
      but auditable).
- [ ] Persist the `Plan` + audit result alongside `bundles`/`token_positions` for preview/replay
      (generalize `bundles.legs` JSONB). Retire dead ad-hoc paths **after** parity.
- **Gate F (Part 2 exit):** a real (small) launch + volume + exit runs end-to-end through the
      `Plan` pipeline; audit logged; landing feed-confirmed on `forge/live` (no RPC poll); the
      on-chain "overflow" launch error class is gone.

**Part 2 invariants to preserve (bug if violated):** feed-confirmed sends / no hot-path RPC poll;
`min_out` late-bound (`slippage None ⇒ min_out=1 ⇒ skip reserve read`, `None ≠ Some(0)`); buy
recovery = sign→persist-sig→submit, never re-send a buy; one `classify_swap_revert` table (no
fork); account index 17 fee recipient never reshaped; durable-nonce **off by default** for forge,
fresh nonce account per wallet; `*_quote`/`*_base` BIGINT vocab (no `amount_lamports`); CU
cost-model constants single-sourced.

---

## PART 3 — Ingest redesign (`ingest-redesign-plan.md`) ⚠DERIVED

> **⚠ Every item below is INFERRED by me** from `monorepo-structure-plan.md` (§ shared/ingest,
> rules 4–5) and the `executor-redesign-plan.md` cross-references — **not** from an actual
> `ingest-redesign-plan.md`, which was not provided. Treat this Part as a scaffold: **reconcile
> phase-by-phase against the real plan before running any step.** Structure mirrors Part 2 because
> the two stacks are deliberately symmetric.

Read side. Splits `shared/ingest-laserstream` into an engine + venue decoder, makes the provider a
config axis, and collapses the per-product mapping into a `live/src/ingest/` module. **Totally
separate from Part 2** — no crate crosses between the two stacks.

Target crates: **`ingest-core`** (`shared/ingest/core/`, venue-agnostic: transport/pipeline/
reconnect + `Decoder` trait + neutral `IngestEvent`, provider-as-config) →
**`ingest-pumpfun`** (`shared/ingest/pumpfun/`, pump decoders + own protocol consts, deps
`ingest-core`) → **`ingest-websocket`** (already moved in Phase 1.1, stub second transport).

### Phase G — Split `ingest-laserstream` → `ingest-core` + `ingest-pumpfun` (no behavior change) ⚠DERIVED
- [ ] `git mv shared/ingest-laserstream shared/ingest/pumpfun` (pkg `ingest-pumpfun`); create
      `shared/ingest/core` (pkg `ingest-core`). Add both to root `[workspace]` members.
- [ ] Move to **`ingest-core`** (venue-agnostic): transport/Yellowstone-gRPC connection,
      pipeline/reconnect, the neutral `IngestEvent` type, and the new `Decoder` trait seam.
- [ ] Keep in **`ingest-pumpfun`**: pump decoders (`grpc.rs` `decode_curve_pb`, AMM decode),
      protocol consts (self-contained copy — the executor stack keeps its own; deliberate).
- [ ] **Back-compat façade** so `hunter/live` + `forge/live` compile unchanged this phase;
      repoint the direct consumers' Cargo deps only.
- **Gate G:** `cargo build -p hunter-live -p forge-live` green via façade;
      `cargo test -p ingest-core -p ingest-pumpfun` green; `cargo tree -p hunter-lab`/`-p forge-lab`
      link **none** of `ingest-core`/`ingest-pumpfun`.

### Phase H — `Decoder` trait + neutral `IngestEvent` + provider-as-config ⚠DERIVED
- [ ] Formalize the `Decoder` trait in `ingest-core`; `ingest-pumpfun` implements it. Emit only the
      neutral `IngestEvent` across the seam.
- [ ] **Provider = config** inside `ingest-core`: endpoint · pluggable `Auth` · capability flags —
      so Helius→Triton/Shyft (same Yellowstone gRPC) is a config change, no new crate.
- [ ] Carry forward the known decode fixes as guarded behavior: log-truncation dropped-legs
      (use inner-ix CPI events on "Log truncated"), and the still-exposed **AMM path** truncation
      (verify/close per the real plan).
- **Gate H:** decoder unit tests over fixture logs (curve + AMM, incl. truncated-log fallback)
      green; switching provider config compiles + connects with no crate change.

### Phase I — Collapse `ingest-host` mapping into per-product `live/src/ingest/` modules ⚠DERIVED
- [ ] Confirm `forge/live/src/ingest/` (moved in Phase 1.3) consumes `ingest-core` +
      `ingest-pumpfun` **directly**; the old `ingest-host` `consumer.rs`/`map.rs`/`pumpfun.rs`
      live here as a **module**, not a crate.
- [ ] Confirm `hunter/live/src/ingest/` follows the same bridge pattern. Both products now read
      identically: shared ingest stack + per-product `src/ingest/` bridge. `lab` links neither.
- **Gate I:** `cargo tree -p forge-live` / `-p hunter-live` show `ingest-core` + `ingest-pumpfun`
      but no `ingest-host` crate; `cargo tree -p *-lab` link neither.

### Phase J — WebSocket transport (only if actually switching) ⚠DERIVED
- [ ] Keep `shared/ingest/websocket` as the stub second-transport sibling to `core`. Flesh out
      **only** if you actually move off gRPC; otherwise leave as the documented stub.
- **Gate J:** stub compiles as a workspace member; no consumer forced to link it.

**Part 3 exit gate:** full-workspace `cargo build` green; `scripts/dep-partition-check.{sh,ps1}`
passes; both `*-live` bins ingest live feed with no behavior regression vs. pre-split.

---

## Final verification (whole restructure)
- [ ] `cargo build -p hunter-live -p forge-live -p hunter-lab -p forge-lab` all green.
- [ ] `cargo tree` confirms `hunter-lab`/`forge-lab` link **none** of
      `executor-core`/`executor-pumpfun`/`ingest-core`/`ingest-pumpfun`.
- [ ] `cargo test -p executor-core -p executor-pumpfun -p orchestrator` green (catalog,
      `classify_swap_revert`, `jito_tip`, `fan_out`, `reserves`, min-out math, dry-run, auditor).
- [ ] `scripts/dep-partition-check.{sh,ps1}` passes.
- [ ] `docker compose build` (ctx=root) succeeds for each `deploy/*-live`.
- [ ] Zero-SOL chain check: `simulate_ixs` / `*-dryrun` confirm a hand-authored Plan would land.
- [ ] One small real launch + volume + exit through the `Plan` pipeline on `forge/live`;
      feed-based landing confirmed; overflow class gone.
- [ ] `git status` clean; no secret files tracked; root has only products / `shared/` / `deploy/` /
      repo meta.
