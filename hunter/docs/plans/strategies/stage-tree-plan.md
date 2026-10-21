# Stage tree: phases x situations

A rule's held side as a grid instead of one list of stages. Built for any fingerprint: the
engine knows no group, tag or threshold; a rule names all of them.

- **Phase**: where the coin is in its life. Moves forward only.
- **Situation**: what the owner and the outsiders do now. Re-picked every step.
- **Cell**: one phase x one situation. Holds that stage's lines.

The 6ix case that motivates it: `hunter/_local/study-6ix-buy/NOTES.md` sections 16-16c.

## Rule document

```json
{
  "enter":  { "event": [...], "cells": [["P2", "leaving"], ["P3", "*"]] },
  "phases": [
    { "name": "P1", "until": [{"metric": "m_state.age_sec", "is": [{"operator": ">=", "value": 1.9}]}] },
    { "name": "P2", "until": [...] },
    { "name": "P4c" }
  ],
  "situations": [
    { "name": "idle",  "when": [...] },
    { "name": "burst", "when": [{"metric": "m_flow.buy_sol", "tag": "!volume", "span": "1s", "is": [{"operator": ">=", "value": 0.5}]}] },
    { "name": "pump-light" }
  ],
  "cells": [
    { "phase": "*",  "situation": "burst",   "on": [{"if": [...], "sell": "burst"}] },
    { "phase": "P2", "situation": "leaving", "on": [...] }
  ]
}
```

- `phases[i].until`: the AND that ends phase `i`, on metrics that never fall (`>` / `>=` on a
  monotonic life metric: `m_state.age_sec`, `m_state.peak_liquidity_sol`, a life count). The
  last phase has none. **The coin's phase is the number of leading phases whose `until`
  holds now**: it only moves forward, is the same before and after the buy, and needs no
  stored state, so a restart loses nothing. A buy at age 20 s finds the coin already in P3.
- `situations`: checked top to bottom, the first `when` that holds wins; the last one has no
  `when` and catches the rest.
- No situation hold: a situation switches on the print that makes it. A hold of 0.25-2 s
  scores the same or worse on 6ix (`_local/study-6ix-buy/r44_hold.py`), because it delays
  both the buy and the sell; it would also be the one piece of stored state.
- `cells[].phase` / `.situation`: a name or `*`. A cell takes its lines from the ONE most
  specific entry that covers it (phase and situation named, then one of them, then `*`/`*`);
  that entry replaces the wider ones, so `P4a x burst` can raise the burst a wider
  `* x burst` line sells on. A cell that both `P x *` and `* x S` cover needs its own entry
  (validation refuses the doubt). The first line that holds acts, as in a stage today.
- `enter.cells`: where a buy may start. The position starts in the cell the coin is in at the
  fill. Omitted = every cell.
- `always` keeps its meaning: checked before any cell, in every cell.
- Cell lines read `m_position.*`. A situation's `when` may read `m_position.*` too; before a
  buy those read `NaN`, so such a situation only matches while holding.
- A cell has no `go`. A part sell (`sell_pct`) has no stage move to end it, so it stops
  itself: its `m_position.sold_pct` conditions leave a range no wider than the sell
  (`sell_pct: 50` with `sold_pct < 50`), and once it fills the line no longer holds. A
  situation may read `sold_pct` too ("after the half is sold").
- A cell entry carries all its lines: it replaces the wider entry whole, so a narrower entry
  that changes one threshold repeats the other lines. Inheritance waits until rules show the
  need.

`stages` and `phases` do not appear in one rule. A rule without `phases` runs exactly as
today.

## One step

All on the same print or tick, in this order:

1. Phase: how many leading `until`s hold.
2. Situation: the first `when` that holds.
3. `always` lines.
4. The lines of that cell's most specific entry. A `sell` acts as today; `go` does not
   exist in a cell (phases and situations move by their own conditions).

So the print that turns the coin into `burst` is the print the burst sell fires on, as in the
study kernel. Entry runs steps 1-2 on every armed coin's print; outside `enter.cells` the buy
waits like a failed filter, and the pre-entry veto reads `always` plus the current cell.

A tree rule keeps the one implicit stage, and the held step reads the cell's lines where a
staged rule reads its stage's: one step function serves both (`CompiledRule::held_step`).

## Metrics

| Metric | Meaning |
| --- | --- |
| `m_state.peak_liquidity_sol` | the highest `liquidity_sol` any print has left so far, SOL, life; never falls |

| `m_position.sold_pct` | percent of our first buy's tokens that part sells have sold, 0 until one fills |

Added only when a rule needs one, on approval: time in phase / time in situation (seconds since
the coin's current phase / situation began).

## State and restart

Nothing is stored: phase and situation are read from the coin's metrics on every step, so a
restarted engine finds every coin in the cell it was in.

## Limits

At most 16 phases and 16 situations per rule. Cell entries are compiled into one table at
load, so a step costs one phase check, at most one pass over the situations, and the lines of
one cell.

## Editor

Phases (left, the stage color) and situations (right, their own color), each a reorderable
list with its `until` / `when`, then the grid in the same two colors. A grid cell shows BUY
(click to allow or stop a buy there) and its sell (`+ sell` adds one; a dim sell is the
column's, row's or every cell's). The selected cell's pane edits its lines for this cell, its
column (`* x S`), its row (`P x *`) or every cell. Typing a part-sell percent in a cell line
adds `m_position.sold_pct < that percent`. In words draws the same two lists and the grid.

## Status

Built: the document (`rule_params.rs`, tests in `rule_params_tests.rs`), the compiled tree and
the step (`arm.rs`, tests in `arm_tests.rs`), `m_state.peak_liquidity_sol`,
`m_position.sold_pct` and part sells in cells, the registry parts `phases` / `situations` /
`cells` / `enter.cells`, the editor (`TreeEditor.tsx`; the shared fixture
`engine/fixtures/rule_v2_tree.json` is read by both sides). The 6ix rules run in lab simulate: three
paper, inactive rules `6ix tree max_pnl / recommended / safest`; engine vs kernel on 10-18 +
10-19: 135 of 138 shared coins within 0.003 SOL, 5 engine-only coins, totals +2.34 vs +2.36.

Open: the live server needs this engine before a tree rule can run there.
