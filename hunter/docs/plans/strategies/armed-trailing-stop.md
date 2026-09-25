# The gated trailing stop

Deep-dive reference for a trail that only counts once the trade is in profit: what it
does, why the exit grammar needs it, and the measurement that justifies it. The engine
overview is [../../arch/strategies.md](../../arch/strategies.md); the honesty laws that
grade any exit shape sit in [_!___strategy.md](_!___strategy.md) 7.2.

## The problem

**`m_position.retrace_pct`'s peak seeds at the entry fill.** `EnteredCtx::at_fill` sets
`peak_price = trough_price = entry_price`, so before the price ever rises, `retrace_pct`
measures the drop *from entry*. An authored `retrace_pct >= 3` is therefore a 3 % trailing
stop **after** a run-up and a hard -3 % stop **before** one.

So "trail out, but only once the trade has cleared the fee" needs a gate on
`m_position.pnl_pct`, and the closest ungated thing silently doubles as a tight stop from
entry. For a dip-buying scalper that is not a rounding error, it is the whole strategy:
you deliberately buy into a falling price, and the continuation stops you out before the
reversion you bought for.

## The measurement

Replaying the exit policy over omego's own 2,974 closed episodes (5-day window
2026-07-22..27, `scratchpad/c1b_policy.sql` pattern — episodes reconstructed from
his running token balance, priced against every market tick inside each episode):

**Max since-entry-peak drawdown he holds through, by outcome:**

| outcome | episodes | median | p75 | over 3% | over 5% |
| --- | --- | --- | --- | --- | --- |
| winners | 1,757 | 4.98% | 10.83% | **65.4%** | 49.7% |
| losers | 1,217 | 14.27% | 24.89% | 92.4% | 84.3% |

Two thirds of his winners dip more than 3% off their running peak *before* winning.

**Applying an unarmed trail to his own episodes** (exit at the first tick where
`retrace_pct >= T`, else his real exit; `mean_net` = gross - 2 pp round-trip fee):

| exit policy | fired | mean gross | mean net | median gross | win | clears fee |
| --- | --- | --- | --- | --- | --- | --- |
| *his actual* | — | 4.75% | **+2.75%** | 1.93% | 59.1% | 49.7% |
| unarmed trail 3, stop 25 | 76.4% | 2.84% | +0.84% | 0.14% | 51.3% | 39.7% |
| unarmed trail 5, stop 8 | 63.9% | 2.95% | +0.95% | 0.23% | 51.9% | 41.1% |
| armed g=0, trail 3, stop 12 | 55.5% | 3.27% | +1.27% | 2.17% | 68.6% | 51.1% |
| armed g=2, trail 4, stop 12 | 46.1% | 3.43% | **+1.43%** | 2.86% | 63.9% | **55.7%** |
| armed g=0, trail 5, stop 25 | 35.5% | 3.45% | +1.45% | 2.35% | 67.1% | 51.7% |

**No trail width rescues the unarmed form** — 2, 3, 5, 8, 12 and 20 all land between
+0.84% and +0.98% net, and 21% of his winners flip to losers at trail 3. Arming it
roughly doubles the net edge and lifts the median exit from break-even to clearly
above the fee.

Read the armed rows with one caveat: they fire on fewer episodes (46-56% vs 64-76%),
and a non-firing episode falls back to *his* exit, so part of their mean is borrowed.
The unconfounded signal is the **median gross of the exits it does make** — 2.2-2.9%
armed vs 0.14-0.23% unarmed — and that gap is what the fee threshold turns into the
difference between an edge and a treadmill.

Two corollaries worth keeping:

- **With an unarmed trail the `stop_loss` is dead code.** Trail 3 gives identical
  results at stop 8 and stop 25 — the trail always fires first. Only once the trail
  is armed does the stop start doing work (net +1.06% at stop 6 → +1.45% at stop 25).
- **His winners barely go underwater**: median worst mark −0.81%, p25 −3.31%, only
  26.6% ever below −3%. So the hard stop wants to sit near −8..−12%, not −3% and not
  −25%.

## The design

A gate is ordinary rule grammar: a line's `if` is an AND across metrics, and a stage is a
latch. The two spellings are different exits, so say the one the rule means.

| form | spelling | the trail counts |
| --- | --- | --- |
| per-reading gate | one line: `pnl_pct >= 2 and retrace_pct >= 4 -> sell` | only at a reading where the position is still at least +2 % |
| armed stage (latch) | stage `start`: `pnl_pct >= 2 -> go armed`; stage `armed`: `retrace_pct >= 4 -> sell` | from the first reading at or above +2 % for the rest of the hold |

On a run to +10 % that then falls, the per-reading trail fires at 4 % off the peak only
while pnl is still at least +2 %; below that it is silent and the stop loss closes the
position. The armed stage keeps trailing all the way down.

```json
{
  "stop_loss": 12,
  "stages": [
    { "name": "start",
      "on": [ { "if": [ { "metric": "m_position.pnl_pct", "is": [{ "operator": ">=", "value": 2 }] } ],
                "go": "armed" } ] },
    { "name": "armed",
      "on": [ { "if": [ { "metric": "m_position.retrace_pct", "is": [{ "operator": ">=", "value": 4 }] } ],
                "sell": "trail" } ] }
  ]
}
```

| Decision | Why |
| --- | --- |
| the gate reads `pnl_pct`, and never gates the stop | `stop_loss` / `take_profit` compile to the first `always` lines, checked in every stage before its own lines, so the stop works armed or not. Gating a stop on already being in profit would disable it |
| `0` is a legal gate | "arm at break-even" is a real setting |
| a non-finite pnl never arms or counts | `NaN` satisfies no condition, so the trail fails closed |
| the move takes effect from the next print or tick | one step per evaluation: the reading that crosses the gate moves the stage, the `armed` lines are first read on the next one |
| `m_position.stage_sec` is time since arming | a clock on the armed phase needs no second latch |

**Converted v1 rules.** v1's `arm_above_pct` on a lone trailing clause checked the current
pnl at each reading, so it converts to the per-reading line (locked by
`scan_matches_replay_armed_trailing_exit` at gates 0 / 5 / 40, the last high enough that
the trail never counts and the stop loss closes the position). A v1 `m_position.armed = 1`
clause under a pnl latch converts to the armed stage; v1 latched before the exits of the
crossing event, so the converted `start` stage also carries `pnl_pct >= X and <clause> ->
sell` for each such clause, which can hold only on the crossing reading
([`v1.rs`](../../../engine/src/v1.rs)).

### The sweep walks both forms

The sweep's fast exit paths (`sweep/generic/fast_exit.rs`) answer only a flat held side:
one stage, no deadline, every line one condition selling the whole bag, where a
prefix-extrema hull finds a `pnl_pct` bound and a running-peak scan finds a trail. A gated
trail is a conjunction (two conditions on one line) or a stage move, which neither index
can see, so `FastPlan::of` returns `None` and the rule goes to the row walk
(`scan::resolve_exit_walk`), the reference every fast path is checked against. Do not
"optimise" a gated trail onto the hull without an index that can see the gate.

### A restart does not remember an armed stage

A pure stage move writes nothing: `strategy_positions.scale_stage` records the stage a
partial fill landed in, not a `go`. An adopted `Holding` row therefore resumes in the stage
PG last recorded (for a trail with no partial sells, `start`: unarmed), with the peak
re-seeded from entry and `stage_sec` counting from the entry (`orphan_exit.rs`). That is the
consistent pair: a peak with no history is no evidence the gate was crossed, and re-arming
on it would trail from a price the position never reached. The per-reading form needs no
memory, so it survives a restart unchanged.

### Authoring

The rule editor spells both forms directly: two conditions on one sell line, or a
`start` stage whose line moves to an `armed` stage. The sentence under each condition
comes from the registry.
