# Running sell/stop task visibility — design & plan

**Problem.** Across both products, every user-triggered sell/stop action is either
**blocking with no push** (forge `manage/execute`, sweep, consolidate; hunter
`manual_sell`) or **fire-and-forget where the only live signal is the generic feed**
(hunter real rule stops → `tpsl_positions_changed` / `trade_executed`). **No surface
reports an action-scoped "running → N of M → done/failed" status.** The operator clicks,
sees a frozen spinner or a button label, and cannot tell what is happening.

**Principle (one pattern, applied everywhere).**

1. A long-running action becomes **fire-and-forget with an `action_id`**: the handler
   records the action, returns the id immediately, and spawns the work.
2. The worker **emits a scoped progress event over the product's existing SSE hub** at
   **start** (`running`, 0/N), **each step** (k/N), and **end** (`done|partial|failed`).
3. The frontend **subscribes** and renders live status **on the triggering surface**
   (row status / bulk progress) plus a small **global "running tasks" indicator**.
4. **Instant DB-flip actions** (pause, stop-bot, cancel-ladder) don't fake progress —
   they get **optimistic + confirmed** UI only.

Status is then **real server state**: it survives page reload and shows across tabs.

---

## Shared progress-event shape (both products)

Each product keeps its own SSE hub and enum — these are separate products with
decoupled wire contracts — but both add a variant with the **same field vocabulary** so
the frontend hooks/UI are near-identical:

```
action_progress {
  action_id: string        // opaque id the triggering request returned
  kind:      "sell" | "stop"
  scope:     { mint?: string, rule_id?: uuid, wallet?: string }  // what it targets
  status:    "running" | "partial" | "done" | "failed"
  done:      number        // steps/legs/positions completed
  total:     number        // steps/legs/positions total
  error?:    string        // set when status = failed | partial
}
```

- `done/total` drives the "3/8" counter and the bulk progress bar.
- `status` drives the row/badge tone: `running` = amber "…ing", `done` = green,
  `partial|failed` = red.
- Terminal frame (`done|partial|failed`) lets the client stop showing in-flight state
  without waiting on the incidental trade feed.

---

## HUNTER (do first — the position SSE stream already exists)

Hunter already streams position transitions over `tpsl_positions_changed`
(`runtime_cache.rs::emit_position_delta`, wire shape `SseEvent::TpslPositionsChanged`).
The real rule-stop path (`service.rs::stop_and_close_rule` → `trigger_real_exit` /
`spawn_real_sell`) already marks each open position `ExitPending`, pushes the delta, and
spawns the on-chain sell. So most of the machinery exists; the gaps are:

- **Per-row "Sell ALL" (surfaces C, H4) doesn't move the row.** It calls `manual_sell`
  (`live/src/api/handlers/trading/solana.rs:281`) — a **raw wallet sell by mint** that
  never touches the `StrategyPosition`, so the row stays `Holding` until the async
  reconcile closes it. **Fix:** route a rule-position sell through the **position-aware
  close path** (same mechanism as Stop & close, scoped to one position) so the row goes
  `ExitPending` → pushed immediately → closed. The `TradePage` free-form sell (H4) keeps
  `manual_sell` but gets optimistic "Selling…" console state (no position to move).
- **Stop & close / Stop All (H1/H2) rely on scattered position deltas** with no
  action-scoped rollup. **Fix:** emit an `action_progress` frame (kind `stop`, `done/total`
  = positions closed / total) at start and as each spawned sell confirms, so the bulk
  button shows "Stopping 3/8".
- **Pause / Pause All (H3) + rule enable/disable** are instant flag flips. **Fix:**
  optimistic "Pausing…" → SSE-confirmed `Paused` via the existing `tpsl_rules_changed`.
- **Frontend:** reusable amber "Selling…"/"Stopping…" status tone on rule-position rows
  (reuse the `ExitPending` amber from `positionColumns.tsx`), a bulk progress indicator
  on the Stop-All button, and a global running-tasks indicator fed by `action_progress`.

## FORGE (bigger lift — no progress push exists at all)

Forge's `SseHub` (`forge/live/src/sse.rs`) has no event covering `manage/execute`,
sweep, or consolidate; all three are blocking. `restore_progress`/`restore_complete`
(the wallet-pool restore job) is the **working precedent** to copy.

- **Add `action_progress` to `SseHub`** + the `EventSink` seam so launcher handlers emit
  it (mirror how `wallet_pool_fund` already receives `sse`).
- **`manage/execute` (surfaces A, F1): make fire-and-forget.** Insert the `ManageAction`
  row (already done — `insert_executing`), return `action_id` (202), spawn the per-leg
  loop, emit `action_progress` (kind `sell`, `done/total` = legs confirmed) at start /
  per leg / end. Stops holding an HTTP connection open for minutes on the 2vCPU box.
- **ManagePanel + HoldingsTable:** subscribe, show live per-wallet status in the Status
  column + an "N/M legs" banner instead of a frozen spinner.
- **Sweep (F2) + consolidate (F3):** long wallet-loop jobs → emit `action_progress`, or at
  minimum a blocking spinner + the returned `SweepReport` surfaced.
- **Volume stop / ladder cancel / pause-resume (B, F4):** instant DB flips → optimistic
  "Stopping…" row state + confirmed badge flip. **No new bulk endpoints** (decision: a
  "stop everything" stays N individual calls for now).

---

## Surface inventory (status quo → target)

| # | Surface | Product | Now | Target |
|---|---|---|---|---|
| C | Rule-position row "Sell ALL" | hunter | blocking `manual_sell`, row stuck `Holding` | position-aware close → row `ExitPending`→closed via SSE |
| H1 | Per-rule Stop & close | hunter | fire-and-forget, scattered deltas | + `action_progress` rollup |
| H2 | Stop All | hunter | looped H1 | + bulk `action_progress` (N/M) |
| H3 | Pause / Pause All | hunter | instant flip, no feedback | optimistic + `tpsl_rules_changed` |
| H4 | TradePage manual sell | hunter | blocking | optimistic console "Selling…" |
| A/F1 | manage/execute (single + all-wallets) | forge | blocking, no push | fire-and-forget + per-leg `action_progress` |
| F2 | Wallet sweep & retire | forge | blocking, no push | `action_progress` (or spinner + report) |
| F3 | Consolidate → treasury | forge | blocking, no push | `action_progress` (or spinner + report) |
| B | Volume stop / ladder cancel | forge | 204 flip, no feedback | optimistic "Stopping…" + confirmed badge |
| F4 | Volume pause / resume | forge | 204 flip | optimistic + confirmed |

## Verification (every surface)

- Trigger, watch status go `running → done/failed`.
- **Reload the page mid-action** → status is still shown (proves real server state, not a
  local guess).
- **Second tab** → same status appears (proves it's pushed, not request-local).
- `cargo check -p hunter-live`/`-p hunter-lab` + `-p forge-live` clean; `npm run
  build:live` (both frontends) clean; clippy on touched code.
