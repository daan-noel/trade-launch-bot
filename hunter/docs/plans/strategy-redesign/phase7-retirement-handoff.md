# Phase 7 — legacy-strategy retirement: handoff

Status snapshot for continuing the tpsl1/tpsl2/swing1 retirement in a fresh
session. Branch **`chore/retire-legacy-strategies`** (off `strategy-redesign`),
**not pushed**. Working tree clean at handoff.

## Scope decisions already made (do NOT re-ask)

1. **Swing removed entirely** — both the swing1 *trading strategy* AND the swing
   *detection/analysis* tooling (chart pivot overlay, "Chain of Swings" sort,
   `swing1-detect`, swing-probe). User picked "delete swing entirely (literal plan)".
2. **Armed-history endpoint is dropped**, not rebuilt — the generic engine keeps
   only a live armed *snapshot* (`GET /api/strategies/armed`), no per-rule history.
3. **Manual-sell reconcile → route through `engine.manual_close`** — the live
   manual-sell path (`solana.rs`) must find the open real position by mint via the
   repo and drive it through the engine's own close path, NOT re-home the legacy
   `reconcile_externally_cleared_mint` (which is welded to the deleted runtime cache).
   This is a **real-money behavior change** → needs a paper + real smoke test.

## Commits already on the branch (newest first)

- `0a54e4ac` frontend — swing chart feature + legacy strategy dead code removed
  (tsc/lint/build:live/build:lab all clean).
- `b274512e` lab — legacy tpsl1/tpsl2/swing1 strategy stack retired (handlers,
  sweep families, registry legacy arms, backtest dirs, matched_mints, sim_spawn).
- `9acb42a4` docs — finished sweep plans consolidated (unrelated cleanup, swept in).
- `29d573dc` core+lab — swing-detection *analysis* feature removed (analyzers.rs,
  tokens.rs swing-sort, swing state/handlers, SwingDetectionFinished SSE).
- `4d111c30` live — `snipe_reserves_from_cache` inlined into the generic engine.

Verified green after each: `cargo check -p hunter-live -p hunter-lab` clean;
143 lab tests + 35 core token-list tests pass.

## What still remains

### Commit 4 — live: retire StrategyService (IN PROGRESS, not started editing)

Consumer map (grep-verified). Everything routes off `DeployState.strategy`
(a `StrategyService`) or the legacy `StrategyRuntimeCache`.

**A. `state/deploy_state.rs`**
- Replace field `pub strategy: StrategyService` (:34) with
  `pub strategy_repo: trading_core::storage::repositories::strategy_repo::StrategyRepo`.
- Ctor param (:71) + assignment; drop the `use crate::strategies::StrategyService` (:11).
- `strategy_cache()` accessor (:101-103) returns `self.strategy.runtime()` — **delete it**
  (only the legacy service had a runtime cache; grep for callers first — none expected
  outside the deleted paths).

**B. `api/handlers/strategies/positions.rs`** (KEEP the file — live positions UI)
- `repo()` helper (:241-243): `app_state.strategy.repo()` → `&app_state.strategy_repo`.
- `strategy_id(seg)` (:235-239): drop the `StrategyImpl::from_id` validation. All
  positions carry `strategy_id = "generic"` (engine stamps `GENERIC_STRATEGY_ID`,
  `strategies/engine/sinks.rs:41`). Simplest: make it return `"generic"` (or accept
  any seg and query with `"generic"`), so `find_positions_by_strategy` still works.
  Remove `use ...registry::StrategyImpl` (:19).
- `swing_legs` field (:82-86) + its `From` decode (:110-113,144) + the
  `use ...swing_1::swing::SwingLeg` (:20): **remove** (swing_1 dies in commit 5).
- `close_position` (:587): `app_state.strategy.close_position(position_id)` →
  `app_state.engine.manual_close(position_id)` (returns bool; same Ok(true/false) shape).
- `get_armed_history_by_rule` (:529-547): **delete the handler** + its route
  (`api/mod.rs:143-144`) + the FE panel (`ArmedHistoryPanel.tsx` — check callers).

**C. `api/handlers/trading/solana.rs`** (:461-485, real-money manual-sell reconcile)
- Replace the `reconcile_externally_cleared_mint(mint, strategy.repo(), trade_repo,
  strategy.runtime(), trader)` call with: look up the open **real** `Holding` position
  for the mint via `app_state.strategy_repo.find_holding_by_mint("generic", mint, ..)`
  (or a suitable repo query filtered to mode='real'), then `engine.manual_close(id)`.
  Drop `use crate::strategies::execution::real::reconcile_externally_cleared_mint`.
  **Preserve the retry loop semantics** (retry until closed or none-open).
  ⚠️ Real-money path — verify double-sell safety with a paper+real smoke.

**D. `api/handlers/strategies/rules.rs`** (legacy rule CRUD) — **DELETE the whole file**.
  The generic `/strategy-rules/*` routes (`handlers::strategies::engine::*`) already
  cover create/get/update/delete/activate/pause/stop + pause-all/stop-all.
- Remove `pub mod rules;` from `api/handlers/strategies/mod.rs`.
- Delete the legacy routes `api/mod.rs:80-141` (the `/strategies/{strategy}/rules/*`
  block → `rules::*`). **Keep** the `/strategies/{strategy}/positions*` routes
  (→ `positions::*`) except armed-history. Fix the module doc (:10, :80).

**E. Delete legacy modules**
- `strategies/service.rs`, `strategies/runner.rs`, whole `strategies/execution/` dir.
- `strategies/mod.rs`: drop `pub mod execution/runner/service;` +
  `pub use runner::StrategyRunner;` + `pub use service::{PaperActivation, StrategyService};`.

**F. `main.rs`**
- Delete the legacy `strategy_cache` block (:632-644: `StrategyRuntimeCache::new()`,
  `set_sse_sender`, `load_from_db`).
- Delete the `StrategyService::new(...)` construction (:738-746) + the doc above it.
- Delete the `strategy_cache.clone()` reaper at ~:837 (`let sc = strategy_cache.clone();`
  — read the surrounding block; it's the legacy cache reaper, safe to drop since the
  engine owns positions).
- `DeployState::new(...)` call (:782-794): replace the `strategy_service.clone()` arg
  (:785) with a fresh `StrategyRepo::new(db.clone())` for the new `strategy_repo` field.

**Compile:** `cargo check -p hunter-live --target-dir "C:/Users/User/Documents/Bot/target-check"`.
Expect fallout — fix each. Core still holds `StrategyImpl`/`StrategyRuntimeCache` at
this point, so live compiles against them until commit 5.

### Commit 5 — core: delete the legacy strategy domain

Once live + lab no longer reference them, delete from `hunter/core/src/`:
- `strategies/swing_1/`, `strategies/tpsl_sniper_1/`, `strategies/tpsl_sniper_2/` dirs
  (+ `strategies/mod.rs` `pub mod` lines).
- `strategies/registry.rs` (`StrategyImpl`, `Tpsl1Params/Tpsl2Params/Swing1Params`).
- `strategies/runtime_cache.rs` + `strategies/exit_state.rs` (legacy cache + exit state).
- `strategies/match_keys.rs` (both `fingerprint_key` + `sim_key` take `LegacyStrategyRule`).
- `strategies/rules.rs`: **bisect at the `// LEGACY` banner (~:189)** — keep :1-187
  (generic `RuleDraft`/`build_rule`/`create`/`save`), delete the legacy half
  (`LegacyRuleDraft`, `validate_tpsl1/2`, `build_legacy_rule`, `create_legacy`,
  `save_legacy`, legacy tests). Trim the `use` on :21/:25.
- `models/strategy.rs`: `LegacyStrategyRule` (:47) + `models/{tpsl1,tpsl2,swing1}_strategy_rule.rs`
  (`Tpsl1Rule`/`Tpsl2Rule`/`Swing1Rule`) + their `models/mod.rs` re-exports.
- `storage/repositories/strategy_repo.rs`: legacy methods (`LegacyStrategyRuleDbRow`,
  `insert_rule`/`update_rule`/`find_rule`/`find_rules_by_strategy`/`find_active_rules`/
  `delete_rule`) + drop the legacy `strategy_rules_legacy` JOIN branch in the kept
  position/name queries (surgical, ~:1535 and ~:1639 — do NOT delete those whole queries).
- `strategies/mod.rs` re-exports of the above.

**Compile:** `cargo check -p hunter-live -p hunter-lab` clean; run
`cargo test -p hunter-core -p hunter-lab`. Grep-sweep for `tpsl`, `swing_1`,
`StrategyImpl`, `LadderParams`, `exit_state`, `LegacyStrategyRule`.

### Phase 7.2 / 7.3 / 7.4 (from the master plan)

- **7.2 migration** — drop `strategy_positions.strategy_id` (the engine stamps a
  now-meaningless `"generic"`; positions.rs no longer filters on it after commit 4);
  decide fate of `strategy_rules_legacy` + old per-strategy sweep tables
  (`{tpsl1,tpsl2,swing_1}_grouped_sweep_*`) — keep read-only or drop.
- **7.3 docs** — rewrite `docs/arch/strategies.md` around the generic engine; update
  `arch/sweep.md` (registry is generic-only now), `arch/database.md`, `arch/architecture.md`;
  hunter/CLAUDE.md crate table; new `docs/plans/strategy-redesign/metrics-reference.md`.
  Also tidy the now-stale `sweep/registry.rs` module doc header (still says "adding
  swing later means…").
- **7.4** — full `cargo test` across all four crates; clippy on touched code;
  **paper runtime smoke** (esp. the rewired manual-sell reconcile) + a real-money
  smoke before trusting the reconcile change.

## Gotchas learned this pass

- Build with `--target-dir "C:/Users/User/Documents/Bot/target-check"` (a live .exe
  may lock `target/`). Absolute path, forward slashes.
- **Concurrency:** another session was editing `hunter/docs/plans/sweep/**` during this
  work. Commit backend with **pathspec** (`git commit -m .. -- hunter/lab`) so unrelated
  staged changes don't ride along. Check `git diff --cached --stat` before every commit.
- `sweep/registry.rs` tests are all RAM-sizing (generic) — keep them; only the
  `sweep_*`/`simulate_*_one_combo` fns + `Noop`/`fill_outcomes` import were legacy.
- Survivors that had to be relocated out of the deletion set (already done):
  `snipe_reserves_from_cache`→engine, `rows_to_json`→engine_sim, `ChainStats`→swing.rs,
  `MatchedTokenResult`/`matched_page_response`→live... no, that was lab engine.rs.
- `engine.manual_close(pg_position_id: Uuid) -> bool` and `close_rule(rule_id)` exist
  on `EngineHandle` (`strategies/engine/mod.rs:81,90`). `ArmedRegistry::snapshot()` only.
