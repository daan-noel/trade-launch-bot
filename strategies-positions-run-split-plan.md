# Strategies positions — split into Current run vs Old runs

Split the single per-rule **Positions** table on the strategies pages into two sections,
each with its own summary + table:

- **Current run** — positions of the rule's latest run (`run_seq = MAX`). Live.
- **Old runs** — every prior run's positions, flat/merged, visually **banded by run**.

## Why this is cheap (findings)

- Positions already carry `run_id`; `strategy_runs.run_seq` is monotonic per `(rule_id, mode)`.
- Fresh activation does **not** delete prior runs — `runtime_cache::start_run` takes
  `next_run_seq` (`MAX+1`) and only clears *in-memory* state. Old `strategy_positions`
  rows keep their `run_id`, so run history is real DB history. (The "only latest run
  retained" comment in `models/paper_run.rs` is stale — ignore/fix it.)
- The repo already exposes run-scoped primitives:
  - `find_positions_paged("sp.run_id", …)` + `positions_summary("run_id", …)` — per run
  - `find_positions_paged("sp.rule_id", …)` + `positions_summary("rule_id", …)` — all runs merged (**today's UI**)
- Today's single table silently merges every run via the `rule_id` scope. The split is
  "stop merging."
- SSE `tpsl_positions_changed` deltas only ever touch the **active** run — so the current-run
  table keeps its live path unchanged; the old-runs table is immutable (no SSE/poll needed).
- `DataTable` already supports `rowClassName?: (row) => string` — banding needs no new primitive.

## Backend (`trading_core` + `live`/`lab` handlers)

1. **Wire run_seq for history rows.** Add `run_seq: Option<i64>` to `PositionResponse`
   (`models/position.rs`). `None` on live/current + SSE deltas; `Some` on history rows
   (needed for the banding + a run column). History query joins `strategy_runs`.
2. **Scope param on positions + summary endpoints** — `?scope=current|history` (absent =
   today's merged behavior, back-compat):
   - `current`: resolve `latest_run(rule_id, mode)`, then `find_positions_paged("sp.run_id", run_id)`
     + `positions_summary("run_id", run_id)`.
   - `history`: new repo fns filtering `rule_id = $1 AND run_id <> <latest>` joined to
     `run_seq` — `find_positions_paged_history` + `positions_summary_history` (or a
     `run_seq < MAX` variant). Order by `run_seq DESC, exit_time DESC` so runs stay grouped.
3. SSE bridge unchanged (`PositionResponse::from` sets `run_seq = None` — it's the current run).

## Frontend (`shared` + `live`/`lab` strategy pages)

1. Split `positionsSection` in `TpslPage.tsx` (and the Swing1/lab equivalents) into
   `CurrentRunPositions` + `OldRunsPositions`, both gated on a selected rule.
2. **Current run:** reuse `useRulePositions` (SSE + fallback poll) with `scope=current`.
   Summary card = unrealized / in-progress (open positions still draining).
3. **Old runs:** new `useRunHistoryPositions` hook — fetch-only, server-paged, own summary,
   **no SSE/no poll** (immutable history). Summary card = realized win/loss.
4. **Banding:** old-runs `DataTable` gets `rowClassName={(r) => bandForRun(r.run_seq)}` —
   alternate two subtle bg tints per `run_seq` group (even/odd run_seq), plus a small
   `run_seq`/run-date column. Distinct style so run boundaries read at a glance.
5. Hide the Old-runs section when the rule has ≤1 run; hide both when never activated.

## Edge cases / guardrails

- `run_seq` is per `(rule_id, mode)` — history scoping must pass `mode` (as `latest_run` does).
- Rule with a single run → empty history section (hidden).
- Old runs accumulate on the 4GB live box, but per-run position count is bounded by
  `max_total_tokens`, so volume stays small — no retention change needed; note if it grows.

## Done

- `cargo check -p live` + `-p lab` clean; repo parity/summary tests still green.
- `npm run build` clean; no extra re-render on SOL/USD tick or live-trade delta.
- Docs: update `@arch/strategies.md` (two-section positions) + this plan.
