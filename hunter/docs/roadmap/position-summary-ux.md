# Position summary UX upgrade

> Status: **PLANNED** (not shipped). Upgrade the shared "summary above positions
> table" on Evidence / Simulate / Sweep. **Out of scope:** Console History
> charts deck (already has its own cohort + focus model — see
> [`docs/plans/frontend/review-surfaces.md`](../plans/frontend/review-surfaces.md)).

## Goal

Keep the current summary's depth; make it easier to research a run at a glance
and to drill into rows without losing the full-cohort picture.

Primary read: **am I making money?** Secondary: **does hold time / wall time
matter?** Audience: **deep research**. Vertical space is fine if sections are
**collapsible** with remembered open/closed state.

## Surfaces (same layout everywhere)

| Surface | Today | Must match after |
| --- | --- | --- |
| Rules Evidence (live + lab) | `SimSummaryCard` + `TemporalSummary` + positions table | Shared accordion + focus + new charts |
| Simulate result | `SummaryStatsPanel` + `TemporalSummary` + sim table | Same |
| Sweep combo drill-in | `SummaryStatsPanel` + `TemporalSummary` + combo rows | Same |

Shared building blocks live under `frontend/src/shared/components/strategy/` and
`shared/components/analytics/`. Do not fork a live-only or lab-only summary
chrome.

## Locked product decisions

| # | Decision |
| --- | --- |
| 1 | Accordion defaults: Hold & time / Return shape / Details **open**; Equity **collapsed**. User can change; **persist in one shared `localStorage` key** across all three surfaces. |
| 2 | Tile/chart clicks **add** filters (stack). Not replace. |
| 3 | Clicks land in a **Focus chip strip** (option B) — they do **not** write into the table filter row. Table column filters stay independent and still narrow the cohort. |
| 4 | Clear: click the same tile/slice again to drop that lens; **and** a Clear-all on the chip strip. |
| 5 | Summary + every chart use the **full matching cohort** (all positions in scope that pass table filters + focus chips) — **never the current table page only**. Recompute when either changes. |
| 6 | Clickable filters: Open / Closed / Fired, exit reason, win vs loss, migrated. **Capital tiles are display-only** (no click filter). |
| 7 | New charts **include open** positions (unrealized / mark PnL), not closed-only. |
| 8 | Cohort bound = existing **run selector** (`current` / `run`+`run_seq` / `all`). No soft row cap — always fetch all within the selected scope. Users narrow with run chips when All-time is heavy. |
| 9 | Console History unchanged. |

## Layout

```text
[Always visible]
  Hero KPIs          (PnL realized, PnL incl. open, Win %, Fired / open)
  Exit mix bar       (segments clickable → focus chip)
  Focus chips        (stacked lenses + Clear all)

[Accordion — open state in shared localStorage]
  ▾ Hold & time          (default open)
      Hold × exit bars   (existing Temporal)
      Wall-clock timeline (existing Temporal)
      Hold vs PnL scatter (NEW)

  ▾ Return shape         (default open)
      PnL % distribution  (NEW)

  ▾ Details              (default open)
      Positions / Exits / Realized / MTM / Capital tiles
      (capital tiles not clickable)

  ▸ Equity path          (default collapsed)
      Cumulative PnL + max DD (NEW; includes open as unrealized marks)
```

Hero + exit mix stay outside the accordion so the money read never requires a
click. Temporal controls (grain, hold scheme, wall field, metric toggles) stay
inside Hold & time.

## Cohort model

```text
Run scope  →  table filters (search / columns)  →  Focus chips (stacked)
                         ↓
         one cohort feeds: server/client summary + all charts + table page
```

- **Scope** already exists on Evidence (`current` | `run` | `all`) and is the
  primary way to bound large histories. Keep the run navigator prominent.
- **Table filters** continue to drive the positions query / client row set.
- **Focus chips** further narrow that set for summary, charts, and table rows.
- When the cohort changes, summary numbers and charts update together. No chart
  may fold a different query than the summary.

### Per-surface data path

| Surface | Summary today | Chart series needed |
| --- | --- | --- |
| Evidence | `POST …/rules/{id}/positions/summary` over filtered cohort | **NEW:** compact per-position (or per-close+open) series for the same scope+filters, full population — mirror Console B2 shape but rule/run-scoped; include open rows with unrealized PnL |
| Simulate | `POST …/simulate/result/summary` + time-summary | Fold distribution / scatter / equity from the same filtered result set (server page+summary already; extend or client-fold from a full-cohort fetch if the page is bounded) |
| Sweep drill-in | `runSummaryFromRows(filteredRows)` | Client-fold from the filtered combo rows (already full in memory) |

Evidence must not build distribution/scatter/equity from the current table page.
Fetch the full scoped series (bounded by run selector, not by page size).

## Focus chip contract

Additive lenses. Suggested kinds (wire/UI names can match History's spirit but
stay local to these surfaces — do not overload Console `hfocus`):

| Lens | Source | Table effect |
| --- | --- | --- |
| `status:open` / `closed` / `fired` | Positions tiles / hero | status / fired predicate |
| `exit:<reason>` | Exit mix segment or exit tile | exit reason match (same needles as elsewhere where applicable) |
| `outcome:win` / `loss` | Win% / outcome tiles | `pnl > 0` / `pnl <= 0` (define consistently for open = unrealized sign) |
| `migrated:yes` / `no` | Migrated tile | migrated flag |
| `hold:<bin>` | Hold bar | hold-seconds range or Open bin (existing Temporal mint/hold path, unified into chips) |
| `wall:<cell>` | Wall timeline cell | time window / mint set from cell — prefer expressing as the same chip layer, not a parallel mint-only side channel |
| `pct:<lo>:<hi>` | Distribution bar | pnl % bucket (open uses unrealized %) |
| `pos:<id>` | Scatter point | single position |
| `band:…` | Scatter drag-zoom | hold + pnl% rectangle |

Capital / entry-size tiles: **no lens**.

Chips render above the table (and ideally also under the exit mix). Removing a
chip or re-clicking the source clears that lens only; Clear all drops every lens.
Table column filters are untouched by chip clear (user clears chips only).

**Linked Temporal brush** (hold ↔ wall) stays, but the driving selection must
register as focus chip(s) so summary recomputes and the state is visible/clearable.

## Chart scorecard (keep / add)

| Chart | Verdict | Notes |
| --- | --- | --- |
| Hero KPIs | Keep always on | Money-first |
| Exit mix bar | Keep always on; clickable | Glance; tiles in Details for counts |
| Hold × exit bars | Keep (Hold & time) | Primary for hold research |
| Wall-clock timeline | Keep (Hold & time) | Primary for when |
| Hold vs PnL scatter | **Add** | Money × hold per position; include open |
| PnL % distribution | **Add** | Return shape; include open (unrealized %) |
| Equity / cum PnL | **Add**, default collapsed | Path + max DD; include open marks |
| Detail bands | Keep in Details accordion | Full research detail; clickable where listed |
| Console calendar / heatmap | Do **not** add here | Book-review charts; stay on Console |

## Accordion persistence

- One key, e.g. `hunter.positionSummary.accordion` (exact key chosen at impl).
- Shape: map of section id → open boolean.
- Missing key → defaults above (Hold/Return/Details open, Equity collapsed).
- Shared across Evidence, Simulate, Sweep — not per-page keys.

Reuse existing accordion/localStorage patterns from `components/ui/Accordion` and
`hooks/useLocalStorage` / `lib/storage` where they fit; do not invent a second
persistence helper.

## Implementation sketch (phased)

### Phase 1 — Shell + persistence

- Extract a shared `PositionSummarySection` (name flexible) that composes:
  hero/exit mix via existing `SummaryStatsPanel` / `runSummarySections`,
  Focus chip strip, accordion sections, Temporal, placeholders for new charts.
- Wire accordion ↔ shared localStorage.
- Mount from `RuleAnalyzePanel`, Simulate page, Sweep combo drill-in — same
  component props: summary stats, temporal rows/payload, chart points, focus
  state, callbacks.

### Phase 2 — Focus layer

- Unified focus state (React state; URL optional later — not required for v1).
- Map each lens → row predicate and/or structured filter applied **on top of**
  the table query (Evidence/Sim: fold into request or client filter after fetch;
  Sweep: filter in-memory rows).
- Summary refetch/recompute and chart refold on every cohort change.
- Chip UI: list active lenses, dismiss one, Clear all; toggle on re-click.

### Phase 3 — Evidence full-cohort series

- Backend: rule/run-scoped closes+opens series (compact fields: id, times,
  hold, pnl_sol, entry_sol, win/sign, exit_reason, migrated, status/open flag,
  run_seq). Same scope grammar as positions (`current` / `run` / `all`).
- Apply the same filter subset the summary uses so series and summary cannot
  disagree.
- Frontend: one fetch feeds distribution, scatter, equity (and can replace
  ad-hoc page folds).

### Phase 4 — New charts

- Reuse `PnlDistribution`, `HoldPnlScatter`, `EquityCurveChart` /
  `foldPnlDeck` from `shared/components/analytics/` where possible — extend
  folds to accept open points (unrealized) rather than cloning chart code.
- Wire selection → focus chips; density preference can share
  `hunter.pnlDistDensity` with Console (view preference, not cohort).

### Phase 5 — Tile click targets

- Exit mix segments + exit tiles → `exit:` lenses.
- Open / Closed / Fired / Migrated / Win-Loss → status/outcome/migrated lenses.
- Ensure live `PositionsSummary` exit breakdown and sim/sweep `runSummary`
  expose enough to highlight selected slices.

### Phase 6 — Docs + parity

- Update `docs/arch/frontend.md` Evidence / Simulate / Sweep bullets.
- Fold durable contracts from this roadmap into
  `docs/plans/frontend/` (e.g. extend `rules-cockpit-ux.md` or a short
  `position-summary.md`) when shipped; then mark this file SHIPPED or delete
  per docs discipline.

## Non-goals

- Changing Console History / Portfolio / Home digest.
- Soft-capping All-time series.
- Making capital / avg-entry tiles into filters.
- Writing focus lenses into the DataTable filter row.
- Adding day-calendar or dow×hour heatmap to these surfaces.

## Acceptance checks

- On Evidence, Simulate, and Sweep, the accordion layout and defaults match.
- Toggling a section on one surface is remembered on the others.
- Narrowing the table (column filter) updates hero, Details, Temporal, and new
  charts to the same cohort; paging the table does not change them.
- Stacking exit + hold + pct lenses narrows table and summary together; Clear
  all restores the table-filter-only cohort.
- Open positions appear in distribution / scatter / equity with unrealized PnL.
- Capital tiles never add a focus chip.
- Console History behavior unchanged.

## File map (expected touch points)

| Area | Likely files |
| --- | --- |
| Shared shell | `shared/components/strategy/` (new section + focus chips); `SummaryStatsPanel`, `SimSummaryCard`, `TemporalSummary`, `runSummary.tsx` |
| Analytics reuse | `shared/components/analytics/{PnlDistribution,HoldPnlScatter,EquityCurveChart,pnlSeries}` |
| Evidence | `RuleAnalyzePanel.tsx`; live/lab evidence wrappers; RTK + `…/positions` summary; **new** series endpoint under rule positions / portfolio family |
| Simulate | `lab/pages/strategies/SimulatePage.tsx` (+ time-summary / result summary) |
| Sweep | `lab/pages/strategies/sweep/GenericSweepView.tsx` |
| Persistence | `hooks/useLocalStorage` / `lib/storage`; shared accordion key |
| Docs | this file → arch/plans on ship |
