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

## PART 0 — Prep & safety (do first, once) — ✅ DONE (2026-07-09)

- [x] **0.1 Branch.** Working branch `feat/restructure-hunter-forge` created off `master`.
- [x] **0.2 Inventory.** Members / path-deps / `[[bin]]` / Dockerfiles + the rename map recorded in
      `docs/restructure-inventory.md`.
- [x] **0.3 Security cleanup — move OUT of the repo, never re-commit:**
  - [x] `meme-trading/aws-ec2-key.pem` → `~/.ssh/` (was untracked/gitignored).
  - [x] `wallet-backups/` → `~/restructure-secrets-offline/`. Its `managed_wallets.json`
        exports were **tracked** (only `wallet-backups/**/keystore/` was ignored) → `git rm --cached`
        + `.gitignore` now covers `wallet-backups/`. No private-key material was in the JSON.
  - [~] `keystore/` → **LEFT IN PLACE** (deliberate deviation): already gitignored and read by the
        app at runtime, so moving it offline would break local signing. Moved with its product to
        `forge/keystore/` in 1.4 (still gitignored/untracked).
  - [x] `.gitignore` covers all these paths post-move; `git status` clean of secrets.
- **Gate 0:** ✅ branch exists; no secret files tracked; inventory captured.

> **Convention adopted for all of Part 1** (keeps it a true no-behavior-change reshuffle):
> rename `[package] name` ONLY; KEEP every `[lib]`/`[[bin]]` target name (lib `trading_core`/
> `platform_core`/`launcher`; bins `live`/`lab`/`slp-live`/`slp-lab`) so the ~120 source
> `use trading_core::`/`use platform_core::` refs and all Docker `--bin` flags need ZERO edits.
> Dependents keep the old dependency KEY + `package = "<new>"` + updated `path`.

---

## PART 1 — Monorepo structure migration (`monorepo-structure-plan.md`)

Pure folder/rename reshuffle — **no build/behavior redesign**. Sequenced **shared-first**, then
hunter, then forge, then deploy/docs. The two big `shared/` crate *splits* (`pump-trader`,
`ingest-laserstream`) are **owned by Parts 2 & 3**, not here — Part 1 only positions the
`shared/ingest/` parent and moves the websocket stub.

### Phase 1.1 — `shared/` skeleton (shared-first) — ✅ DONE
- [x] Moved `meme-trading/ingest-websocket` → `shared/ingest/websocket`, creating `shared/ingest/`.
- [x] Left `shared/pump-trader` and `shared/ingest-laserstream` **in place** (splits owned by Parts 2 & 3).
- [x] Updated `[workspace] members` + the websocket crate's `trading_core` path-dep.
- **Gate 1.1:** ✅ workspace metadata resolves; moved crate builds; `git status` clean.

### Phase 1.2 — `hunter/` (was `meme-trading`) — ✅ DONE
- [x] Moved + repointed the three crates (pkg-name-only rename per the convention above):
  - `meme-trading/trading_core` → `hunter/core` (pkg `hunter-core`, lib target `trading_core`)
  - `meme-trading/live` → `hunter/live` (pkg `hunter-live`, bin `live`)
  - `meme-trading/lab` → `hunter/lab` (pkg `hunter-lab`, bin `lab`)
- [x] Moved `meme-trading/frontend-react` → `hunter/frontend`.
- [x] Moved `@arch`/`@plans`/`*-plan.md` → `hunter/docs/{arch,plans}`; `CLAUDE.md` `@arch/`/`@plans/`
      imports rewritten to `docs/arch/`/`docs/plans/` (the `@live`/`@lab` Vite aliases left untouched).
- [x] Moved `meme-trading/scripts` → `hunter/scripts`.
- **Gate 1.2:** ✅ `cargo build -p hunter-live -p hunter-lab` green; hunter frontend `npm run build`
      green; `git status` clean. (`cargo run -p hunter-live` not booted — needs live DB/Helius env;
      the successful `-p hunter-live` build proves the target resolves.)

### Phase 1.3 — `forge/` (was `solana-launch-platform`) — ✅ DONE
- [x] Moved + repointed the crates (pkg-name-only rename; lib/bin targets kept):
  - `platform-core` → `forge/core` (pkg `forge-core`, lib `platform_core`)
  - `slp-live` → `forge/live` (pkg `forge-live`, bin `slp-live`)
  - `slp-lab` → `forge/lab` (pkg `forge-lab`, bin `slp-lab`)
  - `launcher` → `forge/launcher` (pkg `forge-launcher`, lib `launcher`)
- [x] Folded `lake` → `forge/lab/src/lake` **module** (`mod lake;`); dropped the crate + member
      (its deps were already a subset of `forge-lab`).
- [x] Folded `ingest-host` → `forge/live/src/ingest/` **module** (`mod ingest;`); absorbed its deps
      (`ingest-laserstream`, `chrono`, `bs58`), rewrote internal `crate::`→`super::`, and preserved
      its DB-gated roundtrip test as `#[cfg(test)] mod roundtrip_test`. Fixed the compile-time
      `sqlx::migrate!("../../migrations")` → `("../migrations")` for the new `forge/core` depth.
- [x] Moved `frontend-launch` → `forge/frontend`.
- [x] Moved `migrations/` → `forge/migrations`, `docs/` → `forge/docs`. (`idl/` was moved to
      `forge/idl` here, then **removed** in Phase 2 as a byte-identical duplicate of
      `shared/executor/pumpfun/idl/`, which owns the fetch scripts + hand-written decoders and is
      the sole canonical copy — no code loads the IDL JSONs at build/runtime.)
- **Gate 1.3:** ✅ `cargo build -p forge-live -p forge-lab` green; `cargo tree` shows `forge-live`
      free of DuckDB/arrow/parquet and `forge-lab` free of pump-trader/ingest-laserstream; forge
      frontend `vite build` green. ⚠ the frontend `tsc` step fails on a **pre-existing** TS6
      `ignoreDeprecations: "6.0"` config quirk (not caused by this move; tsconfig is byte-identical).

### Phase 1.4 — `deploy/` + root cleanup + docs — ✅ DONE
- [x] Created `deploy/{hunter-live, hunter-lab, forge-live, forge-lab}`, each self-contained:
      hunter-live has backend `Dockerfile` + `web.Dockerfile` + `nginx/` + `compose.yml`; the others
      have `Dockerfile` + `compose.yml`. Moved the Dockerfiles/nginx in and repointed the frontend
      `web.Dockerfile` COPY paths (`hunter/frontend`, `deploy/hunter-live/nginx`); fixed the forge
      Dockerfile `-p slp-live` → `-p forge-live` package selector. Authored a new `forge-lab` Dockerfile.
- [x] Each `*-live` `compose.yml` carries both `image:` (ECR ref) and `build.context: ../..` +
      `dockerfile:`; `*-lab` composes omit the registry (local only).
- [x] Root cleanup — `meme-trading/` and `solana-launch-platform/` are **gone**. Old
      `docker-compose*.yml` deleted; `run.bat` → `hunter/scripts`; `dep-partition-check.{sh,ps1}` +
      `db-incremental-sync.ps1` + `seed-dev-launch.sql` → `forge/scripts` (partition check updated
      to `forge-live`/`forge-lab`); per-product `.env.example`/`.gitignore`/`.gitattributes` →
      product folders. Root holds only `hunter/ forge/ shared/ deploy/ docs/` + repo meta.
- [x] Cross-product docs (`executor-redesign-plan.md`, `monorepo-structure-plan.md`,
      `restructure-execution-plan.md`) → `docs/`.
- **Gate 1.4 (Part 1 exit):** ✅ full-workspace `cargo build` green; both frontends build; all four
      `docker compose config` valid **and** `docker compose build` produced both `deploy/*-live`
      images (`forge/forge-live`, `hunter/hunter-live` + `hunter/hunter-web`, exit 0); `git status`
      clean; root contains only the allowed kinds.

> **Part 1 landed on branch `feat/restructure-hunter-forge` (5 commits, one per group); not yet
> merged to `master`. Parts 2 & 3 not started.**

---

## PART 2 — Executor redesign (`executor-redesign-plan.md`)

Write side. Splits `shared/pump-trader` (8,600-line god-object) into an engine + venue, makes
variants first-class, cures the dev-buy overflow, adds the `orchestrator` brain + personas +
auditor, then cuts forge over. **Every phase ends with a zero-SOL verify gate.**

Target crates: **`executor-core`** (`shared/executor/core/`, third-party deps only) →
**`executor-pumpfun`** (`shared/executor/pumpfun/`, deps `executor-core`) →
**`orchestrator`** (`forge/orchestrator/`, deps both).

### Phase A — Split `pump-trader` → `executor-core` + `executor-pumpfun` (no behavior change) — ✅ DONE (2026-07-09)
- [x] `git mv shared/pump-trader shared/executor/pumpfun` (pkg `executor-pumpfun`, **lib stays
      `pump_trader`**); created `shared/executor/core` (pkg `executor-core`, lib `executor_core`).
      Both added to root `[workspace]` members; workspace dep key `pump-trader` kept via
      `package = "executor-pumpfun"`.
- [x] Moved to **`executor-core`** (venue-agnostic): `tx.rs`→`send.rs`, `nonce.rs`, `blockhash.rs`,
      `jito_tip.rs`, `swap_retry.rs`→`retry.rs` (SSOT `classify_swap_revert` + error-code consts),
      `error.rs` (`TradeError`+`bail!` via `#[macro_export]`), `config.rs` (`TraderConfig`), the
      `sim.rs` **Layer-0** primitive (`simulate_ixs`+`SimOutcome`/`AccountDelta`), the `Arc<dyn
      Signer>` seam, and new `venue.rs` (`Venue` trait + `VenueId`, static dispatch).
- [x] **Deviation (correct):** `pool.rs` + the 4 `simulate_*` Layer-1 helpers **stay venue-side**
      (they read venue caches / depend on `TokenProgram`) — the plan mis-listed them for core.
- [x] Decomposed `PumpFunTrader` across an **`Engine` struct** (engine owns signer+rpc+http+
      nonce/tip/blockhash caches+cu-ixs+SOL-ledger; `Engine::initialize` does the engine-half of
      init). `PumpFunTrader { engine, …venue caches }` **`Deref`s to `Engine`** so every
      `self.send_transaction/acquire_nonce/jito_tip_ix/simulate_ixs` call resolves unchanged;
      duplicated `config`/`rpc`/`http` handles (same allocation) keep the ~60 field reads unedited.
      Implements `Venue for PumpFunTrader`.
- [x] **Back-compat façade:** `pump_trader::*` / `::constants` / `::protocol` / `::config` /
      `::error` re-export from both crates so `hunter/live` + `launcher` compile **unchanged**
      (0 source edits). Repointed only the two direct consumers' Cargo deps (+ workspace dep).
- [x] SSOT guard tests preserved (`protocol_constants_ssot` passes; `classify_swap_revert`,
      `jito_tip`, `fan_out`, min-out math all green).
- **Gate A:** ✅ `cargo check -p hunter-live -p forge-live` green via façade; `cargo test -p
      executor-core -p executor-pumpfun` green (18+27); `cargo tree -p hunter-lab`/`-p forge-lab`
      link **none** of `executor-core`/`executor-pumpfun`; full `cargo check --workspace` = 0 errors.

### Phase B — Variant catalog = SSOT + structural overflow fix — ✅ DONE (2026-07-09)
- [x] Added the const `VariantSpec` catalog to `executor-pumpfun/catalog.rs` — rows for `buy`,
      `buy_exact_sol_in`, `buy_v2`, `buy_exact_quote_in_v2`, `sell`, `amm_buy`, `amm_sell`,
      `create`, `create_v2`, `transfer_with_seed`, `spl_transfer`, `spl_transfer_checked` carrying
      `venue / stage(Curve|Amm|NA) / kind / denom / disc / needs_wsol / v2_accounts`.
      `valid_variants(venue)` / `valid_of_kind` / `is_valid` / `spec` return the legal subset so an
      off-catalog `(venue, name)` is unrepresentable. (`buy_exact_quote_in` non-v2 + `sell_v2` are
      not real pump ixs — the `BuyExactQuoteIn` variant aliases to sol-in/v2-quote — so they're
      absent by design; `is_valid` rejects them.)
- [x] **Discriminator SSOT:** promoted the scattered buy/sell discriminators (were inline in
      `buy.rs`/`sell.rs` + local consts in `bundle_buy.rs`/`amm.rs`) to `protocol.rs`
      (`BUY_DISC`/`BUY_EXACT_SOL_IN_DISC`/`BUY_V2_DISC`/`BUY_EXACT_QUOTE_IN_V2_DISC`/`SELL_DISC`);
      every builder + the catalog now reference them. `catalog::tests` guards catalog≡protocol.
      Curve+AMM `buy`/`sell` share one Anchor disc (program disambiguates) — so `VenueId`+`stage`
      are load-bearing axes. Identical bytes ⇒ zero behavior change (all 27 prior tests still pass).
- [x] **Slippage SSOT (`price.rs`):** the two `compute_curve_{buy,sell}_min_out` copies (were in
      `buy.rs`/`sell.rs`) collapsed into ONE `crate::price::curve_{buy,sell}_min_out`; the old names
      are zero-churn re-export aliases so every call site (sim/create/bundle) + test is unchanged.
- [x] **Dev-buy min_out:** `create.rs` no longer hand-inlines `INITIAL_VIRTUAL_*_RESERVES` — it
      derives the floor through the SAME `price::curve_buy_min_out` seeded by a named
      `price::fresh_curve_reserves()` (documented: the curve is created in the same tx, so the
      protocol-constant initial reserves ARE its live reserves for that first buy — a curve that
      already exists reads live reserves as before). Removes the drift-prone inline tuple; behavior
      unchanged (still `slippage=None ⇒ min_out=1` for the launch leg).
- [x] **Variant ⊥ amount** expressed in the catalog (`denom` axis: two buy encodings, same `Buy`
      kind, different `denom`) + a `price.rs` unit test showing the dev-buy floor rises with tighter
      slippage over the fresh-curve reserves. The full "builders take a canonical `Amount`+`QuoteCtx`"
      generalization is deferred to Phase C's `Plan` model (which owns `Amount`).
- **Gate B:** ✅ `cargo test -p executor-pumpfun` green (35, +8 new catalog/price); catalog test
      asserts `valid_variants` is pump-only + rejects off-catalog names; dev-buy floor unit-tested
      over `price.rs`; `cargo check --workspace` = 0 errors.
- **⚠ Overflow caveat (honest):** the curve buy is ALREADY `buy_exact_sol_in` (fixed SOL in,
      `min_out` a floor) — that leg cannot overflow the program's cost calc, so the plan's on-chain
      `MathOverflow (6025)` is NOT caused by this file. Its likely origin is the **launcher's**
      divergent variant/amount paths (free-text `template.variant`, the `BuyVariant` round-trip,
      the equal-`quote_per_leg` bundler) — cured in **Phase F** when those are wired onto the
      catalog + `Plan`. Phase B delivers the SSOT + structural prerequisites; a live mainnet repro
      is still needed to confirm the class is gone (Gate F).

### Phase C — `orchestrator` crate: `Operation`/`Plan` + providers + dry-run — ✅ DONE (2026-07-09)
- [x] Created `forge/orchestrator/` (pkg `forge-orchestrator`, lib `orchestrator`; deps
      `executor-core` + `pump-trader`=`executor-pumpfun`); added to root `[workspace]` members.
      LIVE/forge-only — neither lab links it (`cargo tree -p forge-lab`/`-p hunter-lab` = none).
- [x] `plan.rs`: **`OpKind` reuses the catalog's `VariantKind`** (`Create`/`Buy`/`Sell`/
      `TransferSol`/`TransferToken`) — SSOT, can't drift from the on-chain ix table. `Role`
      (`dev`/`bundler`/`volume`/`treasury`/`external`), `Intent` (`snipe`/`accumulate`/
      `make_volume`/`exit`/`fund`/`consolidate`/`create`), `VenueId`, and canonical `Amount`
      (`ExactQuote`/`ExactBase`/`ExactBaseIn`/`Sol`/`Token`/`None`) are **orthogonal fields**
      (`DevBuy ≡ {Buy, role:Dev, intent:Snipe}`). `min_out` is **NOT** stored — only
      `slippage_bps` (late-bound). `Plan { mint_address, ops, funding, schedule }`; `Funding`/
      `Schedule` are **typed `Default` inputs** for the SLP unlinkability workstream, not built
      here. Addresses are base58 `String`s so a `Plan` serializes/persists (generalizes
      `bundles.legs` JSONB). Constructors: `Operation::{create,buy,sell,transfer_sol}`.
- [x] **Providers (`provider.rs`):** `prepare(&Plan) → PreparedPlan` — static dispatch over
      `VenueId`/`OpKind` (no `Box<dyn>`); rejects (fail-fast, crate-owned `PlanError`) an
      off-catalog variant (`is_valid`), a kind≠spec.kind, an `Amount` the variant's `denom` can't
      encode (variant ⊥ amount), an unparseable address, a missing target, a dangling dep, or a
      duplicate id. `MinOut` policy = `Late{bps}` / `Unprotected` / `NotApplicable` (never a frozen
      number). **Deviation (honest):** providers do NOT emit `Instruction`s this phase — the pump ix
      builders are methods on an *initialized* `PumpFunTrader` (read the on-chain `Global` account),
      so real tx assembly + `simulate_ixs` land at the **Phase F** live cutover; `PreparedOp` carries
      exactly what that build step consumes (resolved `VariantSpec`, parsed accounts, `Amount`,
      min_out policy).
- [x] **Dry-run (`dryrun.rs`):** `dry_run(&Plan) → DryRunReport` — serializable per-op summary
      (kind/variant/role/intent/wallet/target/amount/`min_out` label/deps) + roll-ups; `min_out`
      shown as "computed-at-send @ Nbps". Zero SOL, zero network.
- [x] **Generalize seam:** `Operation` + its constructors unify the two divergent leg types; the
      typed `transfer_sol` op is the shape that replaces the three raw `system_instruction::transfer`
      bypasses. The actual `manage::ActionPlan`/`PlanLeg` + `launch_templates.leg_structures` →
      `Plan` adapter lands in **Phase F** (it lives launcher-side; orchestrator can't dep the
      launcher without a cycle).
- **Gate C:** ✅ zero-SOL portion — `cargo test -p forge-orchestrator` green (6): the hand-authored
      launch `Plan` (1 `create_v2` + dev-buy + 2 bundler `buy_exact_sol_in`, all deps→create)
      dry-runs clean, serializes, roll-ups correct; off-catalog / kind-mismatch / denom-mismatch /
      dangling-dep all rejected; typed consolidate transfer builds. Executor guard tests still green
      (18 + 34) after adding `serde` derives to `VenueId`/`VariantKind`; `cargo check -p hunter-live
      -p forge-live` green. ⚠ the **signed-txs + `simulate_ixs` would-land** check needs a live
      `PumpFunTrader` (on-chain `Global` read) → deferred to Phase F, same as prior phases' live-only
      gate portions.

### Phase D — Macros + personas (disguise, forge-only) — ✅ DONE (2026-07-09)
- [x] `orchestrator/macros.rs`: `fund`, `bundle_launch`, `volume_make`, `exit`, `consolidate` —
      each expands to `Vec<Operation>` with correct `deps`, drawing ids from a shared `plan::IdSeq`
      so several macros compose into ONE plan without id collisions (bundler buys `deps` the create;
      consolidate `deps` the exits). Consolidate + fund emit a typed `TransferSol` op (via new
      `Operation::transfer_sol_as`, which records the acting wallet's role), **not** raw
      `system_instruction`. Added `Operation::sell_with` (explicit intent) so a `MakeVolume` sell and
      an `Exit` sell are the same mechanism, different `intent`.
- [x] `personas.rs` + `disguise.rs` + a deterministic `rng.rs` (SplitMix64 + FNV-1a, **no `rand`
      dep**): per-wallet **sticky persona** (`PersonaSet::assign` = stable `fnv1a(pubkey) % n`) →
      `disguise::draw` a landing `Disguise { variant, cu_limit, cu_price, tip }`. Variant drawn from
      the persona's pool intersected with the catalog encodings the op's `Amount`+stage can take
      (denom-safe — a disguise never breaks `prepare`). Landing guarantees: `cu_limit =
      real_consumption_cu(cfg, spec) + persona_headroom` where the floor reads the executor
      `ComputeBudgetCfg` **SSOT** (can't drift), and `cu_price ≥ persona.cu_price.min > 0` (a snipe
      never zero-fees). **Personas, not independent per-field jitter** — jitter seeded per
      `(wallet, op.id)` so a wallet's ops share its persona yet differ op-to-op (reproducible/replayable).
- [x] **Offline persona derivation seam:** `PersonaSet::from_config(json)` loads a `forge/lab`
      clustering job's `personas.config` (validated against the catalog at load — an off-catalog or
      zero-cu-price template is rejected); `PersonaSet::builtin()` is a 3-archetype starter set
      (`aggressive_sniper`/`patient_accumulator`/`balanced_churner`) for forge/live before that job
      ships. `funding`/`schedule` remain typed `Plan` inputs from the unlinkability workstream. Sells
      default un-bundled / direct-send, **`tip: None`**.
- [x] Disguise stays **forge-only** — the module lives in `forge/orchestrator` (neither lab nor
      hunter links it); hunter/live keeps its lean snipe path, no per-tx sampling.
- **Gate D:** ✅ `cargo test -p forge-orchestrator` green (22, +16 new: rng/personas/disguise/macros);
      `gate_d_persona_coherence_and_sell_no_tip` builds a `volume_make`+`exit` plan for one wallet,
      asserts every op shares that wallet's persona, the three volume buys' disguises are **not**
      byte-identical (jitter varies), and the exit sell's `tip` is `None`; `composed_plan_shares_idseq
      _and_validates` runs fund→launch→volume→exit→consolidate through one IdSeq (unique ids) +
      `dry_run`. `cargo check --workspace` = 0 errors; executor tests still green (18+34);
      `cargo tree -p forge-lab`/`-p hunter-lab` link **none** of orchestrator/executor.

### Phase E — Fingerprint auditor (mandatory) — ✅ DONE (2026-07-09)
- [x] `orchestrator/audit.rs` — all 10 rules as standalone unit-testable fns over an `Ctx { plan,
      profiles, cfg }`: star funding (funding-graph fan-out ≥ N), equal amounts (identical swap
      legs ≥ N), same-slot cluster (non-launch ops sharing a `SlotKey` — launch `Snipe`/`Create`
      excluded as legitimately-atomic Jito bundles), direct own-graph token edge (`TransferToken`
      whose target is an acting own-wallet — SOL consolidate deliberately NOT flagged), constant
      CU/tip (disguises collapsing onto one `(cu_price, tip)` pair), synchronized `Bundler` exit
      (≥ 2 bundler sells in one slot), nonce-account reuse, `DurableNonce` on a non-`Snipe` op,
      clustered `close_token_ata`, and **account-shape integrity** (declared fee recipient must sit
      at `PUMP_FEE_RECIPIENT_INDEX` = **17**, mirroring the curve-buy account layout — a mangled
      list is `Severity::Reject`).
- [x] The build-shape rules (CU/tip · nonce · ATA · account list) read a per-op **`SendProfile`**
      (disguise + nonce_account/durable_nonce/close_token_ata/accounts/fee_recipient) that the Phase F
      build step will resolve; all fields default so a **pre-build Plan-only `audit(plan)`** still
      runs the graph/amount/schedule/token-edge rules. `audit_with(plan, &profiles, cfg)` runs
      everything. Output = `AuditReport { findings, score, hard_reject }`; **`passed(allow_fingerprint)`
      = `!hard_reject && (allow_fingerprint || no Warn findings)`** — a hard reject (malformed account
      shape) is **never** overridable, matching the invariant "account index 17 fee recipient never
      reshaped".
- [x] SSOT: `PUMP_FEE_RECIPIENT_INDEX` mirrors the executor's positional layout (the *pubkey* stays
      owned by `pump_trader::protocol`); `OpKind` reused (not `Ord`, so equal-amounts keys on a
      `&'static str` kind tag).
- **Gate E:** ✅ `cargo test -p forge-orchestrator` green (31, +9 new): `gate_e_naive_plan_is_rejected`
      builds the deliberately-naive plan (star fund + equal 0.02-SOL legs + unscheduled same-slot
      sells + a direct own-graph token transfer + a reused nonce + a mangled account list) → asserts
      hard-reject + every fingerprint rule fired + `!passed(true)`; `gate_e_clean_plan_passes` builds
      a real launch bundle + delay-spread varied-amount volume/exit + SOL-only consolidate +
      persona-drawn disguises + fresh nonces + correct account shape → `passed(false)` with `score ==
      0`. Executor tests still green (18+34); `cargo tree -i forge-orchestrator` from both `*-lab` =
      no match (partition holds). Zero SOL, zero network.

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
