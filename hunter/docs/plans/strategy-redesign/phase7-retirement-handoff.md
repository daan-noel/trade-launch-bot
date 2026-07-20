# Phase 7 — legacy-strategy retirement: handoff

Status snapshot for continuing the tpsl1/tpsl2/swing1 retirement in a fresh
session. Branch **`chore/retire-legacy-strategies`** (off `strategy-redesign`),
**not pushed**.

## Progress (updated)

- **Commit 4 (`73f79180`) — DONE.** Live `StrategyService` retired; closes route
  through the generic engine. Added a pure-engine `Event::ExternallyCleared
  { position, fill }` (closes Entered→Closed as Manual WITHOUT a SubmitSell) +
  `EngineCommand::ReconcileCleared` / `EngineHandle::reconcile_cleared`; the
  manual wallet-sell reconcile (`solana.rs`) confirms the bag cleared via the feed
  then drives each open real position through it. Deleted
  `strategies/{service,runner}.rs` + `strategies/execution/`, the legacy
  `rules.rs` handler + its routes, the `swing_legs` wire field + armed-history.
- **Commit 5 (`07592d19`) — DONE.** Legacy strategy DOMAIN deleted from
  `trading_core` (registry, `runtime_cache`, `exit_state`, `match_keys`,
  `swing_1/`, `tpsl_sniper_{1,2}/`, the per-strategy rule models, `LegacyStrategyRule`,
  the legacy half of `rules.rs`, the `strategy_rules_legacy` repo methods + JOIN).
  Re-pointed the two remaining legacy-table readers onto generic `strategy_rules`
  (RuleRepo): live positions-by-rule `find_rule`→`rule_repo.find`, portfolio
  `find_active_rules`→`rule_repo.list_active`; deleted lab's dead per-strategy
  paper-position handlers.

Verified: `cargo check` core+live+lab clean; clippy no new warnings; hunter-engine
107 tests (incl. new `externally_cleared_closes_without_sell` golden), live 14,
lab 143, core 121 pass. **One pre-existing failure** — `hunter-core`
`generic_params_registry_checked` (engine `RuleParams` contradiction detection for
`time >30 AND <10`); fails identically at `73f79180`, unrelated to Phase 7.

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

Commits 4 + 5 are landed (see Progress above); the code is fully off the legacy
stack. What's left is the DB/docs/smoke tail:

### Phase 7.2 / 7.3 / 7.4

- **7.2 migration — DONE (`817ba47f`).** core `0005` drops `strategy_rules_legacy`;
  lab `0005` drops the 12 `{tpsl1,tpsl2,swing_1}_grouped_sweep_*` tables. Applied on
  the next lab/live boot (not yet run against a DB — `DROP TABLE IF EXISTS`, idempotent).
  **Deliberately NOT dropped:** `strategy_positions.strategy_id` / `strategy_runs.strategy_id`
  (harmless `'generic'` sentinel woven through the hot models — a model refactor, not a
  schema tidy; left as an optional follow-up).
- **7.3 docs — DONE (`95e32a2` + `39249344`).** Rewrote `docs/arch/strategies.md` around
  the generic engine; updated `arch/architecture.md` (+ a `hunter-engine` crate row),
  `arch/sweep.md`, `arch/database.md`, `hunter/CLAUDE.md`, and the `sweep/registry.rs`
  doc header. **Still open (optional):** a new `docs/plans/strategy-redesign/metrics-reference.md`.
- **7.4 — PENDING (needs the live stack / user).** Full `cargo test` + clippy are green
  (1 pre-existing engine `RuleParams` failure, unrelated). What remains is a **paper +
  real-money runtime smoke** of the rewired manual-sell reconcile (`solana.rs` → the new
  `ExternallyCleared` engine path): confirm a manually-sold held real position books
  `End`/`Manual` cleanly (no phantom re-sell, no stuck `Holding`). This is the only
  real-money behavior change in Phase 7.

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
