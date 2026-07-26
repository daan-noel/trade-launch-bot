# Sweep vs simulate divergence — root causes + entry-cache fix plan

Investigation date **2026-07-26**. Status: **diagnosed, fix approved, not yet implemented.**
Companion ledger: [../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md) (update it when
the fix lands). Evidence run: grouped-sweep `593844a2` (fp-scoped, 248 tokens, buy 0.01,
first_in_window + fee_only), promoted rule `promoted g0 c655362` (`1e143b5a`).

## Symptom

The grouped-sweep best-combo row and a simulate of the very same promoted rule disagreed
wildly and repeatedly (sign flips, win% 36 vs 3, fired 55 vs 100), surviving every retry
the user made (same params, same fill/cost, re-exported lake).

## Root causes found (three, layered — all real)

### 1. Notional mismatch x fixed per-leg cost (first comparison)

- Sweep default `buy_amount_sol = 1.0` (`registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL`, FE form
  default); the promoted rule traded at 0.01 SOL.
- Every cost model except `frictionless` charges `fixed_cost_sol_per_leg` =
  `JITO_MIN_TIP_SOL` (env: 0.001) + avg CU priority fee (~0.000025) ~= 0.001025 SOL/leg,
  ~0.00205 SOL per round trip — **0.2% of notional at 1.0 SOL, 20.5% at 0.01 SOL**.
  `pumpfun_fee_only` only drops `slippage_bps`, never the fixed cost.
- Breakeven gross move: ~+2.2% (sweep at 1.0) vs ~+22.6% (sim at 0.01) -> win% collapse +
  sign flip. Documented as residual 4b in sim-parity.md; nothing in the UI warns.
- Status: understood; behavioral fix = sweep at the intended live notional. Optional UI
  follow-up F2 below.

### 2. Comparison-context mismatches (second comparison)

- **Sim pricing defaults**: `SimulatePage` resets to `worst_case` + `pumpfun_default` on
  every page load; a rule simulated right after promote silently ran under different
  fill/cost than the sweep (`EngineSimRequest` serde defaults are the same pair). The UI
  already paints the chips red but does not inherit the sweep run's pricing.
- **Stale lake**: sweep corpus is LakeSource-only (last export 05:01Z that day); simulate
  splices the PG fresh tail (`sim_fetch::pg_tail_beyond_lake`, trades to 09:33Z). The
  sweep froze 82 positions as `Open (est)` at 5-hour-old prices; the sim watched those
  same tokens die (85 Dead exits, 0 open). Also sim's candidate scan (PG `tokens`, 250
  matched) vs the run's frozen corpus (248).
- Status: workflow fix = re-export lake before sweeping (user now does). Optional UI
  follow-ups F1/F2 below.

### 3. THE BUG - entry-cache poisoning across exit variants (remaining gap)

**Mechanism.** `resolve_entry` is exit-dependent: it mirrors the engine's `can_enter`
veto — *never enter while exit conditions already hold*
(`sweep/generic/strategy.rs`, veto inside the entry walk). But the fold's single-slot
entry cache (`sweep/engine.rs::fill_outcomes_with_state`) is keyed by
`AxesModel::entry_key` = **entry-axis picks only**, and `order_for_entry_cache` makes
same-entry combos contiguous. So the **first combo of each entry class resolves entries
under ITS OWN exit veto and every sibling silently inherits that entered set** — wrong
count, wrong entry rows, wrong prices.

**Proof (run `593844a2`, all reproducible from local DB + drill-in API):**

- Combos 655360..655379 share identical entry params; all 20 stored rows say `n_fired = 55`.
- 655360 (first in class) exits on `buy(30s)<3 | buy(60s)<1 | trail(5s)>25 | liq>85`.
  The quiet-flow exits hold exactly when the entry's own `gross_flow(3s)<10` holds, so
  its veto kills most candidates: 55 is *its* honest number.
- 655362 (the promoted combo) exits only on `trail(5s)>30 | liq>85` — the veto almost
  never fires. Fresh drill-in (`token-results` endpoint, same corpus/as_of/pricing):
  **101 entered**, agreeing with the independent engine simulate (100 of its 250 set).
- Control drills of first-in-class combos reproduce their stored rows exactly
  (589824: 93<->93; 655360: 55<->55). Aggregate-vs-own-drill-in disagreement is the
  bug's signature.
- True numbers for the promoted combo under the run's own pricing: 101 fired / 19 closed /
  82 open, realized +0.219, open MTM -0.256 — vs stored (poisoned) 55 / 14 / 41,
  +0.246 / -0.132. The best-combo crown was won on poisoned numbers.

**Blast radius.** Every grouped sweep with exit-side metric axes: any combo not first in
its entry class may carry a wrong entered set -> scores, crowning, promote, and the
combos table are unreliable. Existing stored runs stay poisoned after the code fix —
re-run and expect the crown to move. The guard suite never covered two same-entry /
different-exit combos through the *fold* (all guards test single rules), which is why
this survived.

## Fix design (approved): cache candidates, re-veto per combo

Naive fixes rejected:

- *Key the cache by (entry, exit) picks*: re-runs the full O(n) entry walk per combo; on
  TP/SL sweeps with O(log n) indexed exits, entry would then dominate the whole sweep.
- *Drop the cache*: same, worse.

Two observations make a near-free fix possible:

1. **The veto depends only on token-scoped exit reqs.** Position-scoped exit reqs
   (`pnl`/`held`/`retrace`/`bounce` — every desugared TP/SL) read `NaN` pre-entry
   (`MISSING_COL`) and can never veto. Pure-TP/SL sweeps (the heavy 1M-combo case) are
   *already correct* today; the fix must not tax them.
2. **Everything expensive in the entry walk is exit-independent** (dead check, mono-kill,
   entry-conds eval). Only the veto — evaluated at rows where entry conds already hold —
   reads the exit side, and for selective entries those rows are few.

### Stage A — `EntryCandidates`, cached once per entry class (same key, same cost as today)

One walk over the series producing:

- `kill_row`: first row where `dead[i]` or a mono-kill fires (exit-independent), else `n`.
- `cands`: rows `< kill_row` where entry conditions hold. For `enter_on_arm` rules this is
  an implicit range `[0, kill_row)`, never materialized.
- Reused per-worker buffer (`Vec<u32>`, few KB); nothing allocated per combo.

### Stage B — per combo, cheap

Walk `cands` in order; at each row evaluate only the combo's **token-scoped** exit reqs
(the veto, same `reqs_any_satisfied` eval); first non-vetoed row is the entry row; then
resolve the fill. Two memos keep the common case at today's speed:

- **fill memo** keyed by admissible row: combos landing on the same entry row (the
  overwhelmingly common case) share one `find_paper_entry_at` call;
- **exit-ctx rebuild only when `fill_row` changed** (currently rebuilt on cache-stale) —
  the `ExitIndex` hulls are fill_row-dependent, so this is both a correctness requirement
  under per-combo entries and a saving.

The existing `resolve_entry` stays untouched as the SSOT reference; the drill-in /
`simulate_one_combo` / guard `scan` paths already use it and are already correct.

### Expected performance

| Sweep shape | Today | After fix |
| --- | --- | --- |
| Pure TP/SL exits (fast path, indexed/SIMD) | correct + fast | bit-identical work: veto vacuous, Stage B = first candidate + fill-memo hit |
| Token-scoped exit axes (the broken case) | fast but wrong | Stage A shared as today; Stage B adds veto evals over ~1-few rows/combo — well under 5% total, dwarfed by the O(n) scalar exit walk these rules already pay |
| Worst case (loose entry + token-scoped exits) | wrong | Stage B degrades to <=1 extra O(n) pass/combo doing strictly less per-row work than the walk it replaces; bounded < 2x |

## Implementation steps

1. `hunter/lab/src/sweep/generic/strategy.rs`
   - Extract the exit-independent walk into `entry_candidates(series, bound) -> EntryCandidates`.
   - Add `resolve_entry_from(cands, trades, series, bound, pricing, &mut fill_memo)`
     applying veto + fill; `debug_assert!` it equals a fresh `resolve_entry` (debug only).
   - Expose whether a `BoundCombo` has any token-scoped exit req (vacuous-veto fast path).
2. `hunter/lab/src/sweep/strategy.rs` (trait) + `hunter/lab/src/sweep/engine.rs` (fold)
   - Entry cache slot becomes `(EntryKey, EntryCandidates-ctx)`; per combo the fold calls
     the Stage-B resolver; rebuild `ExitCtx` only when the resolved `fill_row` differs
     from the previous combo's.
   - Keep `order_for_entry_cache` (key unchanged — candidates are shared per class).
3. `hunter/lab/src/sweep/generic/guard.rs`
   - **The regression test**: two combos sharing an entry class, where combo A's exit
     holds at combo B's first entry row; assert fold (cache path) == per-combo `scan`
     == `run_replay`. This is the test that would have caught the bug.
   - Existing fixtures automatically exercise Stage A+B == reference `resolve_entry`.
4. Docs + bookkeeping
   - `docs/plans/sweep/sim-parity.md`: move this from "open" to "fixed", with the
     mechanism + guard names.
   - Delete this roadmap file once landed (docs discipline: roadmap holds WIP only).
5. Definition of done: `cargo check -p hunter-lab` clean, `cargo test -p hunter-lab`
   green (guards included), clippy on touched code, no new warnings.

## Follow-ups (optional, not in the fix PR)

- **F1 — lake staleness surface**: show "data through HH:MM" on sweep runs (corpus max
  block_time), so frozen-tail `est` rows are not mistaken for live opens.
- **F2 — pricing/notional inheritance**: simulate launched for a promoted rule inherits
  the source run's `fill_model`/`cost_model` (and warns when the rule's `buy_amount`
  differs from the run's); SimulatePage stops silently resetting to
  `worst_case`/`pumpfun_default`.
- **F3 — re-run + re-crown**: after the fix, re-run the affected sweeps; expect rankings
  to change — that is the fix working, not a regression.

## Verification cheatsheet (how this was proven, for future audits)

- Stored run config: `grouped_sweep_runs` (pricing, corpus window, fingerprint scope).
- Per-combo stored aggregates vs params: `grouped_sweep_results` join `grouped_sweep_combos`.
- Honest re-sim of one combo: `GET /api/strategies/sweeps/{run}/groups/{gid}/token-results?strategy_id=generic&combo_id=N`
  (threads the run's own pricing + as_of + corpus; no entry cache).
- Sim ground truth: `$SWEEP_LAKE_DIR/sim-results/{rule_id}.{meta,rows}.json`.
- Lake freshness: `$SWEEP_LAKE_DIR/trades/dt=*/_meta.json` `exported_at_utc` vs PG `max(trades.block_time)`.
