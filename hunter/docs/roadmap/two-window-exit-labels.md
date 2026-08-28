# Two-window exit-reason labels

`m_flow_window` is **entry-side only**. `rule_params::validate_group` rejects it under
`exit`, and this is what has to land before that restriction lifts.

## Why the restriction exists

A metric exit stamps a persisted reason string —
`trade_share(60s) >= 7.69` — parsed back by `event::parse_metric_exit_label`, whose
window qualifier is a single `({number}s)`. A group whose basis is a window PAIR has no
spelling there: `m_flow_window{60,3}` and `m_flow_window{60,10}` on the same exit side
would both record `trade_share(60s)`, and an operator reading a closed position could not
tell which read fired.

That is a metric shipping *unexplained*, which the root rule forbids
([CLAUDE.md](../../../CLAUDE.md), "A metric ships explained, and explained once"). Rejecting
the authoring is the smaller and honest option: it costs an exit-side capability nobody has
asked for, where the alternative costs an exit reason that lies.

The entry side is unaffected — the readout names the whole requirement rather than a
one-line label, so a burst entry gate renders correctly today.

## What lands

Four places carry the one window, and all four move together or not at all:

| site | today | needs |
| --- | --- | --- |
| `ExitReason::Metrics.window` | `Option<f64>` | the `Windows` carrier |
| `event::split_window_qualifier` | parses `(60s)` | also `(60s/3s)` |
| the reason's `Display` | writes `(60s)` | writes both axes |
| `arm::ConditionRead.window` / `BlockedReq` -> `end_detail.window_size_sec` | `r.window.primary` | a SECOND key, never a widened one |

The last row is the one to get right. `end_detail.window_size_sec` is a stored wire shape
that existing rows already carry; a reader must never mistake a burst axis for the
reference window, so the second axis gets its **own** key (`slice_size_sec`) rather than
overloading the existing one.

`parse_metric_exit_label` must keep accepting the single-window form unchanged — every
stored reason predates this, and a stricter parser turns old rows into unparseable
strings.

## What does not need to change

Nothing on the entry path, and no metric state: `m_flow_window` owns none. The
`Windows` carrier through `MetricReq` / `MonoBound` / `MonoMetricKill` / `BlockedReq` and
the two-axis window registration in `CompiledRule` are already in place, so this is a
labelling change, not a plumbing one.

## When it is worth doing

When a burst term earns a place in an exit, and not before. The derived rules that use it
([6ix-instant-crowd-launch.md](../plans/strategies/6ix-instant-crowd-launch.md)) gate entry
only, and the exit that was graded with them is give-back / clock / arrival — none of which
reads a window pair.
