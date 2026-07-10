# Full-repo refactor audit — 2026-07-10

Seven parallel audits (hunter/core, hunter/live+lab, forge crates, shared crates + workspace,
cross-product duplication, both frontends, deploy/Docker/docs) on branch
`feat/restructure-hunter-forge`, working tree as-is. Constraints agreed: breaking changes OK,
no behavior-preservation requirement. This file is the synthesis; per-area detail is inline.

> Note: the deploy audit observed the working tree changing during the run (split compose dirs
> being deleted by another session). Findings referencing `deploy/hunter-live/` etc. may already
> be resolved by that in-flight work — re-check before acting on §6.

---

## EXECUTION STATUS — re-verified 2026-07-10 (scope agreed with user)

Whole-repo re-check after significant code changes since the audit. **Agreed scope:**
- **C1 (strategy triplication) — OUT.** tpsl1/tpsl2/swing1 are intentional clones; keep them.
- **C2 (forge↔hunter infra duplication) — OUT.** forge was copied from hunter and is still WIP;
  do not extract to shared crates / de-duplicate yet.
- **In scope:** all remaining **B** bugs + the **C3** dead-code cleanups (delete unreachable).

**B status — ALL DONE (2 were already fixed pre-session; 15 fixed this session):**

| Item | Status | Item | Status | Item | Status |
|------|--------|------|--------|------|--------|
| B1 | ✅ CAS `claim_for_submitting` | B7 | ✅ poison-recovery lock | B13 | ✅ e500 generic + e400 Display |
| B2 | ✅ `FUNDING_LOCK` serializes passes | B8 | ✅ retain batch + `MAX_PENDING_ROWS` | B14 | ✅ lab-api 127.0.0.1 (both) |
| B3 | ✅ `${…:?}` pw + 127.0.0.1 (both) | B9 | ✅ `revert_stale_funding` sweep + `reserved_at` stamp | B15 | ✅ exhaustive match + warn |
| B4 | ✅ forge-lab bearer gate (token optional) | B10 | ✅ strict `env_u64/f64` → `Result` | B16 | ✅ log+counter only on real drop |
| B5 | ✅ (pre-session) | B11 | ✅ tpsl1 emits `target_*`; `rule_id: Option<Uuid>` both; FE type aligned | B17 | ✅ `constant_time_eq` |
| B6 | ✅ (pre-session) | B12 | ✅ untracked + gitignored + `.example` | | |

**C3 — ALL DONE (with two precision calls noted):**
- C3-1 `shared/ingest/websocket` → **deleted** (crate + member + doc refs).
- C3-2 forge/orchestrator → **`dryrun.rs` deleted** (+ macros tests repointed to `prepare()`); **funding graph
  deleted** (`Plan.funding`/`Funding`/`FundingEdge`/`StarFunding`/`star_sources` — genuinely dead, edges never
  populated). **`Plan.schedule` KEPT** — it feeds LIVE audit rules (`same_slot_cluster`,
  `synchronized_bundler_exit`) via the `Unscheduled` bucket, so removing it changes audit behavior, not dead
  code. `Plan` is persisted JSON but removing a field is serde-read-compatible (unknown keys ignored).
- C3-3 hunter/core legacy `Position` → **dropped the 3 `#[allow(dead_code)]` mutators** (`set_target`,
  `mark_buy_submitted`, `mark_entry_filled` — live uses `StrategyPosition`'s same-named copies) + repointed their
  tests. Full DTO reduction was NOT done: `close`/`mark_exit_*`/read-helpers are exercised by the core exit tests
  (C1 clone territory, off-limits).
- C3-4 forge/lab `run_export` `Ok(0)` stub → **deleted** (+ its CLI branch; unknown args now error). `lake::schema`
  SSOT seam kept (`#[allow(dead_code)]`).
- C3-5 `forge/scripts/db-incremental-sync.ps1` stub → **deleted** (roadmap note updated).
- C3-6 `transfer_with_seed` → renamed **`system_transfer`** (const + catalog row). Venue-neutral rows keep
  `VenueId::PumpFun` (the enum is a deliberate closed single-variant; a neutral arm is a speculative 2nd-venue
  change) — clarified via comment instead.

**Also fixed (pre-existing, unblocked hunter-core tests):** `table_eval.rs` `include_str!` path
`frontend-react/` → `frontend/` (stale from the earlier frontend rename).

**USER FOLLOW-UPS REQUIRED:**
- **B12:** the old committed bcrypt hash is compromised (still in git history). Regenerate a UNIQUE per-gate
  `.htpasswd` (`htpasswd -B -C 12 -c …`) for hunter-live / hunter-lab / forge-live before deploy (see each
  `.htpasswd.example`). Consider scrubbing the hash from history.
- **B3:** compose now REQUIRES `POSTGRES_PASSWORD` in the env-file (no default) — sync `.env` before `compose up`.

**Verification:** `cargo check` clean across all 10 touched crates; tests green — orchestrator 24, http-auth 5,
hunter-core position 12 + table_eval 6. (`cargo 1.96.1` builds here — old "no linker" note is stale.)

---

## A. Verdict in one paragraph

The codebase is much better engineered than a typical solo trading project — the live hot path,
the executor send engine, and the ingest transport are genuinely strong, error handling is
disciplined, and invariants are pinned by tests. The three real problems are:
(1) a handful of **real-money correctness/security bugs** (bundle double-submit race, funding
rails only enforced per-pass, unauthenticated exposed lab API, default-password postgres publish);
(2) **the strategy triplication**: tpsl1/tpsl2/swing1 are ~80–88% copy-pasted at *every* layer
(core exit machinery, lab handlers, frontend pages/columns/API) — ~7,000 removable lines and the
single biggest tax on adding strategy #4;
(3) **the venue seam is mostly aspirational**: shared/ingest's core/venue split is real, but
executor-core still contains pump.fun retry codes and CU shapes, hunter/core has no venue
abstraction at all (stringly `"curve"/"amm"`, baked 30-SOL/1B-supply constants), and forge's
"venue-agnostic" orchestrator hardcodes `VenueId::PumpFun` in every constructor while the
launcher never consults its own `LaunchpadAdapter` trait. Extending to launchpad #2 today would
be a rewrite, not a trait impl.

---

## B. Correctness / safety / security (fix before anything else)

| # | Sev | Where | Defect | Fix |
|---|-----|-------|--------|-----|
| B1 | CRITICAL | `forge/launcher/src/bundle_execute.rs:66-155` | Read-check `status == planned` then unconditional `set_status(Submitting)` with ~8–12 RPC round trips in between → two concurrent executes can submit two different signed tx sets from the same bundler wallets (double real-SOL buys) | CAS: `UPDATE bundles SET status='submitting' WHERE id=$1 AND status='planned' RETURNING id` as step 1 |
| B2 | HIGH | `forge/launcher/src/wallet_funding.rs:101-143,337-361,675-698` | Treasury reserve floor + per-interval spend cap enforced against pass-local snapshots; background funder + manual fund + JIT fund can run concurrently → combined spend N× cap, reserve breach | Serialize funding passes (tokio::Mutex in-process, or pg advisory lock) |
| B3 | HIGH | `deploy/hunter.compose.yml:77,82` + forge equivalent | Postgres host-published on 0.0.0.0 with `:-password` fallback; forgotten `--env-file` = default password on a public port | `${POSTGRES_PASSWORD:?}` (required) + bind `127.0.0.1:${DB_PORT}:5432` |
| B4 | HIGH | `forge/lab/src/main.rs:57-61` + `deploy/forge.compose.yml:154-155` | forge-lab has NO auth middleware and is published on 0.0.0.0:8240 (only bin of four without the shared gate) | Wire `require_bearer_auth` like hunter/lab, and/or publish loopback-only |
| B5 | HIGH | `forge/frontend/tsconfig.json:12` | `"ignoreDeprecations": "6.0"` rejected by the pinned TS 5.6.3 → `npm run build` fails with the project's own toolchain | Bump forge to TS ~6.0 (align with hunter) |
| B6 | HIGH | `forge/frontend/vite.config.ts:11-19` | Dev proxy doesn't inject `Authorization: Bearer` → 401s against forge-live's new fail-closed auth | Copy hunter's `vite.config.base.ts:88-93` pattern (loadEnv + proxy headers) |
| B7 | MEDIUM | `shared/executor/pumpfun/src/trader/amm.rs:743,783` | `.lock().unwrap()` on the AMM trade path panics on poison (rest of crate uses `unwrap_or_else(|p| p.into_inner())`) | Same poison-recovery idiom |
| B8 | MEDIUM | `forge/live/src/ingest/consumer.rs:152-166` | `flush()` clears batches even when `insert_batch` failed → transient DB error permanently loses trades (inserts are idempotent; retry is safe) | Retain batch on error, retry next flush |
| B9 | MEDIUM | `forge/launcher/src/wallet_funding.rs:214-219` | `funding` state has no TTL; dropped manual-mode tx leaves wallet invisible to claims forever | Extend reservation sweep to revert stale `funding` |
| B10 | MEDIUM | `forge/.../config.rs:344-350` | Malformed FUND_*/safety-rail env values silently fall back to defaults (`5O000000` → default reserve) | Parse errors on safety rails must be fatal |
| B11 | MEDIUM | `hunter/live` vs `hunter/lab` PositionResponse | Live defines its own wire struct (`live/.../positions.rs:30`), lab uses `core/src/models/position.rs:341`; they've already drifted (`run_id`/`swing_legs` vs `target_*`) while the shared frontend assumes one shape | Collapse to the core model, extended with live-only fields |
| B12 | MEDIUM | `deploy/{hunter-live,hunter-lab,forge-live}/nginx/.htpasswd` | Real bcrypt hash committed, identical across all three gates, cost 05 | Gitignore, rotate per-gate with `-B -C 10+`, mount at runtime |
| B13 | MEDIUM | `forge/live/src/http.rs:27` + `forge/lab/src/http.rs:11` | `e500` returns `format!("{e:?}")` → leaks internal SQL/anyhow chains to clients | Log detail, return generic body |
| B14 | MEDIUM | `deploy/hunter.compose.yml:152-153` | lab-api published on 0.0.0.0 bypasses the nginx Basic-auth wall for all GETs | Loopback publish or document the security-group requirement |
| B15 | LOW | `hunter/core/.../runtime_cache.rs:1213-1221` + `live/.../service.rs:1226-1232` | Unknown strategies silently labeled "tpsl1" in SSE — landmine for strategy #4 | Exhaustive match / explicit error |
| B16 | LOW | `shared/ingest/core/src/session.rs:115-124` | Logs "dropping event" before the retry that usually succeeds; final drop is silent with no counter | Log only on final drop + counter |
| B17 | LOW | `shared/http-auth/src/lib.rs:65` | Token compare not constant-time | `subtle` crate |

Also: hunter has **no `/health` route** on either bin, and neither compose stack has app
healthchecks, resource limits, or log rotation (§6).

## C. Redundancy (ranked by removable lines)

**C1 — The strategy triplication (~7,000 lines, CRITICAL priority).** tpsl1/tpsl2/swing1 are
"intentional clones" whose ladder rungs are in fact identical; drift is prevented only by
discipline, and the wire DTO drift (B11) shows discipline already failed once.

- hunter/core: exit modules ~82% identical after name-normalization; `util.rs` byte-identical;
  `ExitReason`/`ExitFill`/`ExitWalkState`/`CachedExitState`/`LadderParams`/walkers defined 3×;
  `exit_state.rs` (223 lines) exists purely to enum-wrap the duplicates; registry `*Params`
  repeat 16 base fields + field-by-field mirrors (~370 lines). → shared `strategies/ladder/`
  module + `BaseParams` with `#[serde(flatten)]` extension. ~2,000 lines.
- hunter/lab: `tpsl1.rs`/`tpsl2.rs`/`swing1.rs` handler modules (2,476 lines, ~87% identical);
  live already unified this with `{strategy}`-keyed handlers — port that pattern. ~1,600 lines.
- hunter/frontend: `Tpsl1Page`/`Tpsl2Page` ~88% identical (1.2K lines each), live pages ~81%,
  `ruleColumns` ×2 ~81%, `services/api.ts:284-581` three parallel 12-function CRUD blocks →
  `makeStrategyApi(seg)` factory + one parameterized page/columns set. ~3,000–3,500 lines.

After this pass, strategy #4 = a registry entry + a param-extension struct + a FE config object.

**C2 — Cross-product infra duplication (forge copied hunter, drift already started).**
- `shared/db` crate: `DbPools`+`build_pool`+`session_guard_sql`+generic `connect(settings, Migrator)`
  (`hunter/core/src/storage/postgres.rs` ↔ `forge/core/src/storage/postgres.rs` are near-verbatim),
  CAgg-setup scaffolding (`timescale.rs` pair), `DbSettings::from_env` + env-parse helpers
  (forge's weaker copies accept `DB_MAX_CONNECTIONS=0`, ignore unparseable `PORT`). ~250 lines.
- `shared/units`: forge's decimals-generic `units.rs` wins; hunter's `sol_to_lamports`/
  `lamports_to_sol`/`LAMPORTS_PER_SOL` (3 defs) become re-exports; executor keeps its documented
  truncating u64 carve-out.
- Ingest consumer scaffolding into `ingest-core`: transport-build boilerplate is byte-identical
  in both live bins; hunter's channel-decoupled `db_writer.rs` vs forge's inline flush (which
  blocks the event loop on wallet-intern round-trips) solve the same problem twice.
- `shared/http-auth` bootstrap (`ApiAuth::from_env_required()`), `resolve_host/resolve_port`
  (hand-inlined twice in forge), hunter's extractor error-configs + a non-leaking error mapper,
  `/health` helper.
- `shared/sol-price`: CoinGecko+Jupiter fetcher exists twice (hunter services vs
  `forge/live/src/sol_price.rs`); products keep only their sinks.
- Small: `task_fault` duplicated verbatim in both hunter main.rs; forge's 6× repeated
  Option<JoinHandle> adapter; forge supervision returns `Ok(())` on task death where hunter
  exits non-zero — hunter's fail-fast pattern should win.
- `hunter/live/src/services/token_sync.rs:1411-1465` re-implements the ingest event→domain
  translators that exist in `live/src/ingest/consumer.rs:426-474` (the camelCase buy-ix bug
  came from exactly this dual-site pattern).

**C3 — Dead code.**
- `shared/ingest/websocket`: 40-line scaffold, `spawn` is `unimplemented!()`, zero consumers,
  and it depends on hunter/core (a shared crate importing a product — inverted layering). Delete.
- forge/orchestrator: ~40% production-unreachable (`dryrun.rs` 338 lines, most of `macros.rs`,
  `Plan.funding`/`Plan.schedule` never populated → StarFunding/SameSlotCluster/etc audit rules
  run over always-empty inputs; funding/dust transfers bypass the gate entirely, contradicting
  the "every SOL move is audited" doc claims). Decide: wire it or mark it roadmap.
- `hunter/core/models/position.rs`: legacy `Position` kept as SSE adapter with dead
  `#[allow(dead_code)]` mutators — reduce to serialize-only DTO.
- forge/lab lake: `run_export` returns `Ok(0)` with a warn — ops-script footgun; return an error
  until implemented. `forge/scripts/db-incremental-sync.ps1` is a non-functional stub (FDW setup
  is a TODO comment) with nothing marking it unusable.
- Misleading: `DEFAULT_SOL_TRANSFER_VARIANT = "transfer_with_seed"` but the executed ix is plain
  `system_instruction::transfer`; catalog rows for venue-neutral transfers carry `VenueId::PumpFun`.

## D. Extensibility (the venue/quote seam)

Adding venue #2 today requires: editing executor-core (VenueId arm, retry table, CU buckets),
writing a ~6.7K-line venue crate with nowhere central to register catalog variants, hand-writing
dispatch at 63 concrete `PumpFunTrader` call sites, extending pump-flavored `IngestEvent`, and
rewriting forge's launcher service. The plan below front-loads the cheap fixes so venue #2
becomes: implement 2 traits + add catalog rows + a dispatch layer.

- **executor-core**: move pump revert-code taxonomy (`retry.rs:67-122`) into the venue crate;
  replace `curve_buy_cu`/`amm_cu`-shaped `ComputeBudgetCfg` + `Engine.cu_ixs_*` with
  venue-supplied path-keyed CU profiles; move catalog *types* (`VariantSpec`/`VariantKind`/
  `Stage`/`Denom`) into core so venues contribute rows; decide whether `Venue` trait grows the
  real op set (Create/Buy/Sell/Transfer) — do this BEFORE a second concrete trader exists.
- **ingest-core**: `IngestEvent` is pump-flavored — `Venue{Curve,Amm}` bakes the curve→AMM
  lifecycle in; `Reserves` is the pump model; `TokenCreated` carries `bonding_curve`/
  `is_mayhem_mode`; amounts are SOL-only f64. Rename to a market-stage concept + typed venue
  payload extension when generalizing.
- **hunter/core**: introduce a `Venue` enum (kill stringly `"curve"/"amm"` and `is_amm: bool` in
  `TraderHook`); bundle pump curve economics (30-SOL baseline, 1B/2B supply, dead thresholds)
  into a per-venue `CurveEconomics` carried on Token/TokenState; lift `CostModel::pumpfun_default`
  out of the strategy kernel; move program IDs into a shared leaf crate (today duplicated
  "keep in sync by policy"); `derive_pump_swap_pool` hardcodes WSOL quote; thread a `Trader`
  trait through `StrategyService` (holds concrete `Arc<PumpFunTrader>` today).
- **forge**: Operation constructors must take `venue: VenueId` (all hardcode PumpFun at
  `plan.rs:214,249,300,336`); re-export `VariantKind`/`VenueId` from executor-core not
  pump_trader; give the launcher a venue trait for launch/dev-buy/bundle legs (the existing
  `LaunchpadAdapter` is only used by ingest); orchestrator's hand-mirrored `Role` vs
  `WalletRole` needs one SSOT. Quote currency: schema is generalized, code is lamports —
  acceptable for now, but `Amount` needs a quote-asset id before a non-SOL quote.
- **Façade rename** (pump-trader→executor-pumpfun, ingest-laserstream→ingest-pumpfun): cheap and
  mechanical (~85 refs / 37 files + 5 Cargo.tomls, delete ~60 shim lines); vocabulary is already
  split-brain (orchestrator uses both names). One dedicated commit. Until then fix hunter/live's
  duplicate alias declaration (declares the same alias by path with different features instead of
  `workspace = true`).
- **Workspace**: the "solana 1.17.27 pin" is a caret req — the lock is at **1.18.26**, so the
  `resolver = "1"` rationale rests on a pin that doesn't pin. Either `=1.17.27` or update the
  story and re-evaluate resolver "2". Migrate hunter crates to `workspace = true` deps. Forge's
  reqwest keeps default features → native-tls/openssl compiled into the real-money binary
  alongside two rustls stacks: `default-features = false, features=["json","rustls-tls"]`.

## E. Modularization & quality

- God files to split (no logic change): `hunter/live/services/token_sync.rs` 2069
  (replay/backfill/decode + a 400-line fn), `hunter/core/api/handlers/tokens/tokens.rs` 1937
  (DTO/column-registry/query/eval = 4 modules), `hunter/core/storage/strategy_repo.rs` 1915
  (~50 methods + its own query builder → split at aggregate boundary),
  `hunter/lab/.../grouped_sweep.rs` 1492 (contains the entire ~500-line sweep job runner —
  layer violation; move to `sweep/job.rs`), `forge/live/http.rs` 972, `forge/launcher/own_launch.rs`
  951 (7 repos in one file), `run_probe` 360 lines inline in hunter/live main.rs.
- 17–18 positional-arg constructors (`Token::new`, `Tpsl1Rule::new` etc.) — adjacent same-typed
  Options transpose silently; use builders/struct-literal defaults.
- Per-event `p.to_rule()` in registry entry dispatch allocates Strings + clones JSON on the live
  hot path per token-created × per rule (`registry.rs:573-707`) — cache like `ladder_by_id`
  already does.
- Stringly `run.status == "Running"` literals; mirror the `PositionStatus` enum.
- Hand-synced `TOKIO_WORKER_THREADS` constant (`lab/sweep/registry.rs:215`) + stale
  pre-split comment.
- Frontends: no ESLint/Prettier anywhere; no npm workspace root (precondition for any shared
  package); React 19/TS 6/Vite 8 vs React 18/TS 5.6/Vite 6 divergence; hand-mirrored backend
  DTOs (761-line types file, "add a field on both sides" comments) → generate TS from Rust
  (ts-rs or schemars→openapi-typescript); duplicated formatUsd/formatAge already diverged →
  extract Tier-1 pure format/link/baseApi kit (~400 lines) after version alignment.

## F. Performance (mostly good; targeted fixes)

Positive: live ingest→decision→execution path is well-engineered (bounded lossy channels,
render-once SSE fan-out, memoized exit walks, no locks across awaits); send path serializes once
and fans out Arc bytes; ingest decode uses memoized base58 + raw-byte program matching; lab
sweeps are memory-budgeted with warm corpus caches.

- `TokenQuery::matches` re-parses filter operand strings + re-lowercases per row per poll over a
  100K+ universe — precompile predicates at request build; `mint_in` Vec→HashSet.
- `reqwest::Client::new()` per Jito bundle submit (TLS handshake on the latency-critical path)
  and per sol-price fetch — share clients.
- `manage/execute.rs::build_wallet_trader` fully initializes a trader (~8–12 RPC) per leg; the
  volume bot pays this every cycle — reuse global-account state per action (bundle_execute
  already proved it's signer-independent).
- JIT funding does one `get_balance` per wallet sequentially — batch via `getMultipleAccounts`
  like the poller already does.
- `instruction_labels` deep-cloned per leg of multi-leg txs → `Arc<[String]>`.
- `Token` cloned on every TokenCreated before the `rules.is_empty()` check — reorder.
- `unique_wallets()` rebuilds a HashSet over 50K trades; the interner already knows the count.
- Sim single-rule path always cold-loads from DuckDB, never probes the warm sweep corpus cache.

## G. Deploy / docs / hygiene

- Compose: no resource limits (lab sweeps can starve the live hot path on the shared box — cap
  lab-api cpus/mem), no `logging:` rotation, healthchecks only on postgres (add `/health` to all
  four bins + `condition: service_healthy`). Positives: envsubst filter handled correctly, port
  scheme collision-free, shared-PG-two-DBs design sound, cargo-chef multi-stage builds with cache
  mounts, .dockerignore comprehensive.
- `CARGO_BUILD_JOBS=2` OOM guard added to hunter api Dockerfiles only — mirror to forge (forge-lab
  compiles the same libduckdb). Stale `EXPOSE` ports predate the port redesign.
- Docs: `hunter/CLAUDE.md` half-migrated (Architecture still says meme-trading/pump-trader/
  frontend-react; Commands updated); `forge/CLAUDE.md` + README still framed as
  solana-launch-platform with `crates/` paths; `forge/docs/*.md` say `cargo run -p lab/live`
  (packages no longer exist); `deploy/DOCKER.md` documents six compose files (in-flight);
  no root README/CLAUDE.md tying the monorepo together.
- Hygiene: `target-check/` = 21 GB reclaimable (plus a stray copy in `hunter/frontend/`);
  `_monorepo-backup/` 5.6 MB deletable post-merge; `.gitignore`/secrets otherwise clean
  (no tracked keys; `Cargo.lock` deliberately committed — correct).

---

## H. Phased refactor plan

**Phase 0 — Safety & security hotfixes (1–2 days, no structure change).**
B1 bundle CAS; B2 funding-pass serialization; B9 funding TTL; B7 AMM poison idiom; B8 flush
retry; B10 loud rail parsing; B4 forge-lab auth; B3 postgres required password + loopback;
B12 .htpasswd untrack/rotate; B5+B6 forge frontend build/proxy; B13 e500; B15 strat_label.
Each is a small independent diff; verify with existing tests + compose config.

**Phase 1 — Dead weight & hygiene (1 day).**
Delete `shared/ingest/websocket`; prune/mark orchestrator dead modules (decision: wire funding
through the gate, or `#[doc(hidden)]` + honest docs); legacy Position slim-down; lake stub →
error; façade rename commit; hunter deps → `workspace = true`; forge reqwest rustls; solana pin
story; doc sweep (both CLAUDE.mds, forge docs, DOCKER.md, root README); delete target-check +
frontend stray; rename `transfer_with_seed` catalog variant.

**Phase 2 — Strategy unification (the ~7K-line win; do before venue work so there are 3× fewer
copies to generalize).**
2a hunter/core shared ladder module + BaseParams (dissolves exit_state.rs, halves registry
boilerplate) — the memo-vs-oracle tests port over and protect the merge.
2b hunter/lab unified `{strategy}` handlers (port live's pattern) + B11 PositionResponse collapse.
2c frontend `makeStrategyApi` factory + parameterized strategy page/columns.
Gate: backtest parity run (same rules, identical results pre/post) + FE builds.

**Phase 3 — Cross-product convergence (shared/ extractions, mechanical).**
`shared/db` (pools/CAggs/settings/env-helpers) → `shared/units` → ingest consumer scaffolding
into ingest-core (fixes forge's blocking wallet-intern too) → http-auth bootstrap + api
conventions + `/health` everywhere → `shared/sol-price` → runtime helpers (task_fault, forge
adopts fail-fast supervision). Each extraction = one commit, forge and hunter cut over together.

**Phase 4 — Venue seam (before any venue-#2 code).**
4a executor-core de-pumping (retry taxonomy out, CU profiles, catalog types in core, Venue trait
decision). 4b hunter/core Venue enum + CurveEconomics + shared venue-const crate + Trader trait
in StrategyService. 4c ingest event generalization (market-stage + venue payload). 4d forge:
venue-parameterized Operation constructors, launcher venue trait, Role SSOT, executor-core
re-exports. Acceptance test: a mock "venue #2" (stub trait impls) compiles both products with
zero edits outside the new crate + dispatch tables.

**Phase 5 — Quality/perf polish (ongoing, piecemeal).**
God-file splits; constructor builders; precompiled TokenQuery; to_rule caching; shared reqwest
clients; trader-reuse in manage/volume; batched JIT balances; run-status enum; compose resource
limits/log rotation/healthchecks; npm workspace + version alignment + Tier-1 shared FE kit +
Rust→TS DTO codegen; ESLint/Prettier baseline.

Rough sizing: P0 ≈ 15 small diffs; P1 ≈ deletion-heavy day; P2 is the big one (~7K lines removed,
needs the parity gate); P3 ≈ 6 mechanical extractions; P4 is design-bearing (do 4a/4b carefully);
P5 is background work.
