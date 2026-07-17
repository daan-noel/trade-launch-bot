# Generic sweep — precompute-then-scan (strategy redesign 5.4–5.6)

Deep-dive reference for the redesigned grouped sweep that replaces the three
per-strategy wrappers (`tpsl1`/`tpsl2`/`swing1`) with one generic engine. Read
[`fingerprint-metrics-engine-plan.md`](fingerprint-metrics-engine-plan.md) §2.6 /
§5.4–5.6 first for the design contract; this documents the *implementation*.

## Where it lives

| File | Role |
| --- | --- |
| `hunter/lab/src/sweep/generic/axes.rs` | axes model: wire `AxisSpec` → registry-resolved `ResolvedAxis`; combo enumeration → `RuleParams` |
| `hunter/lab/src/sweep/generic/strategy.rs` | `GenericSweepStrategy` (`Strategy` impl): per-token precompute (`MetricSeries`) + per-combo scan |
| `hunter/lab/src/sweep/generic/guard.rs` | **scan ≡ `run_replay`** drift lock (step 5.5) |
| `hunter/engine/src/metrics/series.rs` | `MetricSeries` — extended with per-row `price`/`reserve_sol`/`dead` |
| `hunter/lab/src/sweep/registry.rs` | `"generic"` strategy id → `GENERIC_TABLES` + `sweep_generic` |
| `hunter/lab/migrations/0003_generic_grouped_sweep.sql` | the one `grouped_sweep_{runs,groups,results,combos}` table set |
| `hunter/lab/src/api/handlers/strategies/grouped_sweep.rs` | `promote_group` (5.6) |

The partition (`grouped_engine::partition` by `group_key`), two-phase pool,
`GroupSink` streaming persistence, `ComboAgg`/`ComboMetrics` aggregation, and the
whole `start_grouped_sweep` handler are **reused unchanged** — the generic engine
is just one more `Strategy` implementation behind the registry.

## Reuse via the `Strategy` trait

`GenericSweepStrategy` implements the existing `sweep::strategy::Strategy` trait, so
the entire sweep machinery drives it exactly like the legacy strategies:

- `prepare_token(token)` → **the precompute**. One replay pass builds a
  `MetricSeries` over the token's trades + 500 ms ticks.
- `resolve_entry` / `resolve_exit` → **the scan**. A cheap walk over the
  precomputed series applying the engine's decision logic. The engine's per-token
  entry cache (keyed by `entry_key`) recomputes the entry once per contiguous
  same-entry combo block — entry axes are ordered as the high-order combo digits so
  a grid keeps them contiguous.

`prepare_token` was widened from `&[CorpusTrade]` to `&CorpusToken` so the generic
engine can anchor its metric clock at the token's real `created_at` (carried onto
`CorpusToken` from the lake `tokens` dimension) — the same `created_at`
single-rule simulate feeds `ReplayToken`, the 5.3 parity keystone.

## The axes model

An axis is one swept dimension; a combo picks one value per axis, assembling one
`RuleParams`:

- **metric axis** `(side, group, metric, operator[, window])` — each value → a
  `{operator, value}` condition on that metric. Group/metric are named and resolved
  against `hunter_engine::metrics` (a typo is a hard error, not a silent no-op);
  dynamic (`m_time_window`) metrics require a `window`.
- **`take_profit` / `stop_loss` axis** — each value sets the rule's TP / SL %.

`combo_count` = product of axis value counts; `combo_params(idx)` mixed-radix
decodes (axis 0 most significant) and assembles the `RuleParams`. One
`window_size_sec` per side's `m_time_window` group is enforced (RuleParams stores a
single window per group per side).

## The precompute (`MetricSeries`)

`build_series` mirrors, event-for-event, the single-token stream
`strategies::replay` folds through the live `reduce`:

- trades → `TradeLite` **exactly** as `replay::load_tokens` (canonical spot price;
  REAL reserves for deadness; `NaN` reserves ⇒ alive),
- interleaved with 500 ms ticks on a grid anchored at `created_at + TICK_MS`,
- a tail up to `min(as_of, last_trade + DEAD_QUIET_SECS + TAIL_MARGIN_SECS)`.

`MetricSeries` records, per event row: every axis metric column's value **plus**
three rule-independent facts the scan needs but no metric expresses — `price` (the
fill price / TP-SL reference), `reserve_sol`, and the precomputed `dead` verdict
(`is_dead_verdict`, computed once per row since it is rule-independent).

## The scan

Per combo (a `CompiledRule`), over the precomputed rows:

- **entry** (armed side) — `Dead > Unsatisfiable > (enter-on-arm | entry
  conditions)`. The fill lands at the first finite-price row at/after the decision
  (an enter-on-arm or pre-print decision defers to the first trade — the engine's
  `pending_buys` wait-for-price).
- **exit** (open side, from the fill row on) — `Dead > StopLoss > TakeProfit >
  Metrics`. No exit by the tail ⇒ `Open`, marked to last price.

Conditions are evaluated with the **shared** `evaluator::eval` (same tolerance,
same `=`-bucket semantics) the live engine uses; TP/SL and dead use the same
`reduce` arithmetic. Caps do **not** apply — the sweep judges each token
independently, as the legacy sweep always has. PnL is priced through the shared
`round_trip_with_costs(CostModel::pumpfun_default)` + f32 quantization, identical to
single-rule simulate's `outcome_to_row`.

### The new exit taxonomy

The redesigned engine has one metric-condition exit (`ExitReason::Metrics`) in
place of the legacy ladder's granular metric exits. `ExitCode::Metrics` (kernel) +
`n_exit_metrics` (RunMetrics / ComboMetrics / results tables) carry it. The legacy
strategies never emit it (their `n_exit_metrics` is always 0).

## Parity lock (step 5.5)

`guard.rs` runs a sample corpus (one token per exit path) two ways — a single-token
`run_replay` (the real engine fold) and the scan — and asserts identical per-token
outcomes (fired, exit code, entry/exit price, PnL) across TP / SL / Metrics / Dead /
Open and entry-gated rules. Because the series values ARE `TokenTrack` values, the
tick grid + `TradeLite` mapping match replay, and the evaluator / decision / cost
code is shared verbatim, the scan is parity-correct by construction; the guard fails
first if that ever drifts.

## Promotion (step 5.6)

`POST /api/strategies/sweeps/{run_id}/groups/{group_id}/promote?strategy_id=generic[&combo_id=N]`
rebuilds the group's `Fingerprint` from its stored `group_key` **at the run's bucket
width** (continuous SOL fields use the bucket's lower-edge representative, which lies
in the bucket, so `same_bucket` reproduces the group's membership), `find_or_create`s
it (equal winning groups map onto ONE fingerprint), and returns a pre-filled
`RuleDraft`-shaped body the editor opens for review → dry-run → save. The rule itself
is not persisted here.
