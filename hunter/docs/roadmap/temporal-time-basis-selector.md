# Temporal time-basis selector (Wall clock + Timing)

> Status: **PLANNED, not started.** The prerequisite shipped 2026-08-06: every dated
> chart now bins on the decision instant and the Wall clock's `exit_time` option exists
> and is the default. Durable contract:
> [`../plans/frontend/position-summary.md`](../plans/frontend/position-summary.md)
> (*Wall clock vs Timing — the shared time basis*).

## Why

A position carries three stamps — token **created**, **bought**, **sold**. Today the Wall
clock can be pointed at any of the three (toolbar toggle), but the Timing card (calendar +
dow×hour heatmap) is hard-wired to *sold*. So the moment you switch the Wall clock to
"Created at" to ask a launch-cohort question, the two cards silently disagree again — the
exact confusion the 2026-08-06 work removed.

This makes the basis **one shared choice** across both cards, which also unlocks the
question you cannot ask today: *do my bad Tuesdays come from Tuesday **entries** or
Tuesday **exits**?* — flip the basis and watch the same grid re-fold.

## The hard constraint (read first)

The selector drives **only the wall-clock-dated charts**: Wall clock, calendar, heatmap.

It must **NOT** reach:

| Chart | Why not |
| --- | --- |
| Equity path | Realized PnL lands when you sell. A cumulative curve ordered by token creation date is not an equity curve — it is a different quantity wearing the same shape. |
| Return shape | Buckets by PnL %, has no time axis. |
| Hold mix / Hold vs PnL | Bucket by *duration*; the wall stamp is irrelevant. |

Equity is the anchor the default was chosen from. Letting it follow the selector would
re-open the inconsistency from the other side.

## Decisions

| # | Decision |
| --- | --- |
| 1 | **Reuse `WallTimeField`** (`exit_time` \| `entry_time` \| `created_at`) as the one vocabulary. Do NOT introduce a parallel `TimeBasis` enum — same fact, two names, guaranteed drift. UI copy says "Sold / Bought / Created"; the wire keeps the field names. |
| 2 | Default stays **`exit_time`**. |
| 3 | State is **owned by `PositionSummarySection`** and passed down; `TemporalSummary`'s internal `localField` remains the fallback for standalone mounts (DryRunDetail). |
| 4 | Persist in **one shared key** across Evidence / Simulate / Sweep, same pattern as `usePnlDistDensity`. New `STORAGE_KEYS` entry, e.g. `positionTimeBasis: \`${PREFIX}strategy.timeBasis\``. |
| 5 | **Two renderings, one state:** keep the existing toggle in the Wall toolbar AND add the same control to the Timing card header, both bound to the shared value. A single control in one card leaves the other looking hard-wired. |
| 6 | Changing the basis **clears the `day` / `week` / `heat` focus lenses** (they were authored against the old stamp). Same rule `setHoldChoice` already applies to the hold lens. |

## Anatomy (so you don't re-explore)

| File | Role now | Change |
| --- | --- | --- |
| `lib/strategy/positionChartPoints.ts` | `PositionChartPoint.timeMs` = decision instant | **+ `createdMs` / `entryMs` / `exitMs`**; `timeMs` stays as-is (equity/scatter/dist read it) |
| `components/strategy/RuleAnalyzePanel.tsx` `evidenceChartPoints` | collapses the 3 stamps into `timeMs` | also emit the 3 stamps |
| `pages/strategies/SimulatePage.tsx` `simChartPoints` | same | same |
| `pages/strategies/sweep/GenericSweepView.tsx` `comboChartPoints` + `comboFocusRow` | same | same |
| `components/strategy/PositionSummarySection.tsx` | owns Timing fold + `toFocusRow` | own the basis state; feed `basisMs(p, basis)` into the timing fold **and** `toFocusRow.timeMs` |
| `lib/strategy/positionFocus.ts` | `timeLensMatches` reads `row.timeMs` | unchanged — it follows automatically once `toFocusRow` carries the basis stamp |
| `components/strategy/TemporalSummary.tsx` | `localField` + Wall toolbar `ToggleGroup` | accept controlled value (already does via `wallField` prop); render the shared control |
| `services/api.ts` `fetchEngineSimTimeSummary` | sends `wall_field` | unchanged — already carries the basis |
| `lab/src/strategies/sim_query.rs` | `WallTimeField::time_of` | unchanged |

Note the shape of the win: the server, the Rust fold, the focus predicate and the table
filter all need **no change** — they already key off one stamp. The work is the projection
layer plus the control.

## Phases

### Phase 1 — carry all three stamps

`PositionChartPoint` gains `createdMs` / `entryMs` / `exitMs` (`number | null`). Fill them
in the three projections; the data is already parsed there today, just discarded.

**Trap:** all three projections currently `continue` when the collapsed `timeMs` is not
finite. Under a basis switch that drop must move to fold time (per basis) — otherwise a
row with no `created_at` disappears from *every* chart the moment anyone picks "Created",
and never comes back.

### Phase 2 — basis-derived folds

Add `basisMs(point, field)` next to `toPnlPoints` (the twin of FE `wallTimeMs`, over the
point shape instead of the row shape — or better, unify them; one reader if it fits).

In `PositionSummarySection`:
- `timingPnlPoints` uses `basisMs` instead of `timeMs`
- `toFocusRow` sets `timeMs: basisMs(p, basis)` — this one line makes the `day`/`week`/
  `heat` lenses **and** their table filters follow the basis for free
- `deck` (equity/buckets) and `holdPoints` keep `timeMs` untouched — see the constraint

### Phase 3 — the control + persistence

Shared hook (`useTimeBasis`, mirroring `usePnlDistDensity`) → `STORAGE_KEYS` entry.
`PositionSummarySection` owns it, passes to `TemporalSummary` via the existing `wallField`
prop, and renders the same `ToggleGroup` in the Timing card header. Wire the lens-clearing
from decision 6. Simulate/dry-run already refetch the server fold when `wallField` changes.

### Phase 4 — labels

- Both card headers state the active basis ("by sold", "by created").
- **Focus chips need a basis suffix** (`Tue 14:00 · sold`). Without it, the same cell
  clicked under two bases produces two chips that render identically —
  `positionFocusLabel` in `lib/strategy/positionFocus.ts`.
- Decide whether the lens *stores* its basis. Cheapest: it does not (basis change clears
  the lens per decision 6), and the suffix is rendered from the current basis. Revisit only
  if lenses ever become URL-backed here.

## Edge cases

- **Null stamps.** `created_at` can be missing on older rows; `exit_time` is null while
  open (falls back to entry — already handled by `wallTimeMs` / `time_of`). Under a basis
  with no fallback, rows drop out of the timing charts. Per the no-silent-caps rule, show
  the dropped count in the card hint (`3 positions have no created time`), never a silently
  shorter cohort.
- **Open positions** under `sold` sit at their buy instant. Unchanged, but the label should
  not claim otherwise.
- **Sweep** folds client-side and holds every row in memory, so a basis switch is a re-fold
  with no refetch; **Simulate/dry-run** refetch. Both paths must land on the same numbers —
  worth one manual cross-check on the same rule.

## Tests

| Test | Where |
| --- | --- |
| `basisMs` returns each stamp; `sold` falls back to bought when open | `positionChartPoints.test.ts` (new) |
| A day lens authored under `bought` filters on entry time, not exit | `positionFocus.test.ts` |
| Basis change clears `day`/`week`/`heat` lenses | component-level or a pure helper test |
| Rows with a null stamp are counted, not silently dropped | `temporalSummary.test.ts` |

Existing twins (`wall_buckets_floor_in_the_requested_zone`,
`exit_time_binning_keeps_open_positions_at_their_buy`) must stay green — this work does not
touch the flooring or the Rust fold.

## Acceptance

- Switching the basis re-folds Wall clock + calendar + heatmap **together**; equity, return
  shape and hold scatter do not move.
- The choice survives a reload and is shared across Evidence / Simulate / Sweep.
- A calendar day clicked under `bought` narrows the table to rows *bought* that day.
- Chips read unambiguously under a basis change.

## Effort

~Half a day. Mostly the projection plumbing (Phase 1-2); the control is small and the
backend is already done.
