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
  "hold_sec": 0.5,
  "cells": [
    { "phase": "*",  "situation": "burst",   "on": [{"if": [...], "sell": "burst"}] },
    { "phase": "P2", "situation": "leaving", "on": [...] }
  ]
}
```

- `phases[i].until`: the AND that ends phase `i`; the coin moves to `i + 1` on the first
  step it holds. The last phase has none. A step moves at most one phase.
- `situations`: checked top to bottom, the first `when` that holds wins; the last one has no
  `when` and catches the rest.
- `hold_sec`: a new situation takes over only after it has held this long, so a reading that
  sits on a cut does not flip the cell every print.
- `cells[].phase` / `.situation`: a name or `*`. A step runs the lines of every entry that
  covers its cell, most specific first (cell, then situation row or phase column, then
  `*`/`*`); the first line that holds acts, as in a stage today.
- `enter.cells`: where a buy may start. The position starts in the cell the coin is in at the
  fill. Omitted = every cell.
- `always` keeps its meaning: checked before any cell, in every cell.

`stages` and `phases` do not appear in one rule. A rule without `phases` runs exactly as
today.

## One step

1. `always` lines.
2. Phase: if the current phase's `until` holds, move one phase on.
3. Situation: the first `when` that holds; it replaces the current one once it has held
   `hold_sec`.
4. The cell's lines (most specific first). A `sell` acts as today; `go` does not exist in a
   cell (phases and situations move by their own conditions).

Each step is one print or tick, as now. A move takes effect from the next step.

## Metrics

| Metric | Meaning |
| --- | --- |
| `m_position.phase_sec` | seconds since the position entered its current phase |
| `m_position.situation_sec` | seconds since the current situation took over |

Both are clock reads on the held side, like `m_position.stage_sec`. Situation and phase
conditions use the existing families. Two reads the 6ix map needs and the registry lacks, each
added only on approval:

| Needed read | Meaning |
| --- | --- |
| time since a tag's last trade | seconds since the last print carrying the tag (`owner silent 3 s`) |
| highest pool so far | the highest `liquidity_sol` the coin has reached (one-way life stages) |

## State and restart

A position stores its phase index, situation index, and the times each began, in
`strategy_positions.extra`. A restart restores them from that row. The pending situation
under `hold_sec` is not stored: after a restart it starts over, which only delays a switch.

## Limits

At most 16 phases and 16 situations per rule. Cell entries are compiled into one table at
load, so a step costs one phase check, at most one pass over the situations, and the lines of
one cell.

## Editor

Two lists (phases with their `until`, situations with their `when`, both reorderable) and the
grid. A grid cell shows its action (buy allowed / sell lines / hold) and opens its lines; a
row or column header sets lines for every cell in it.

## Work, in order

1. Parse and validate the document (`rule_params.rs`), compile the cell table (`arm.rs`).
2. The step (`arm.rs`), the two `m_position` clocks, restart state.
3. Parity: the lab simulate fold and a golden test per move kind (phase move, situation
   switch under `hold_sec`, cell precedence, entry cell).
4. The editor grid.
5. The two missing reads, if approved.
