# Project Cleanup Plan — Unused / Legacy / Duplicated Logic

Goal: identify and remove dead code, keep "future-use" logic deliberately, and resolve
duplication. We work **one big-logic area at a time** (checklist below). Per item we tag:

- 🟥 **DEAD** — zero references, safe to delete after confirm.
- 🟨 **FUTURE** — unused now but intentionally kept for an upcoming feature → keep, but document/`#[allow]` explicitly so it's clearly deliberate.
- 🟦 **DUP** — used but duplicated / near-identical with another module (may be intentional clone — confirm before merging).
- 🟩 **HYGIENE** — minor: over-broad `pub`, dead imports, leftover artifacts, stale docs.

Compiler signal: `cargo check --bin backend` is clean to compile but emits **26 dead-code
warnings**; `pump-trader` is clean. Several modules carry blanket `#![allow(dead_code)]`
(`ingest_laserstream`, `models/token_info`) which *hide* further dead code — those need manual review.

---

## Big-logic areas to audit (work order)

### Backend (Rust)

- [ ] **1. Sweep engine** (`backend/src/sweep/`) — **highest dead-code concentration**
  - 🟥 `corpus.rs`: `from_cached` (94), `sweep_trade_from_cached` (115), `CacheSource` struct + `new` (377/382) — never used.
  - 🟥 `engine.rs`: struct fields `tokens`,`combos`,`rows`,`fired` (73) never read.
  - 🟥 `grouping.rs`: `from_token` (50) never used.
  - 🟥 `progress.rs`: `CancelOnly` struct (124) never constructed.
  - 🟥 `projection.rs`: `clone_table`,`len`,`is_empty` (123) never used.
  - 🟥 `strategy.rs`: `id` (395) never used.
  - 🟨/🟩 `strategies/tpsl1.rs` & `tpsl2.rs`: `Strategy` unused import (registry.rs:26); `rem`/`col` assigned-never-read (dead-store in decode loops).
  - Decide: is the `*_from_cached` corpus path a planned cache feature (FUTURE) or abandoned (DEAD)?

- [ ] **2. Strategies tpsl_sniper_1 / tpsl_sniper_2** (`backend/src/strategies/`)
  - 🟦 The two trees are **intentional clones** (per project memory — do NOT merge into shared core; that's a won't-fix). Audit only for *internal* dead code, keep them parallel.
  - 🟨 `tpsl_sniper_1/exit/mod.rs:198,448` and `tpsl_sniper_2/cohort.rs:54` use `#[cfg_attr(not(test), allow(dead_code))]` — test-only helpers, keep.
  - Confirm `backtest.rs` (both) is still wired to the tpsl1/tpsl2 HTTP handlers (it is) → KEEP.

- [ ] **3. State caches** (`backend/src/state/`)
  - 🟥 `app_state.rs`: `available_permits` (153), `set_settings`/`set_sol_price`/`subscribe_sol_price` (228) never used.
  - 🟥 `token_cache.rs`: field `is_curve` (71), `wallet_table`/`initial_price` (272) never used.
  - 🟥 `swing_run_cache.rs`: field `created_at` (30) never read.
  - Decide FUTURE vs DEAD for the `subscribe_sol_price` pub-sub (looks like an unused notify hook).

- [ ] **4. Storage repositories** (`backend/src/storage/repositories/`)
  - 🟥 `analysis_repo.rs`: `upsert_result`,`upsert_creator_profile` (85) never used.
  - 🟥 `wallet_repo.rs`: `find_by_address`,`touch_last_seen` (95) never used.
  - 🟥 `tpsl1_paper_trading_repo.rs:466` & `tpsl2_paper_trading_repo.rs:509`: `count_by_run` never used (dup dead method in both clones).
  - 🟨 Several `#[allow(dead_code)]` repo fns (`settings_repo:129`, `token_info_repo:113`, `token_repo:213`, `trade_repo:482`) — confirm each is FUTURE vs DEAD.

- [ ] **5. Models** (`backend/src/models/`)
  - 🟥 `analysis.rs:21` `new` never used; `trade.rs:71` `pool_spot_price`/`chart_spot_price` never used.
  - 🟨 `token_info.rs` has file-level `#![allow(dead_code)]` — manual sweep needed (hidden dead code).
  - 🟨 `position.rs:137` `#[allow(dead_code)]` — confirm.

- [ ] **6. Analyzers** (`backend/src/analyzers/swing_analyzer.rs`)
  - 🟥 struct fields `slot`,`position`,`execution_price` (167) never read.
  - Otherwise actively used by tokens/swing handlers → KEEP module.

- [ ] **7. Ingest pipeline** (`backend/src/ingest_laserstream/`) — has blanket `#![allow(dead_code)]`
  - 🟨 `decoder/trade.rs` & `decoder/create.rs`: ~30 `#[allow(dead_code)]` fields — these are full protobuf/decoded-struct field sets; likely FUTURE (kept for completeness of decoded model). Confirm intent, document.
  - 🟨 `adapter_rpc.rs`: confirmed **still used** (token_sync RPC→protobuf path) → KEEP despite "gRPC is sole live transport" (it's the backfill/sync path, not live).
  - Manual review needed because the blanket allow hides real dead code.

- [ ] **8. Services** (`backend/src/services/`)
  - `token_sync.rs` (1843 lines — largest file): audit for dead private helpers (hidden, no warnings if `pub`).
  - `laserstream_replay.rs`: confirm it's wired (reconnect replay) vs orphan.
  - `clients/coingecko.rs` + `jupiter.rs`: both used (sol_price fallback, wallet_tokens) → KEEP.

- [ ] **9. API handlers** (`backend/src/api/handlers/`) — audit for unused endpoints / handlers not in router (`api/mod.rs`).

### Frontend (React/TS)

- [ ] **10. Duplicate `cn()` util** — 🟥 `components/token-price-chart/cn.ts` is a local copy of `lib/cn.ts`; only 3 files use the local one. Collapse to `lib/cn.ts`.

- [ ] **11. tpsl1 vs tpsl2 component duplication** (`components/tpsl1/` ↔ `components/tpsl2/`) — 🟦 mirrors the backend clone policy:
  - **Identical** (0 diff): `TokenInspectModal.tsx`, `utils.ts` → candidates to share (or keep mirrored to match backend policy — decide).
  - **Divergent** (intentional): `SimSummaryCard`, `RuleFormModal`, `ruleColumns`, `tableColumns` → KEEP parallel.

- [ ] **12. Export hygiene** (`components/analysis/swingParams.tsx`) — 🟩 several `export`s never imported elsewhere (`RangeInputSide`, `RangeInputs`, `swingParamLabelClassName`, internal prop types). Drop `export`.

- [ ] **13. `EXIT_REASON_META` duplication** — 🟩 defined in both `components/dashboard/creationStats.ts` and `tpsl2/SimSummaryCard.tsx`. Lift to one shared constant if truly identical.

### Repo hygiene (non-code)

- [ ] **14. Tracked root scratch docs** — 🟩 `tpsl2-param-analysis.md`, `strategy-token-enrich-plan.md`, this `cleanup-plan.md` are root `*-plan`/scratch files. Per CLAUDE.md, temp plan files get deleted when done. Confirm which are still live.
- [ ] **15. Large ignored artifacts** (FYI, already gitignored, not in repo) — `backup.dump` (812M), `sweep-out/` (151M), `target/`, `target-check/`, `.env.7z`. No git action; flag only if disk cleanup wanted.

---

## Notes
- `pump-trader` crate compiles with **zero** dead-code warnings — no action.
- The 18 `#[allow(dead_code)]` markers split into: legit FUTURE/test-only (keep, but verify) vs masking real DEAD (remove the allow + the code).
- Each 🟥 item: confirm zero refs (grep) → delete → `cargo check` clean → note in commit.
