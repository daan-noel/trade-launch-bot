# Plan: make grouped-sweep progress survive navigation

## Problem
Sweep run/progress state lives entirely in `GroupedSweepPage`:
- `running` is `startState.isLoading` (local to the page's mutation hook instance).
- The progress bar's SSE subscription (`connectSweepProgress`) is opened/closed by the page.

Navigating away unmounts the page → `isLoading` is lost, SSE closes. The **backend keeps
running** (the sweep executes inline in the still-open POST; `sweep_running` stays claimed),
but the UI shows nothing, and a fresh page load can't recover the in-flight state because SSE
only delivers *future* frames.

## Goal
A single source of truth for "is a sweep running + how far" that any page can read, that
survives navigation and a full browser refresh, with a global indicator.

---

## Backend (3 small additions)

### 1. Expose current progress in `AppState`
SSE is future-only; a client that mounts mid-run (or after refresh) needs the *current* value.
Add to `AppState` (`state/app_state.rs`), next to `sweep_running`/`sweep_cancel`:
```rust
pub sweep_processed: Arc<AtomicUsize>,
pub sweep_total: Arc<AtomicUsize>,
```
Init to 0. Hand both to `SweepProgress::new(...)` so the observer writes them in `set_total`
(total) and `token_done` (processed) — same atomics it already keeps internally, just shared.
Reset both to 0 when a run starts (alongside the existing `sweep_cancel` reset in the handler).

### 2. Status endpoint — `GET /api/strategies/sweeps/status`
New handler in `grouped_sweep.rs`:
```json
{ "running": bool, "processed": u64, "total": u64 }
```
Reads the three atomics. Register in `api/mod.rs` (sits beside `/strategies/sweeps/cancel`).
Lets a freshly-loaded client recover before any SSE frame arrives.

### 3. Terminal SSE event — notify on finish (no polling)
Add `SseEvent::SweepFinished { strategy_id: String, cancelled: bool }`
(`models/ingest.rs`), rendered as `sweep_finished` in `stream.rs` (not mint-scoped → `None`).
Emit it on **every** exit path of `start_grouped_sweep` (done / cancelled / config-error /
db-error). Cleanest: wrap the emit in the `Gate` Drop, OR send explicitly before each return.
Drop-based is safest (can't forget a path) — give `Gate` an `sse_tx` + `strategy_id` and send
in `drop()`. This is the signal the global bar uses to clear itself.

---

## Frontend

### 1. `SweepStatusContext` (always mounted in `AppProviders`)
`context/SweepStatusContext.tsx`. Holds `{ running, processed, total, cancelling }`.
- **On mount:** fetch `/api/strategies/sweeps/status` once → seed state (refresh recovery).
- **Subscribe** `connectSweepProgress` → `running=true`, set processed/total.
- **Subscribe** new `connectSweepFinished` → `running=false`, `cancelling=false`,
  and `dispatch(apiSlice.util.invalidateTags(['GroupedSweep']))` so the runs list refreshes
  on whatever page the user is on.
- Expose `markStarting()` — page calls it when it fires the mutation, to set `running=true`
  optimistically (covers the gap between POST and the first SSE frame while the corpus loads).
- Expose `cancel()` → `cancelGroupedSweep()` + `cancelling=true`.

Add `connectSweepFinished` to `services/sse.ts` (mirror `connectSweepProgress`) and a
`SweepFinishedEvent` type. Add `getSweepStatus()` to `services/api.ts` (or an RTK query).

### 2. Global indicator in `AppLayout`
Render a slim bar/pill whenever `running` (reuse `ProgressBar`, or a compact variant) fixed at
the top/bottom, with the Cancel button wired to `cancel()`. Clicking the label navigates to
`/strategies/grouped-sweep`. Visible on every route.

### 3. Refactor `GroupedSweepPage`
- Drive the form `running` prop and the inline `SweepProgressBar` from `useSweepStatus()`
  instead of `startState.isLoading` / local SSE.
- In `run()`: call `markStarting()` before `await startSweep(...)`.
- Delete the page-local `SweepProgressBar` SSE wiring (now in context). Keep the inline bar but
  feed it context values, or drop it in favor of only the global indicator (decide in review).
- Auto-jump (`setSelectedRunId(created.id)`) stays best-effort; on return the newest run is
  `runs[0]` by default, so nothing is lost.

---

## Docs (same task)
- `docs/sweep.md`: status endpoint, `sweep_finished` event, `sweep_processed/total` AppState.
- `docs/frontend.md`: `SweepStatusContext` + global indicator + page refactor.
- `docs/architecture.md`: if it enumerates the sweeps endpoints, add `/status`.

## Definition of done
- `cargo check --bin backend` clean; add a small test for the status handler if feasible.
- `npm run build` clean; global indicator doesn't re-render on SOL/USD or trade ticks
  (context state only changes on sweep SSE frames).
- Verify: start a sweep → navigate away → indicator persists → return → bar still live →
  finishes → indicator clears + runs list shows the new run. Refresh mid-run → recovers.
