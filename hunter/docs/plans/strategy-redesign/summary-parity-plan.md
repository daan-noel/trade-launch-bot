# Summary parity — simulate ≡ grouped sweep ≡ live (real/paper)

Goal: **one logic path and one summary shape** across the three surfaces that
report "how did this rule do": single-rule simulate, grouped sweep, and a
live/paper run.

## The SSOT already exists

`trading_core::strategies::kernel` is the canonical aggregate:

- `TokenOutcome` — per-token result (fired / holding_secs / pnl_percent / pnl_sol / exit)
- `RunAgg` — streaming fold, O(1) memory, DDSketch quantiles (unbounded input)
- `exact_run_metrics` — exact quantiles for bounded input
- `RunMetrics` — the 25-field rolled-up shape, **realized-only** with `open_pnl_sol` carried separately

The sweep already folds through it (`ComboAgg` is a thin wrapper). Simulate and
live each compute their own summary instead. That is the whole bug.

## Divergences to close

### Backend

| # | Divergence | Site | Fix |
| --- | --- | --- | --- |
| B1 | simulate `total_pnl_sol` includes **open** marks (mark-to-market); sweep is realized-only | `lab/src/strategies/sim_query.rs:105` | rebuild `summarize` on `exact_run_metrics` |
| B2 | simulate `avg_pnl_pct` averages over all rows incl. open | `sim_query.rs:101-104` | ditto (closed-only `mean_pnl_pct`) |
| B3 | simulate "closed" = `exit_time != null`; sweep = `ExitCode != Open` | `sim_query.rs:94` | use `ExitCode::from_reason(exit_reason)` |
| B4 | simulate lacks expectancy / profit factor / median / p90 / std / score / holding / exit counts | `SimRollup`, `SimSummary` | replace both with `RunMetrics` |
| B5 | sweep hardcodes `max_concurrent_tokens: u32::MAX, max_total_tokens: 0` | `lab/src/sweep/generic/strategy.rs:105-107`, `registry.rs:1170-1171` | thread the rule's real caps |
| B6 | sweep defaults `buy_amount_sol = 1.0`; PnL% is not notional-invariant (fixed per-leg cost) | `lab/src/sweep/registry.rs:47` | thread the rule's `buy_amount_sol` |
| B7 | `as_of` captured 3× (`Utc::now()` at sim start, sweep start, drill-in) | `engine_sim.rs:281`, `registry.rs:640`, `registry.rs:1177` | capture once, persist on the run row, drill-in reuses |
| B8 | tail-tick cap is corpus-wide in simulate, per-token in sweep | `replay.rs:304-315` vs `generic/strategy.rs:394-395` | align on per-token |
| B9 | sweep corpus has no `!is_mayhem_mode` filter; simulate does | `grouped_sweep.rs:478-493` | add filter |
| B10 | sweep `token_cap` (10k) truncates silently | `grouped_sweep.rs:138-140` | surface as a run warning |
| B11 | live `PositionsSummary` is a 4th shape, no `open_pnl_sol` | `core/src/models/strategy.rs:133`, `strategy_repo.rs:1255-1360` | mark open positions at cached price, emit `RunMetrics` shape |

### Frontend

Three renderers, two DTOs. `SummaryStatsPanel` is a layout shell only — callers
pass pre-formatted strings, so nothing enforces consistency.

| # | Divergence | Site |
| --- | --- | --- |
| F1 | sim summary = 5 tiles, no Realized/MTM bands, no open PnL | `lab/pages/strategies/SimulatePage.tsx:477-509` |
| F2 | sweep summary = hero + 3 bands ≈ 25 tiles | `lab/pages/strategies/sweep/GenericSweepView.tsx:195-301` |
| F3 | live summary = 3rd shape (`total_entry_sol`, `total_holding_sol`, …) | `shared/components/strategy/SimSummaryCard.tsx:21` |
| F4 | PnL: 3dp/◎-suffix/unsigned (sim) vs 4dp/◎-prefix/signed (sweep) | `cellFormat.ts` vs `genericSweepColumns.tsx:27` |
| F5 | win rate 1dp (sim) vs 0dp (sweep); labels differ | `SimulatePage.tsx:487` vs `GenericSweepView.tsx:230` |
| F6 | `dashF`/`dashPercent` render an exact **0 as `'-'`** — a real 0% win rate shows as a dash | `cellFormat.ts:11-21` |
| F7 | dash glyph `'-'` (sim) vs `'—'` (sweep) | as above |
| F8 | colors: hand-rolled green/red (sim) vs `goodBad()` (sweep) vs `text-primary` (live) | three conventions |

## Status — 2026-07-19

**Done.** One aggregate (`kernel::run_summary`), one wire shape (`RunSummary`),
one renderer (`lib/strategy/runSummary`), across simulate + grouped sweep +
live/paper.

| Item | Outcome |
| --- | --- |
| B1-B4 | `sim_query::summarize` delegates to the kernel; `SimRollup`/five-scalar `SimSummary` deleted |
| B3+ | **Extra bug found:** "closed" keyed off `exit_time != null` misfiled every death-close (`ExitCode::Dead` carries no exit time) as *open*. Now keyed off `exit_reason`, matching the sweep |
| B4 | `RunSummary { realized, mtm }` added to the kernel — the MTM band is the same aggregator re-run with opens reclassified, not a second copy of the math |
| B5/B6 | **Resolved as "label, don't change"** — see below |
| B11 | `PositionsSummary.open_pnl_sol`, marked in-handler from the live token cache through the *same* `CostModel` the sim/sweep use; repo takes a `price_of` closure so it keeps no cache dependency (`lab` passes `|_| None`) |
| F1-F8 | `runSummarySections()` is the sole builder; formatters single-sourced (sweep columns re-export them); zero-as-dash bug gone; one dash glyph; `goodBad()` everywhere |

Verified: `cargo check -p hunter-live -p hunter-lab` clean · 278 backend tests
pass · `tsc` + `npm run build:live` + `npm run lint` clean. The parity lock is
`sim_query::tests::simulate_summary_equals_the_sweep_drill_in_on_the_same_outcomes`.

Pre-existing unrelated failure (fails on clean HEAD too, not touched here):
`core::strategies::rules::generic_tests::generic_params_registry_checked`.

### B5/B6 resolution: label, don't "fix"

Making the sweep honor `max_concurrent_tokens` is **not** a bug fix — it would
require replacing the per-token independent scan with one globally time-ordered
fold across the corpus (what `replay::run_replay` does), serializing the exact
property the sweep's performance design rests on. The two answer different
questions and are now documented as such at `generic/strategy.rs::compile_combo`:

- **sweep** — a combo's raw per-token edge, every qualifying token taken.
- **simulate** — what the rule would actually have captured through its slots.

A capped rule therefore fires on strictly more tokens in the sweep, and its
`n_fired`/`total_pnl_sol` are **upper bounds** on the simulated figures. Same for
`buy_amount_sol`: a sweep explores many combos, so there is frequently no rule to
inherit a notional from, and PnL% is not notional-invariant (fixed per-leg cost).
Compare a sweep to a simulate only when both were sized the same.

### Still open

- B7 (`as_of` captured 3×), B8 (tail-cap scope), B9 (mayhem filter), B10
  (`token_cap` truncation warning). None change the summary shape — they shift
  *which tokens* enter the corpus and *when* a position counts as dead.
- Runtime smoke: no live/paper run exercised `open_pnl_sol` against a real cache
  yet, and no sweep/simulate was run against the live server (`:8140` may still
  hold an older binary).

## Plan

**Phase 1 — backend aggregate SSOT (B1-B4).** Rewrite `sim_query::summarize` to
build `TokenOutcome`s from the sim rows and delegate to `exact_run_metrics`;
replace `SimRollup` and `SimSummary` with `RunMetrics`. Widen the wire DTO.
Guard test: same rows → simulate summary == sweep `exact_from_rows`.

**Phase 2 — backend run inputs (B5-B7).** Thread the rule's caps and
`buy_amount_sol` into the sweep; capture `as_of` once and persist it.

**Phase 3 — backend corpus + tails (B8-B10).** Align the tail cap, add the
mayhem filter, surface `token_cap` truncation.

**Phase 4 — live parity (B11).** Mark open positions to the runtime-cache price
and emit the `RunMetrics` shape.

**Phase 5 — frontend one renderer (F1-F8).** Promote `pnlBlock`/`blockStats` out
of `GenericSweepView` into a shared `runSummarySections()` builder over the
unified DTO; all three surfaces render it. One formatter pair for SOL/%; fix the
zero-as-dash bug; one dash glyph; `goodBad()` everywhere.

## Open decisions

- **Sweep caps (B5).** Assumed: the sweep should honor the rule's real caps.
  The alternative reading is that a sweep is deliberately cap-free (explore the
  raw edge, apply throttling later) — that would make B5 a UI label, not a fix.
- **Live open marking (B11).** The live summary is a SQL aggregate; open PnL
  needs a current price per position, which lives in the runtime cache, not
  Postgres. Options: (a) enrich in the handler after the SQL aggregate,
  (b) persist a periodically-marked `open_pnl_lamports`. (a) is cheaper and
  matches "notify over poll".
