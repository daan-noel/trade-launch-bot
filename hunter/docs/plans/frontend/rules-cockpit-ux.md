# Rules Control + Evidence UX

Live Rules is a **control board**, not an authoring form with history bolted on.

## Jobs

| Surface | Job |
| --- | --- |
| **Rules Control** (sticky scoreboard) | Compare rules · activate/pause · current-run health |
| **Evidence** (lower pane / `:ruleId` route) | Prove why — runs · summary · positions · charts |
| **Floor** (`/ops`) | Inventory (sell / waiting / open / attention) — not rule ON/OFF |
| **Portfolio** | Cross-rule money over time |
| **Rule Editor** | Params drawer — secondary |

## Scopes

**Control scoreboard** (`GET /api/strategy-rules?score_scope=`):

- `current` (default on live) — latest-run counters for paper **and** real
- `all` — real = all-time; paper = latest run (legacy)

**Evidence** (`POST …/positions?scope=`):

- `current` — latest run
- `run` + `run_seq=N` — one finished/prior run
- `all` — every run (rows stamped with `run_seq`)
- `history` — prior runs only (kept for back-compat)

**Run navigator:** `GET /api/strategy-rules/{id}/runs` — runs newest-first with optional finalized `strategy_run_metrics`.

## UI shape

```text
┌─ Rules Control (sticky) ── [Current run] [All-time] ─────────────┐
│ RULE · PnL · Win% · N · Live · Status · Execute(Pause/Activate) │
└──────────────────────────────────────────────────────────────────┘
         │ select row
         ▼
┌─ Evidence ── Pause/Activate in header ───────────────────────────┐
│ Runs: [#12 current] #11 #10 … [All-time]                         │
│ Summary · Temporal · Positions (+ Run col when All-time)         │
└──────────────────────────────────────────────────────────────────┘
```

## Files

- Backend: `strategy_repo::{list_runs_with_metrics,find_run_by_seq,rule_counters_for_latest_runs}`, `engine::{list_rules,list_rule_runs}`, `positions` scope `all`/`run`
- Frontend: `RulesPage` + `RulesView` Control strip; `RuleAnalyzePanel` Evidence; `run_seq` column
