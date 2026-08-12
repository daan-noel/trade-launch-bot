# Rule-search driver - implementation plan

Implements [rule-search-method.md](rule-search-method.md) as an automated hunter-lab
feature: one API call turns a `fingerprint_id` into a shipped-inactive rule plus a
computed `USE / MARGINAL / DO NOT USE` verdict and a persisted decision report. The
method doc owns the *what*; this plan owns the *how* and the build order. The driver
requirements pinned by the g3 pilot (method doc, "Driver requirements" section) are
binding constraints here.

Workstation-only: the driver lives in `hunter-lab`, never ships to the EC2 box.

## Architecture

```
              POST /api/strategies/rule-search
              { fingerprint_id, mode: full | retune, budget_min, seed_rule_id? }
                              |
                              v
 +---------------------------------------------------------------------+
 |  hunter/lab/src/strategies/rule_search/                             |
 |                                                                     |
 |  [0] cohort.rs    scope by fp -> regime check -> K=4 folds          |
 |                   + ~7d holdout   (broken/thin => verdict, exit)    |
 |  [1] menus.rs     cohort-percentile ladders -> candidate pool       |
 |                   (bands = one 2-D candidate; m_position exit-only) |
 |  [2] search.rs    staged entry/exit loop, alternation >= 2 passes,  |
 |                   simulate-confirm on every accepted entry          |
 |  [3] kernel.rs    decision test: marginal > 0 AND >= 3/4 folds AND  |
 |                   luck floor (permute selected set | test excluded) |
 |  [4] rescue.rs    synergy rescue + two seeds (empty / incumbent)    |
 |  [5] tune.rs      fine grid on survivors: plateau AND fold-hold     |
 |  [6] battery.rs   holdout simulate x4 columns x2 pricings -> spread |
 |  [7] verdict.rs   computed verdict; ship inactive; park rejects     |
 +---------------------------------------------------------------------+
        |                     |                        |
        v                     v                        v
  strategy_rules        rule_search_runs          SSE progress
  (is_active=false,     (report JSON: every       (step/round/candidate
   parked evidence in    decision + evidence;      readout, UI page)
   params.disabled)      feeds retune mode)
```

Everything between [2] and [6] runs **in-process against in-RAM per-position
outcomes** - never persisted sweep rows (retention drops fold cells), never the HTTP
result pager (subsamples rows, working set evicts old runs).

## Reuse map - the driver is a new orchestrator over existing engines

| Need | Existing surface | Change required |
| --- | --- | --- |
| scoped corpus, one load | `sweep::corpus::{Corpus, Selection}` + saved-fp scope resolution | none - call directly |
| candidate menus | `discovery::candidates` percentile-menu generation | expose as a callable, decoupled from the discovery pipeline's validate layer (which carries the closed-only-PnL trap) |
| batch rule evaluation | `sweep::grouped_engine::run_grouped_sweep` + `sweep::generic` axes | **Phase 1 seam**: a caller-supplied result sink that receives full per-combo, per-position outcomes in RAM, bypassing `retention.rs` and row persistence |
| authoritative single-rule runs | `strategies::engine_sim::spawn_engine_simulation` | in-process variant returning per-position rows to the caller instead of (only) the RAM working set |
| fold slicing, luck floor, bootstrap | none (pilot used ad-hoc PowerShell) | new, pure functions in `kernel.rs` |
| rule shipping | `POST /api/strategy-rules` handler logic | call the same service fn internally |

## Phases

### Phase 1 - engine seam (the foundation, and the largest piece)

An in-process call: `evaluate(corpus, precompute, rule_params, pricing) ->
Vec<PositionOutcome>` where `PositionOutcome` carries at least `{mint, created_at,
fired, pnl_sol, open_pnl_sol, fill_ratio, exit_reason}`.

- Batch form: thread a `ResultSink` trait through `run_grouped_sweep` so the driver
  receives every combo's outcomes directly; the existing persistence path becomes one
  sink implementation. Retention keeps governing what is *stored*, never what the
  driver *sees*.
- Single-rule form: factor the core of `spawn_engine_simulation` into a callable that
  returns outcomes; the HTTP path wraps it unchanged.
- Corpus + metric precompute allocated once per driver run over the union of every
  metric any step touches; selection byte-identical across the run (corpus cache is
  single-slot - one eviction per run, not per round).
- Done when: a test evaluates one rule both ways (seam vs existing HTTP path) and the
  fired set + net SOL agree exactly.

### Phase 2 - decision kernel (pure logic, golden-tested)

`kernel.rs`: fold assignment from contiguous creation-time edges; the three-part
marginal test; fire floor `max(20, 5% of fitting tokens)`; luck floor switching form
by fire fraction (permutation of selected subsets when k/n is small, excluded-set test
when the gate fires on most tokens); paired bootstrap for exit deltas. All pure
functions over `Vec<PositionOutcome>`.

- Golden tests replay the g3 pilot's recorded decisions from fixture outcomes: the
  `liquidity > 15` phantom must die, `unique_wallets >= 12` alone must fail the joint
  test, the pair must pass under the real exits.

### Phase 3 - search loop

`search.rs` + `rescue.rs`: entry stage under the neutral exit scored on per-trade
edge; exit stage under the winning entry scored on total SOL including the open mark;
relative add-gating (~25% of the round's best marginal); drop probes; alternation
until stable, minimum 2 full passes, cap 3; every accepted entry simulate-confirmed
(fires-on-fraction sanity + marginal reproduced by the authority engine); synergy
rescue over near-misses under the pinned strongest condition; both seeds (empty set,
incumbent rule including its `params.disabled` block), better result kept.

- Budget knob narrows value menus, rescue depth, alternation count - never folds,
  luck floor, or holdout (method doc, performance rules).

### Phase 4 - tune, battery, verdict

`tune.rs`: fine grids over the 3-6 survivors, 2-D band refinement, accept on plateau
AND fold-hold, drop conditions whose adjacent values tie. `battery.rs`: holdout
simulate, four columns (new / incumbent / seed / no-rule) x two pricings (authority
worst+`pumpfun_impact`, optimistic first+`fee_only`) at the live notional;
fill-spread ratio computed. `verdict.rs`: thresholds from the method doc, weakest
check decides; ship via the strategy-rules service with `is_active: false`, paper,
rejected candidates parked in `params.disabled` with their rejecting marginal.

### Phase 5 - persistence, API, UI

- Migration: `rule_search_runs` (id, fingerprint_id, mode, status, verdict, params of
  the shipped rule, report JSONB - every decision with its evidence numbers, per-phase
  timings, data cutoff).
- `POST /api/strategies/rule-search` (start), `GET .../rule-search/{id}` (report),
  SSE progress events (current step / round / candidate under test).
- Retune mode: load the prior report, re-verify the selected set on fresh data,
  re-screen recorded near-misses, local value tuning only. Target: under 5 min.
- Minimal UI page: start form, live progress, report rendering (decision list +
  battery table + verdict), link to the shipped rule.

### Phase 6 - validation gates (in order)

1. **Golden replay on g3**: full mode reproduces the pilot's F2 (same conditions,
   same parks) with matching evidence numbers, inside 30 min wall-clock. Any decision
   divergence is a bug in the port until proven a fix.
2. **Fresh-data champion battery on g3**: after the next EC2 sync, the battery runs
   F2 vs v2 on data neither has seen - the pending champion test rides along free.
3. **One unseen fingerprint end-to-end**: verdict correct by hand-check, wall-clock
   inside target (full 15-30 min, barren 3-5 min).

## Sizing and risks

- Effort skews to Phase 1 (~40%): the `ResultSink` seam touches `run_grouped_sweep`
  and the sweep persistence path, which other callers (grouped sweeps, discovery)
  share - the refactor keeps their behavior byte-identical (existing sweep tests
  gate this).
- Phases 2-4 are mostly pure logic once outcomes are in RAM; Phase 5 is routine
  plumbing.
- RAM: outcomes for ~600 combos x ~500 tokens are small (tens of MB); the corpus and
  metric precompute dominate as today. `SpillDir` stays per-connection (DuckDB shared
  spill-dir trap).
- The discovery validate layer is NOT reused (closed-only-PnL trap); only its
  screen/menu machinery is.
- Definition of done per CLAUDE.md: `cargo check -p hunter-lab` clean, clippy on
  touched code, sweep parity tests green, arch docs updated
  (`docs/arch/sweep.md` gains the sink seam; a new `docs/arch/rule-search.md`
  describes the driver once it exists).
