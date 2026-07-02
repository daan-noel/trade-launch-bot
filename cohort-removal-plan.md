# Plan: Remove all "cohort" logic (launch-cohort entry gate + E5 CohortExit)

## Context

"Cohort" = the **launch cohort**: wallets that bought a token within `EARLY_COHORT_SLOT_WINDOW`
(150) slots of its first trade. It powers two `tpsl2`-only features:

1. **Entry gate** — `p_entry_max_cohort_held`: reject entry if the cohort still holds too much
   supply (used by `tpsl_sniper_2` scalp entry; also a **dead, unimplemented** field on
   `Swing1Params`/`swing1` — never read by `swing_1/` entry logic).
2. **Exit ladder E5** — `p_exit_cohort_ratio`: `ExitReason::CohortExit`, top-priority rung — exit
   when the cohort's net holdings collapse to ≤ ratio% of what it bought.

**Correction to CLAUDE.md/@arch docs:** cohort is **`tpsl_sniper_2`-only**. `tpsl_sniper_1` has no
`cohort` submodule and never did — despite CLAUDE.md's "entry/exit/cohort... intentional clones"
wording implying otherwise.

**Zero cohort references** in `pump-trader`, `ingest-laserstream`, `ingest-websocket`, or
`frontend-react/src/live/`.

Since `CohortExit` is **E5, the highest-numbered/top-priority rung**, removing it needs **no
renumbering** of E1–E4 (TrailingStop/TimeStop/Stall/LiquidityExit) — just deletion.

## Locked decisions

- **DB migrations:** add a **new** migration to `DROP COLUMN n_exit_cohort` (trading_core
  `strategy_run_metrics`, lab `*_grouped_sweep*` tables) rather than editing `0001_init.sql` /
  `0001_grouped_sweep.sql` / `0002_swing1_grouped_sweep.sql` in place.
- **Stored rule JSONB:** existing `strategy_rules.params` rows may still have
  `p_entry_max_cohort_held` / `p_exit_cohort_ratio` keys. Do **not** backfill/strip — just stop
  reading them in code; stale keys are inert once nothing deserializes them into a live field.
- **Exit ladder numbering:** no renumbering needed (E5 was terminal). Ladder becomes
  CohortExit(removed) → LiquidityExit(E4) → StopLoss → TakeProfit → TrailingStop(E1) → Stall(E3) →
  TimeStop(E2), i.e. just delete the top line.

## File-by-file checklist

### `trading_core`

- [ ] **Delete** `strategies/tpsl_sniper_2/cohort.rs` (whole file: `early_cohort_wallets`,
      `CohortFlow`, `held_ratio`, `cohort_flow`, `outside_net_sol` + its tests).
- [ ] `strategies/tpsl_sniper_2/mod.rs` — drop `pub mod cohort;`.
- [ ] `strategies/tpsl_sniper_2/entry/mod.rs` — drop `scalp_cohort`/`find_scalp_entry_with_cohort*`
      re-exports.
- [ ] `strategies/tpsl_sniper_2/entry/scalp.rs` — remove `scalp_cohort`,
      `find_scalp_entry_with_cohort_indexed` (and non-indexed variant), the
      `p_entry_max_cohort_held` gate check, cohort-vs-outside SOL/liq split helpers; keep the
      organic (non-cohort) gates. Remove associated tests.
- [ ] `strategies/tpsl_sniper_2/exit/mod.rs` — remove `ExitReason::CohortExit` variant + its
      `as_str()` arm; remove `CohortMemo`, the `cohort` field on `CachedExitState`,
      `ensure_cohort_seeded`, cohort seeding in `build_unfolded`, cohort folding in
      `advance_and_find_exit`; remove `cohort_exit_ratio` from `LadderParams` +
      `LadderParams::from_rule`; remove the E5 `.or_else` arm + `cohort` param from
      `ladder_reason`; remove `find_trade_driven_exit_with_cohort` + the cohort precompute branch
      in `find_trade_driven_exit_with_slot`/`run_exit_walk`. Remove the `cohort` param from
      `run_exit_walk`. Remove all `// E5 · ...` cohort tests (`cohort_exit_fires_when_cohort_dumps`,
      `cohort_exit_does_not_fire_while_cohort_holds`, `with_cohort_matches_inline_cohort_exit`,
      `cohort_exit_inert_when_unconfigured`, `rule_cohort` helper, `trade_w` helper if unused
      elsewhere). Update the module doc-comment ladder-priority line (drop `CohortExit →`).
- [ ] `strategies/registry.rs` — remove `p_entry_max_cohort_held` handling for both `Tpsl2Params`
      and `Swing1Params`; remove the "build `scalp_cohort` once" comment/logic in
      `resolve_entry`; remove internal-cohort references in `resolve_exit`.
- [ ] `strategies/exit_state.rs` — remove `ensure_cohort_seeded` wrapper call.
- [ ] `strategies/kernel.rs` — remove `ExitCode::CohortExit` + `n_exit_cohort` counter (index 6);
      shift/handle any positional encoding that assumed its slot.
- [ ] `strategies/rules.rs` — remove "Cohort Exit Ratio %" / "Max Cohort Held %" summary strings.
- [ ] `strategies/mod.rs` — update doc comments listing cohort as a tpsl2 feature.
- [ ] `config/constants/tuning.rs` — remove `EARLY_COHORT_SLOT_WINDOW`.
- [ ] `models/tpsl2_strategy_rule.rs` — remove `p_entry_max_cohort_held`, `p_exit_cohort_ratio`
      fields (+ constructor/serde wiring).
- [ ] `models/swing1_strategy_rule.rs` — remove dead `p_entry_max_cohort_held` field.
- [ ] `models/strategy.rs` — remove `n_exit_cohort` metrics field.
- [ ] `models/grouped_sweep.rs` — remove `n_exit_cohort` + `"CohortExit"` from exit-reason list.
- [ ] `state/token_cache.rs` — remove now-unneeded cohort-set comments (the `TradeRow`/`u32`
      wallet plumbing stays — it's used generically, not cohort-specific — verify nothing else
      depended solely on cohort before touching).
- [ ] `storage/repositories/strategy_repo.rs` — remove `n_exit_cohort` read/write.
- [ ] **New migration** `trading_core/migrations/000X_drop_cohort_metrics.sql` —
      `ALTER TABLE strategy_run_metrics DROP COLUMN n_exit_cohort;`

### `live`

- [ ] `strategies/execution/paper.rs` — remove `scalp_cohort` + `find_scalp_entry_with_cohort_indexed`
      calls from the worst-case entry path; fall back to the non-cohort entry resolver.
- [ ] `strategies/service.rs` — update comment referencing E5 cohort memo.
- [ ] `strategies/mod.rs` — update comment referencing `cohort` decision module.

### `lab`

- [ ] `strategies/tpsl_sniper_2/mod.rs` — drop `cohort` from the `pub use trading_core::...` shim.
- [ ] `strategies/mod.rs` — update comment.
- [ ] `sweep/strategies/tpsl2.rs` — remove `Tpsl2TokenState` cohort fields, `prepare_token` cohort
      precompute, `cohort_bought`, `sweeps_cohort_exit`, cohort axis entries, rule-mapping for
      `p_entry_max_cohort_held`/`p_exit_cohort_ratio`, `for_replay` cohort handling, and JSON key
      output for cohort params.
- [ ] `sweep/strategies/swing1.rs` — remove the dead `entry_max_cohort_held` axis + rule mapping
      to `p_entry_max_cohort_held`.
- [ ] `sweep/engine.rs`, `sweep/strategy.rs` — remove cohort-related doc comments on
      `prepare_token`.
- [ ] `sweep/projection.rs`, `sweep/corpus.rs` — remove cohort-specific doc comments (keep the
      `u32` wallet interning itself if used elsewhere; verify).
- [ ] `sweep/registry.rs` — remove `CohortExit` mapping + `for_replay(has_cohort)` detection tied
      to `exit_cohort_ratio`.
- [ ] `sweep/aggregate.rs`, `sweep/retention.rs`, `sweep/grouped_engine.rs` — remove
      `n_exit_cohort` fields/defaults.
- [ ] `api/handlers/strategies/tpsl2.rs` — remove legacy `"CohortDump"` string mapping + any
      `CohortExit` handling in sim result mapping.
- [ ] `api/handlers/strategies/grouped_sweep.rs` — remove `n_exit_cohort` sort key + DTO field.
- [ ] `api/handlers/tokens/swing1_detect.rs` — remove `entry_max_cohort_held` request field →
      `p_entry_max_cohort_held` mapping.
- [ ] `storage/repositories/grouped_sweep_repo.rs` — remove `n_exit_cohort` CRUD.
- [ ] `swing_probe.rs` — remove the `p_entry_max_cohort_held` fixture clear (field gone).
- [ ] **New migration** `lab/migrations/000X_drop_cohort_metrics.sql` —
      `ALTER TABLE tpsl2_grouped_sweep_runs DROP COLUMN n_exit_cohort;` (and the swing1 grouped
      sweep table(s) from `0002_swing1_grouped_sweep.sql` — confirm exact table names before
      writing).

### `frontend-react`

- [ ] `shared/types/index.ts` — remove `p_entry_max_cohort_held`, `p_exit_cohort_ratio` from
      `RuleRecord`; remove `CohortExit` from exit-reason doc/union.
- [ ] `shared/lib/params/specs/tpsl2.ts` — remove both param specs + section hint.
- [ ] `shared/lib/params/specs/swing1.ts` — remove dead `p_entry_max_cohort_held` spec.
- [ ] `shared/lib/tpslParamHelp.ts` — remove cohort help text entries.
- [ ] `shared/lib/ruleColorGroups.ts` — remove `p_entry_max_cohort_held` color key.
- [ ] `shared/lib/swing1Axes.ts` — remove `entry_max_cohort_held` axis + description.
- [ ] `shared/components/tpsl2/ruleColumns.tsx` — remove "Max Cohort"/"Cohort" columns.
- [ ] `shared/components/tpsl2/tableColumns.tsx` — remove `CohortExit` badge rendering.
- [ ] `shared/components/tpsl2/SimSummaryCard.tsx` — remove `CohortExit` summary bucket.
- [ ] `lab/components/sweep/groupedTypes.ts` — remove tpsl2 `cohort_ratio`/`entry_max_cohort_held`
      axes + `CohortExit` from exit-reason union.
- [ ] `lab/components/sweep/sweepColumns.tsx` — remove `n_exit_cohort` ("Cohort") column.
- [ ] `lab/components/sweep/types.ts` — remove `n_exit_cohort` field.
- [ ] `lab/components/sweep/groupColumns.tsx` — remove cohort param grouped-column keys.
- [ ] `lab/pages/strategies/sweep/Tpsl2GroupedSweepPage.tsx` — remove cohort keys from swept-param
      list.
- [ ] `lab/pages/strategies/sweep/Swing1GroupedSweepPage.tsx` — remove `entry_max_cohort_held`.
- [ ] `lab/pages/analysis/Swing1DetectPage.tsx` — remove `entry_max_cohort_held` default field.
- [ ] `lab/services/swing1Detect.ts` — remove `entry_max_cohort_held` from request type.

### Scripts

- [ ] `scripts/db-incremental-sync.ps1` — remove `n_exit_cohort` from the metrics UPSERT.

### Docs

- [ ] `CLAUDE.md` — fix the "entry/exit/cohort" wording to reflect cohort is tpsl2-only (or drop
      the mention entirely once the feature is gone).
- [ ] `@arch/strategies.md` — remove cohort.rs description, scalp cohort gates, E5 section.
- [ ] `@arch/architecture.md` — remove `cohort` from the domain-layer module list.
- [ ] `@arch/sweep.md` — remove the tpsl2 launch-cohort `prepare_token` precompute description.
- [ ] `@plans/tpsl-strategy/tpsl2-entry-exit-params.md` — remove the cohort section (lines ~64-316)
      or mark as historical/removed.
- [ ] `@plans/database/strategy-storage.md` — remove `cohort_ratio` JSON example, `n_exit_cohort`,
      `CohortExit` reason references.
- [ ] `@plans/database/trades-storage.md` — remove the cohort-related note.
- [ ] `@plans/sweep/sweep-engine-detail.md` — remove the planned (never-implemented)
      `creator_wallet_cohort: Option<CreatorCohort>` note and the tpsl2 cohort precompute note.
- [ ] `@plans/tpsl-strategy/pumpfun-sniper-strategy-research.md` — remove/annotate the launch-slot
      cohort research note as no longer implemented.
- [ ] `PROJECT-OVERVIEW.md` — remove cohort mentions in the tpsl2 superset description.
- [ ] `live-lab-remake-plan.md` — remove `cohort` from the tpsl_sniper_2 module list.
- [ ] `swing1-plan.md` — remove the "de-prioritize E5 cohort exit" / optional
      `p_entry_max_cohort_held` guard notes (already dead — just drop the reference).

## Verification

1. `cargo check -p trading_core`, `cargo check -p live`, `cargo check -p lab` — clean, no
   `cohort`/`Cohort` symbol errors.
2. `Grep -i cohort` across the whole repo (excluding `target/`, this plan file, and git history) —
   zero remaining hits.
3. `cargo test -p trading_core`, `cargo test -p live`, `cargo test -p lab` — green (cohort tests
   removed, no orphaned references).
4. `npm run build` (frontend-react) — clean typecheck, both live+lab trees.
5. Run the two new DROP COLUMN migrations against a scratch/dev DB; confirm `strategy_repo.rs` and
   `grouped_sweep_repo.rs` no longer reference the dropped columns.
6. Manually spot-check the tpsl2 rule editor UI and grouped-sweep pages no longer show cohort
   params/columns.

## Execution order (to keep the build green at each step)

1. `trading_core` core logic + models + migration (compiles standalone).
2. `live` (depends on trading_core).
3. `lab` (depends on trading_core) + its migration.
4. `frontend-react` (depends on backend types only loosely — TS, not generated).
5. `scripts/`.
6. Docs pass.
7. Full verification sweep (step above).
