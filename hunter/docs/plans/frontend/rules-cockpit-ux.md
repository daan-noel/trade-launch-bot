# Rules Control + Evidence UX

Live Rules is a **control board**, not an authoring form with history bolted on.

## Jobs

| Surface | Job |
| --- | --- |
| **Rules Control** (sticky scoreboard) | Compare rules · activate/pause · current-run health |
| **Evidence** (lower pane / `:ruleId` route) | Prove why — runs · summary · positions · charts |
| **Floor** (`/floor`) | Inventory (sell / waiting / open / attention) — not rule ON/OFF |
| **Portfolio** | Cross-rule money over time |
| **Rule Editor** | Params drawer — secondary |

## Scopes

**Control scoreboard** (`GET /api/strategy-rules?score_scope=&score_mode=`):

- `score_scope=current` (default on live) — latest-run counters for paper **and** real
- `score_scope=all` — real = all-time; paper = latest run (legacy)
- `score_mode=paper|real` — score **every** rule on that one ledger instead of its own
  `trade_mode`. Absent is a third state, not a default: it is the per-rule "own mode"
  board you keep/kill on. Pinning a mode is the only way to read a rule's paper record
  while it trades real, and the only basis on which rules in different modes rank
  against each other — paper pays no slippage a real fill pays.

With a mode pinned, `all` means all-time on **both** sides (`rule_counters_for_all_in_mode`),
not the legacy paper-latest-run asymmetry above: an explicit comparison across unlike
spans is worse than no comparison. A row whose `trade_mode` differs from the pinned
`score_mode` carries a mode pill on its PnL cell and is counted in the scoreboard caption
(`N of M rules are not running real`) — the figure is real history, but not the ledger
that rule is live on, and nothing else in the row says so. The TOTAL strip buckets by
the mode the numbers came from rather than by the rule's `trade_mode` — filing a paper
figure under `real` is the currency blend the tiles refuse.

`score_mode` has **no control of its own**. The page carries one Paper/Real picker, and a
`Score all rules on this ledger` checkbox decides which job that pick does:

| Picker | Modifier | Rows | `score_mode` |
| --- | --- | --- | --- |
| `All` | n/a (disabled — `All` names no ledger) | all | absent |
| `paper`/`real` | off | that mode only | absent |
| `paper`/`real` | on | **all** — the pick is a ledger, not a filter | that mode |

A picked mode is either a row filter or a ledger, never both, so `RulesView` derives
`score_mode` and drops the row filter whenever the modifier is on (a board narrowed to
real rules *and* scored on real is the state the modifier is there to escape). The cost
is deliberate: filter and ledger can no longer disagree, so "show only the real rules,
scored on their paper history" is not expressible. Two Paper/Real controls side by side
cost more — they read as one control split in half.

**Evidence** (`POST …/positions?scope=`):

- `current` — latest run **in the rule's own `trade_mode`**
- `run` + `run_seq=N` (+ `mode=real|paper`) — one finished/prior run
- `all` — every run in **both** modes (rows stamped with `run_seq` + `mode`)
- `all` + the server-side `mode` filter — the same population as one trade mode's
  ledger. Paper and real are not one result set (paper pays no slippage a real fill
  pays), so the panel offers a per-mode All-time chip beside the combined one
  whenever the rule has runs in both. It is a **filter, not a fourth scope**: one
  predicate narrows the page, the pager total, the summary card and the chart series
  together, and the Mode column drops out because every row now says the same word.
- `history` — every run except the current one, both modes (kept for back-compat).
  With no run in the rule's own mode there is no current run to exclude, so this is
  every run the rule has — same population as `all`, never empty.

**An empty `current` falls back to `all`, once.** A restarted rule opens on a fresh
`run_seq` that has traded nothing yet, and a rule flipped into a mode it has never
traded in has no current run at all; either way the default scope pages an empty
table while the rule's whole history sits one chip away, which reads as "this rule
never traded". The panel re-scopes to All-time on the first settled load of the
pristine view only — after the user picks a scope, filters, or focuses, an empty
result is theirs and stays put.

`trade_mode` is a switch, not a partition: a rule owns every position it ever took,
and only `current` is mode-scoped. Scoping the history to the live toggle position
would hide the whole real ledger the moment the rule is flipped to paper.

`run_seq` is monotonic per `(rule, mode)`, so it is half a key — `#1 paper` and
`#1 real` are different runs. `scope=run` therefore carries the `mode` too
(absent ⇒ the rule's own), and any cross-mode scope shows the Mode column beside Run.

**Run navigator:** `GET /api/strategy-rules/{id}/runs` — both modes, the rule's own
first, newest-first within each, with optional finalized `strategy_run_metrics`. Each
row carries its `mode`; the panel chips the other mode's runs with a mode badge and
keeps the cross-run PnL trend strip inside one mode (paper and real PnL are not one
series).

## UI shape

```text
┌─ Rules Control (sticky) ── Pause All / Stop All ─────────────────────────────┐
│ SHOW [All][PAPER][REAL]  ☐ Score all rules on this ledger  + tag chips       │
└──────────────────────────────────────────────────────────────────────────────┘
  SPAN [Current run][All-time]   Scoreboard = … — 12 of 31 not running real
┌─ TOTAL tiles ────────────────────────────────────────────────────────────────┐
│ RULE · PnL(+mode pill when off-ledger) · Win% · N · Live · Status · Execute  │
└──────────────────────────────────────────────────────────────────────────────┘
         │ select row
         ▼
┌─ Evidence ── Pause/Activate in header ───────────────────────────┐
│ Runs: [#12 current] #11 … #3 PAPER … [All-time][All REAL][All PAPER] │
│ Summary · Temporal · Positions (+ Run/Mode cols when All-time)   │
└──────────────────────────────────────────────────────────────────┘
```

## Files

- Backend: `strategy_repo::{list_runs_with_metrics,find_run_by_seq,rule_counters_for_latest_runs,rule_counters_for_all_in_mode}`, `engine::{list_rules,list_rule_runs}`, `positions` scope `all`/`run`, `rules_with_counters(score_scope, score_mode)`
- Frontend: `RulesPage` (owns `score_scope` only) + `RulesView` (owns the mode picker, the
  `scoreAllModes` modifier and the `score_mode` derivation); `RuleAnalyzePanel` Evidence;
  `run_seq` + `mode` columns

Two rules keep the controls apart:

- **Place by job.** The sticky strip carries only what steers the board while scrolling —
  which rows, and the bulk actions. Anything that decides how a number is computed (`Span`,
  the caption) sits on the scoreboard it governs, above the TOTAL tiles.
- **Label by verb.** `Span`, `Score on`, `Show` — never a bare `Paper`/`Real`, which every
  other mode control on the page also spells. The mode picker's own prefix flips between
  `Show` and `Score on` to name the job the modifier has put it in.
