# `m_position.arm_above_pct` — the armed trailing stop

Deep-dive reference for the `m_position` strict param added 2026-07-28: what it
does, why the exit grammar needed it, and the measurement that justified it.
Overview of the group lives in [../../arch/strategies.md](../../arch/strategies.md);
the strategy it was built for is
[../../roadmap/flow-scalper-build-plan.md](../../roadmap/flow-scalper-build-plan.md).

## The problem

Two engine facts combine into a trap:

1. **`retrace`'s peak seeds at the entry fill.** `PositionCtx::at_fill` sets
   `peak_price = trough_price = entry_price`, so before the price ever rises,
   `retrace` measures the drop *from entry*. An authored `retrace >= 3` is therefore
   a 3% trailing stop **after** a run-up and a hard −3% stop **before** one.
2. **Exit conditions OR across metrics** (`CompiledRule::exit_fired` returns the
   first req that holds; entry is the AND side). Within one metric the expr is DNF,
   so `pnl` can AND with itself — but `retrace >= 3 AND pnl >= 2` spans two metrics
   and cannot be authored at all.

So "trail out, but only once the trade has cleared the fee" was inexpressible, and
the closest authorable thing silently doubled as a tight stop from entry.

For a dip-buying scalper that is not a rounding error, it is the whole strategy.
You deliberately buy into a falling price; the continuation stops you out before the
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
`retrace >= T`, else his real exit; `mean_net` = gross − 2 pp round-trip fee):

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

A **strict param on `m_position`**, not a new metric and not a grammar change:

```json
"exit": { "m_position": {
    "retrace":       [{ "operator": ">=", "value": 4 }],
    "arm_above_pct": 2
} }
```

| Decision | Why |
| --- | --- |
| strict param, not a metric | it parameterises an existing condition rather than being a quantity you put operators on; `m_position` already had an empty `strict_params` slot and the registry walk picks it up with no new machinery |
| absent ⇒ off | every stored rule round-trips byte-identically; no migration |
| `0` is a legal value | "arm at break-even" is a real setting. This forced `StrictParamSpec::allows_zero` (validators used a blanket `> 0`). It is **not** zero-as-unbound: `None` = off, `Some(0.0)` = arm at break-even, and the two stay distinguishable |
| gates only `retrace` / `bounce` | via the ONE reader `position::is_trailing`. `pnl` is where TP/SL desugar to — gating a stop-loss on already being in profit would disable it. Authoring `arm_above_pct` on an instance with no trailing metric is **rejected at save**, not silently ignored |
| disarmed ⇒ req skipped entirely | `position::trailing_armed` returns false and `exit_fired` `continue`s, so the metric cannot fire by any path. A non-finite pnl stays disarmed (fail closed, matching "NaN satisfies no condition") |

### Call sites (all of them)

| Site | What it does |
| --- | --- |
| `engine/src/metrics/mod.rs` | `StrictParamSpec.allows_zero`; the `m_position` strict param; `registry_json` exposes both |
| `engine/src/metrics/position.rs` | `is_trailing` + `trailing_armed` — the ONE readers |
| `engine/src/arm.rs` | `MetricReq.arm_above_pct`, attached in `build_reqs` to trailing metrics only; the skip in `exit_fired`; `pnl_req` (TP/SL) hardcodes `None` |
| `engine/src/rule_params.rs` | `allows_zero` in the strict-value check; the no-trailing-metric rejection |
| `lab/src/sweep/generic/strategy.rs` | `exit_req_fires` mirrors the skip; `classify_exit_req` sends an armed trailing req to `ExitClass::General` |
| `frontend/.../registry.ts`, `validate.ts` | `allows_zero` mirrored so the FE does not reject `0` |
| `frontend/.../ruleConditionRows.ts` | the row model carries the whole non-window `strict` bag |

### The sweep must not use its fast path

`ExitClass::Trailing` resolves `retrace >= t` through a prefix-extrema hull. Arming
makes the exit a **conjunction** of retrace and pnl — two different running
quantities — which the hull does not index. `classify_exit_req` therefore returns
`ExitClass::General` for any armed trailing req, sending it to the scalar walk.
Locked by `guard.rs::scan_matches_replay_armed_trailing_exit`, which asserts scan ≡
replay under every fill model at gates 0 / 5 / 40 (the last high enough that the
trail never arms and the stop-loss has to close the position).

Per the root rule: the scalar walk is the SSOT, and a correct scalar walk beats a
clever wrong index. Do not "optimise" this back onto the hull without an index that
can see the gate.

### Frontend status

The rule editor has **no dedicated control** for `arm_above_pct` yet — author it via
the API/SQL or the JSON view. What it does have is round-trip safety: the row model
carries every non-window strict param, so opening an armed rule in the editor and
re-saving it returns the param unchanged instead of silently dropping it. Locked by
`ruleConditionRows.test.ts`. Any future registry strict param inherits that for free;
only the editing *control* is per-param work.
