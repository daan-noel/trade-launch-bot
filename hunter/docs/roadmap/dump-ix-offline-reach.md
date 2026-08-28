# `m_dump_ix` offline reach

The dump group is live-reachable and chart-reachable; it is **not** search-reachable.
A rule can gate on `dump_sell_count`, a readout explains it and `/metric-series` draws
it, but no sweep, metric-discovery run or rule-search can put it on an axis. The
threshold in a dump exit is therefore hand-picked, not derived.

Group contract: [`../arch/strategies.md`](../arch/strategies.md#dump-builds-m_dump_ix--m_dump_ix_window).
Offline machinery: [`../arch/sweep.md`](../arch/sweep.md).

## Why it is unreachable

Offline, a fingerprint-scoped metric reads a `SeriesColumn::Fingerprint` column folded
against `SWEEP_FLOW_FP` — the run's synthetic fingerprint, configured from ONE
corpus-wide pattern list on the run body. Two things stop the dump group short:

| Layer | State |
| --- | --- |
| Column routing (`sweep/generic/axes.rs`, `discovery/candidates.rs`, `rule_search/{cuts,scorer}.rs`) | Branches on `is_flow_metric`, so a dump axis falls through to a **bare** `Static`/`Window` column — no fingerprint, hence NaN on every row, and no `FlowPatternsMissing`-style skip to say so |
| Run body + persistence (`grouped_sweep.rs`, `grouped_sweep_runs.ix_patterns`) | One `ix_patterns` field, compiled into `{"m_flow_ix": {...}}`. There is no second list to configure the dump group with, so a corrected column would read an empty set |

The first is a predicate swap (`is_fingerprint_scoped`, then route the group's own list).
The second is a request-shape decision and a lab migration, and it is the one to settle
first: a column that reads the right state off an empty list is the same silent NaN.

## The decision the second layer needs

Either a sibling `dump_patterns` field beside `ix_patterns` (narrow, one more migration
the next fingerprint-scoped group repeats), or one `metric_config` JSON on the run that
every group reads through its own `from_metric_config` (the shape the fingerprint row
already uses, and the reason a group added later needs no sweep change). The second
matches [`../arch/strategies.md`](../arch/strategies.md); it costs a backfill of the
existing `ix_patterns` column into the nested shape.

## Guard to add with the fix

`every_metric_is_live_reachable.rs` covers the ARM path only, which is why this gap
shipped green. The offline twin — every registry metric resolves to a column that reads
non-NaN state on a fold that configures it — belongs beside it.
