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

One aggregate (`kernel::run_summary`), one wire shape (`RunSummary`), one
renderer (`lib/strategy/runSummary`), across all three surfaces.

| # | Outcome |
| --- | --- |
| B1-B4 | **Done.** `sim_query::summarize` delegates to the kernel; `SimRollup` + five-scalar `SimSummary` deleted. `RunSummary { realized, mtm }` added — the MTM band re-runs the same aggregator with opens reclassified, not a second copy of the math. |
| B3+ | **Extra bug found:** "closed" keyed off `exit_time != null` misfiled every death-close (`ExitCode::Dead` carries no exit time) as *open*. Now keyed off `exit_reason`. |
| B5/B6 | **Resolved as "label, don't change"** — see below. |
| B7 | **Done.** `simulate_one_combo` takes the run's `as_of` (its `created_at`); the drill-in no longer calls `Utc::now()`, so stored `Open` positions can't become `Dead` just because the user clicked hours later. |
| B8 | **Not fixed — documented as a deliberate approximation.** See below. |
| B9 | **False positive.** The sweep corpus already filters mayhem at `lake/duck.rs:273` (`WHERE is_mayhem_mode = false`); the audit read only `grouped_sweep.rs` and missed the lake loader. No change needed. |
| B10 | **Done.** Landing on `token_cap` emits a `SweepNotice` + `tracing::warn!`, so a corpus trimmed to its newest slice no longer passes for a full one. |
| B11 | **Done.** `PositionsSummary.open_pnl_sol`, marked in-handler from the live token cache through the *same* `CostModel`; repo takes a `price_of` closure so it keeps no cache dependency (`lab` passes `\|_\| None`). |
| F1-F8 | **Done.** `runSummarySections()` is the sole builder; formatters single-sourced (sweep columns re-export them); zero-as-dash bug gone; one dash glyph; `goodBad()` everywhere. |

Verified: `cargo check -p hunter-live -p hunter-lab` clean · 156 lab + 278 core
tests pass · `tsc` + `npm run build:live` + `npm run lint` clean. Parity lock:
`sim_query::tests::simulate_summary_equals_the_sweep_drill_in_on_the_same_outcomes`.

Pre-existing unrelated failure (fails on clean HEAD too, untouched here):
`core::strategies::rules::generic_tests::generic_params_registry_checked`.

### B5/B6: label, don't "fix"

Making the sweep honor `max_concurrent_tokens` is **not** a bug fix — it would
replace the per-token independent scan with one globally time-ordered fold
(what `replay::run_replay` does), serializing the exact property the sweep's
performance design rests on. Documented at `generic/strategy.rs::compile_combo`:

- **sweep** — a combo's raw per-token edge, every qualifying token taken.
- **simulate** — what the rule would actually have captured through its slots.

A capped rule fires on strictly more tokens in the sweep, so its `n_fired` /
`total_pnl_sol` are **upper bounds** on the simulated figures. Same for
`buy_amount_sol`: a sweep explores many combos, so there is often no rule to
inherit a notional from, and PnL% is not notional-invariant (fixed per-leg
cost). Compare a sweep to a simulate only when both were sized the same.

### B8: why the tail cap stays

The sweep caps each token's tail at `its last trade + DEAD_QUIET_SECS +
TAIL_MARGIN_SECS`. The justification ("past this the token is provably dead")
holds only for a token that lost liquidity — a **quiet but still liquid** token
never books `Dead`, and its monotone `time`/`stall` clocks keep running.

Ground truth is live, which ticks such a token indefinitely and fires e.g.
`exit on time > 2h`. Ranking the three:

- **live** — ticks forever, exit fires.
- **simulate** — tail bounded by the *corpus-wide* last trade, so it usually
  ticks long enough to fire. Closest to live.
- **sweep** — per-token cap, reports `Open`.

Two approaches tried and reverted, recorded so they aren't retried:

1. *Extend the sweep's tail to `dead_cap.max(horizon)`.* Makes the scan fire
   exits a single-token replay never does →
   `guard::scan_matches_replay_stall_eq_exit_across_gap` fails immediately.
2. *Truncate simulate to a per-token tail.* Wrong direction — moves simulate
   **away** from live. Also doesn't work as written: dropping a mint from
   `EngineState::tokens` doesn't stop exit evaluation for its open position, so
   the exit still fires. A correct version needs an engine-level change to the
   pure `reduce` that live shares.

Closing this means either paying the sweep's memory cost to tick a
liquid-quiet token to `as_of`, or accepting that the sweep under-reports exits
for that shape. A perf-vs-fidelity call, not a bug fix — same character as B5.

### Still open

- B8, per above.
- The sweep still takes its own `Utc::now()` at `registry.rs:640` instead of the
  run row's `created_at`. Skew is seconds (immaterial against `DEAD_QUIET_SECS`),
  but sourcing both from the run row would close it exactly.
- **Runtime smoke — nothing here has been exercised against a running system.**
  No live/paper run has marked `open_pnl_sol` against a real cache; no
  sweep/simulate has been run against the server (`:8140` may hold an older
  binary).
