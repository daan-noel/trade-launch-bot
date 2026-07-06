# Dead / Redundant / Unused Code — Cleanup Plan

Full-project audit (2026-07-06). Findings verified by cross-referencing every symbol across the
whole workspace (rustc does **not** warn on unused `pub` items in libs, so these were found by
grep, not the compiler) + `knip` (frontend, configured with **both** `live`+`lab` entrypoints) +
`cargo-machete` (deps).

**Nothing is deleted yet.** Work top-down; run the verification gate after each phase.

Legend: `[ ]` todo · **HIGH** = verified unused everywhere · **MED** = needs a human glance before cut.

---

## Phase 0 — 7.3 GB junk (instant, zero-risk)

- [ ] Delete `frontend-react/src/shared/components/tokens/target-check/` — **7.3 GB** of cargo
      build artifacts accidentally created inside the frontend source tree (someone ran
      `cargo --target-dir target-check` from that folder). Untracked + already gitignored.
- [ ] While here, confirm no other stray build dirs: `find frontend-react/src -type d -name "target*" -o -name debug`

**Gate:** none needed (untracked artifacts).

---

## Phase 1 — Unused dependencies

### 1a. `live/Cargo.toml` — HIGH (agent-verified, no source refs)
- [ ] Remove `borsh`, `url`, `rand`, `solana-client`, `thiserror` (unpinned, zero usage).
- [ ] `spl-token`, `spl-token-2022`, `spl-memo`, `spl-associated-token-account` — unused *directly*
      but exact-`=`-pinned. Run `cargo tree -i spl-token` first; only remove if the pin is redundant
      with pump-trader's transitive graph (otherwise the pin is load-bearing).

### 1b. `lab/Cargo.toml` — HIGH
- [ ] Remove `thiserror` (no `#[derive(Error)]`/`#[error(...)]` anywhere in `lab/src`).

### 1c. `trading_core/Cargo.toml` — MED (cargo-machete flags; verify each with grep first)
- [ ] Grep-verify then remove any truly-unused of: `actix-cors`, `async-trait`, `dotenvy`,
      `solana-client`, `thiserror`, `tracing-subscriber`, `url`. (macro/re-export use can hide a dep —
      confirm before cutting; `tracing-subscriber` in particular is often used only in a bin's `main`.)

**Gate:** `cargo check -p live && cargo check -p lab && cargo check -p trading_core` (clean).

---

## Phase 2 — Whole dead Rust units (HIGH, verified unused workspace-wide)

- [ ] **`TokenSyncStateRepo`** — delete file `trading_core/src/storage/repositories/token_sync_state_repo.rs`,
      the `CoreState::token_sync_state_repo()` accessor (`state/core_state.rs:142`), and the `mod.rs:9`
      decl. Superseded by `TokenInfoRepo::{get_sync_watermark, update_sync_watermark}`; zero call sites.
- [ ] **Kernel simulate cluster** (`strategies/kernel.rs`) — remove `simulate_rule` (:272),
      `simulate_token` (:287), `SimConfig` (:185), `RunMetrics::to_run_metrics` (:235) + their tests.
      Advertised "one sim path" never adopted; `lab` uses its own engine.
- [ ] **`ingest-laserstream` health subsystem** ⚠️ *(runs on the hot decode path, never read)* —
      remove `IngestHandle::health()` (lib.rs:303), `health_watch()` (:308), the `HealthSnapshot`
      re-export (lib.rs:41), `health.rs` (`HealthState`, watch channel), and the
      `health_state`/`health_rx` fields (lib.rs:257-258). `live` uses its own `DbHeartbeat`.
- [ ] **`PaperRun` model** — remove `models/paper_run.rs` (`PaperRun` struct + `PaperRunStatus` enum +
      `as_str`/`FromStr`) and the `models/mod.rs:20` re-export. Never constructed/read; no repo maps it.
      (NOT the live `enum PaperRun{Fresh,Continue}` — different type.)
- [ ] **4 compaction-probe methods** (`lab/.../grouped_sweep_repo.rs:666-719`) —
      `list_all_groups_for_compaction`, `fetch_combo_metrics_for_group`, `delete_combos_except`,
      `vacuum_full_results`. The `compact-sweeps` subcommand was never wired into `lab/main.rs`.

**Gate:** `cargo check -p live && cargo check -p lab && cargo check -p trading_core` + `cargo test` on touched crates.

---

## Phase 3 — Dead Rust `pub` items, methods & constants

### 3a. Dead constants — ~40, unguarded duplicates of `ingest-laserstream`'s live copies (HIGH)
- [ ] `config/constants/discriminators.rs` — remove the 23 dead consts (keep only
      `PUMP_SWAP_BUY_EVENT_DISCRIMINATOR` / `PUMP_SWAP_SELL_EVENT_DISCRIMINATOR`, the only two used).
- [ ] `config/constants/protocol.rs` — remove `program_friendly_name` (:54) and the program-ID cluster
      (lines 33-50) that only it reads; `ASSOCIATED_TOKEN_PROGRAM_ID`/`TOKEN_2022_PROGRAM_ID` fall too.
      Keep `EVENT_AUTHORITY`/`FEE_PROGRAM_ID` (pinned by `protocol_constants_ssot.rs` guard test).
- [ ] `config/constants/token_math.rs` — remove `INITIAL_REAL_TOKEN_RESERVES` (:12),
      `PUMPFUN_GENESIS_PRICE_PER_RAW_TOKEN` (:20).
- [ ] `config/constants/tuning.rs` — remove `POOL_REFRESH_INTERVAL_SECONDS` (:105), `resolve_slippage_bps` (:26).

### 3b. Dead repo / model methods (HIGH)
- [ ] `strategy_repo.rs` — `find_metrics` (:931), bare `find_positions_by_rule` (:1077, paged variants stay).
- [ ] `raw_tx_repo.rs:113` `find_by_signature` · `wallet_dict_repo.rs:83` `addresses_for` ·
      `settings_repo.rs:194` `get_one`.
- [ ] `strategies/swing_1/exit/mod.rs:134` `LadderParams::has_time_exit` (recomputed in `exit_state.rs:78`).
- [ ] `wallet_interner.rs:38` `into_table` (test-only).

### 3c. pump-trader (HIGH)
- [ ] `error.rs` — remove `TradeError::SlippageExceeded` (:22), `TradeError::NotFound` (:34) — never constructed.
- [ ] `trader/buy.rs:77` `buy_token_snipe` — superseded by `buy_token_snipe_write_ahead`; zero callers.

### 3d. Stale attributes / visibility (LOW — tidy)
- [ ] `config/settings.rs:8` — drop the stale `#[allow(dead_code)]` on `helius_api_key` (field IS read).
- [ ] `ingest-laserstream/slot_anchor.rs:43` `is_replay` — remove (0 refs).
- [ ] `ingest-laserstream/config.rs` — `pool_activity_window` (:59), `pool_refresh_interval` (:56) unread (MED).
- [ ] `tpsl_sniper_1/2` test-only clones `should_position_exit_on_clock`, `find_all_matching_buy_rules` —
      add `#[cfg_attr(not(test), allow(dead_code))]` to match their marked siblings (keep as oracles).
- [ ] `api/handlers/tokens/sql.rs` — `SqlArgs::{len,is_empty}` unused; narrow `SqlArgs` to `pub(crate)`.

**Gate:** `cargo check` (all 3 crates) + `cargo test -p trading_core -p live -p lab` + `cargo test -p pump-trader`.

---

## Phase 4 — Frontend dead code

`knip.json` was added at `frontend-react/knip.json` so this is repeatable (the default config falsely
flagged the entire `lab` app because it only saw the `live` entrypoint). Re-run: `cd frontend-react && npx knip`.

### 4a. Orphaned files — HIGH (zero-reference)
- [ ] Removed "Transactions" feature: `src/live/pages/transactions/TransactionsPage.tsx`,
      `src/live/components/transactions/tradeColumns.tsx`, `src/live/hooks/useTradeStream.ts`.
- [ ] `src/shared/components/layout/PageShell.tsx`, `src/lab/pages/strategies/sweep/index.ts`.

### 4b. Unused exports / types — 163 total (MED — trim, don't rush)
- [ ] Trim the `token-price-chart/index.ts` barrel to the ~few *types* actually imported
      (ChartSwingLeg/ChartEventMarker/ChartSwingOverlay); ~30 value re-exports are dead.
- [ ] Work the remaining list from `npx knip` (creation-stats helpers, chartBars/chartTimezone
      helpers, strategyColumns extras, etc.). Delete per-file, rebuild between batches.

### 4c. Deps & deprecations
- [ ] Add `fancy-canvas` to `package.json` (used by chart plugins, currently unlisted transitive).
- [ ] Remove `@deprecated` `truncate` prop in `ui/AddressDisplay.tsx` + the deprecated
      `chartPriceFormatter`-area marker in `token-price-chart/constants.ts:131`.
- [x] `tailwindcss` "unused dep" — **false positive** (`@import 'tailwindcss'` in `index.css`); keep.

**Gate:** `npm run build` clean (tsc checks BOTH trees) + no extra re-render on SOL/USD tick or live-trade stream.

---

## Phase 5 — Housekeeping (docs/config — confirm before deleting)

- [ ] Stale root plan docs referenced nowhere: `live-lab-reskin-plan.md`,
      `token-first-slot-activity-plan.md`, `_project_audit_plan/`. Archive into `@plans/` or delete.
- [ ] Fix stale doc references to retired `DbSource` / `SWEEP_CORPUS_SOURCE` / `compact-sweeps` in
      `@arch/database.md`, `@plans/database/db-patterns.md`, and `lab/src` comments
      (`sweep/mod.rs:13`, `sweep/corpus.rs:7,162`, `lake/duck.rs:4,18,80,112`).
- [ ] `.env.backup` — delete stale local backup (gitignored).
- [ ] Update docs per CLAUDE.md "Definition of done" for whatever Phases 2-3 changed
      (`@arch/database.md` for the dropped repo, `@arch/ingest.md` for the health subsystem, etc.).

### Confirmed NOT dead (leave alone)
- `ingest-websocket` (40-line empty scaffold) — intentional future transport per CLAUDE.md.
- `aws-ec2-key.pem`, `.env` — gitignored, not committed (no secret leak).
- pump-trader `constants.rs` shim, `tpsl_sniper_1/2` decision clones, all `probe`/`claim`/`raw-tx`/
  `rpc-backfill` cfg blocks, Borsh positional-deser structs' unread fields — all intentional.

---

## Suggested order & risk

| Phase | Effort | Risk | Payoff |
|---|---|---|---|
| 0 — 7.3 GB junk | 1 cmd | none | 7.3 GB disk |
| 1 — deps | small | low | faster builds, fewer warnings |
| 2 — dead units | medium | low (verified) | removes whole superseded subsystems |
| 3 — pub items/consts | medium | low | ~40 consts + dead methods |
| 4 — frontend | medium | low | 5 files + 163 exports |
| 5 — docs/housekeeping | small | none | accuracy |

Do 0 + 1 immediately. Batch 2-4 with a `cargo check`/`npm run build` gate after each. 5 last.
