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

**Control scoreboard** (`GET /api/strategy-rules?score_scope=`):

- `current` (default on live) — latest-run counters for paper **and** real
- `all` — real = all-time; paper = latest run (legacy)

**Evidence** (`POST …/positions?scope=`):

- `current` — latest run **in the rule's own `trade_mode`**
- `run` + `run_seq=N` (+ `mode=real|paper`) — one finished/prior run
- `all` — every run in **both** modes (rows stamped with `run_seq` + `mode`)
- `history` — every run except the current one, both modes (kept for back-compat)

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
┌─ Rules Control (sticky) ── [Current run] [All-time] ─────────────┐
│ RULE · PnL · Win% · N · Live · Status · Execute(Pause/Activate) │
└──────────────────────────────────────────────────────────────────┘
         │ select row
         ▼
┌─ Evidence ── Pause/Activate in header ───────────────────────────┐
│ Runs: [#12 current] #11 #10 … #3 PAPER … [All-time]              │
│ Summary · Temporal · Positions (+ Run/Mode cols when All-time)   │
└──────────────────────────────────────────────────────────────────┘
```

## Files

- Backend: `strategy_repo::{list_runs_with_metrics,find_run_by_seq,rule_counters_for_latest_runs}`, `engine::{list_rules,list_rule_runs}`, `positions` scope `all`/`run`
- Frontend: `RulesPage` + `RulesView` Control strip; `RuleAnalyzePanel` Evidence; `run_seq` column
