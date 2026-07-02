# Strategy RUN toggle: skip redundant modals + purge empty `strategy_runs`

## Goal

1. **Stop**: when a rule has 0 open positions, clicking `■ Stop` deactivates it immediately — no `StopConfirmDialog`.
2. **Activate**: the Fresh/Continue `ReactivateDialog` goes away entirely. The backend decides Fresh vs Continue automatically.
3. **DB hygiene**: a `strategy_runs` row that never held a single `strategy_positions` row is deleted, not kept as `Stopped`/`Finished` history. Applies on both Pause and Stop.

These are independent: (1)/(2) are pure frontend UX, (3) is a backend invariant that holds regardless of how the rule was stopped/activated.

## Key facts that shape the design

- `▷ Activate` (the button that opens `ReactivateDialog`) only ever renders when `!rule.is_active && rule.open_positions === 0` (`ruleColumns.tsx` `RunControls`). The `open_positions > 0` inactive case renders `▶ Resume` instead, which already activates directly with `continue`, no modal. So **"activate immediately when no holding position" is unconditional** for this button — it's the only state it can appear in.
- `StopConfirmDialog` with 0 open positions today has no destructive content to confirm ("will just be deactivated"). Clicking through the modal vs. calling `stop` directly hits the identical backend path (`stop_and_close_rule`), which re-reads the authoritative in-memory holding index at execution time — so skipping the modal doesn't skip any safety check, only a redundant click.
- `runtime_cache.rs::start_run` currently documents an explicit invariant: *"Runs are immutable history — prior runs + their positions are kept, not deleted."* This plan carves out one narrow, safe exception: a run with **zero** `strategy_positions` rows, ever, isn't history — it's noise from a click that never did anything.
- Deleting a run is only safe when it currently has 0 position rows, because the only things that reference `run_id` are position rows themselves (background sell-confirm tasks hold a position id, not a run id). Zero rows ⇒ nothing can be in flight against that run ⇒ safe to `DELETE`, cascade is a no-op.
- `next_run_seq` is `MAX(run_seq)+1` over existing rows (`strategy_repo.rs:855-864`) — deleting empty runs just frees that sequence number for reuse next time. No gaps/bugs.
- There is currently no run-history UI (only "latest paper run" + a manual "clear all" button), so purging empty runs has zero visible regression today.
- Chosen defaults (per user decision):
  - Activate default: **Continue** if the rule's latest run has ≥1 position ever recorded, else **Fresh**.
  - Empty-run purge applies on **both** Pause and Stop.

## Backend changes

### 1. `trading_core/src/storage/repositories/strategy_repo.rs`

- Add `pub async fn run_position_count(&self, run_id: Uuid) -> anyhow::Result<i64>` — `SELECT COUNT(*) FROM strategy_positions WHERE run_id = $1`.
- Add `pub async fn delete_run(&self, run_id: Uuid) -> anyhow::Result<()>` — `DELETE FROM strategy_runs WHERE id = $1` (single-row form of the existing bulk `delete_runs_by_rule`).

### 2. `trading_core/src/strategies/runtime_cache.rs`

- Add a private helper:

```rust
/// Finalize a run to `status`: if it never held any position, delete it outright
/// (safe — nothing references an empty run) instead of leaving an empty
/// Stopped/Finished stub. Otherwise a normal status update, as before.
async fn finalize_run(
    &self,
    repo: &StrategyRepo,
    rule_id: Uuid,
    run_id: Uuid,
    status: &str,
) -> anyhow::Result<()> {
    if repo.run_position_count(run_id).await? == 0 {
        repo.delete_run(run_id).await?;
        self.current_run_by_rule.remove(&rule_id); // drop the now-dangling pointer
    } else {
        repo.set_run_status(run_id, status, Some(Utc::now())).await?;
    }
    Ok(())
}
```

- `stop_run` and `finish_run`: replace their `repo.set_run_status(run.id, "Stopped"/"Finished", ...)` calls with `self.finalize_run(repo, rule_id, run.id, "Stopped"/"Finished")`.
- Update the `start_run` doc comment to note the exception: *"...prior non-empty runs are kept; a run that never held a position is deleted instead of finalized, not history."*

### 3. `live/src/strategies/service.rs::stop_and_close_rule`

Edge case: `stop_and_close_rule` calls `pause_rule` (→ `stop_run` → `finalize_run`) **before** the force-close loop that deletes leftover 0-entry (`Arming`/never-filled) position rows. If a rule is stopped while a buy is still `Arming`, `finalize_run` sees `count > 0` at that moment and correctly leaves the run as `Stopped` (can't know yet it'll end up empty). After the loop deletes those 0-entry rows, the run could now legitimately be empty but nothing re-checks.

Fix: after the closing loop, re-run the empty check once more:

```rust
if self.repo.run_position_count(current_run_id).await? == 0 {
    let _ = self.repo.delete_run(current_run_id).await;
}
```

(need the run id — grab it via `self.runtime.current_run(rule_id)` before `pause_rule` clears/reassigns it, or have `pause_rule`/`stop_run` return the finalized run id).

### 4. Backend-side Fresh/Continue auto-decision

`live/src/strategies/service.rs`:

- Extend `PaperActivation`:

```rust
pub enum PaperActivation {
    Fresh,
    Continue,
    /// Continue the latest run if it has ≥1 recorded position, else start fresh.
    Auto,
}
```

- In `activate_rule`, add the `Auto` arm:

```rust
PaperActivation::Auto => {
    let has_history = match self.repo.latest_run(rule_id, &rule.trade_mode).await? {
        Some(run) => self.repo.run_position_count(run.id).await? > 0,
        None => false,
    };
    if has_history {
        if self.runtime.resume_run(&self.repo, rule_id, &rule.trade_mode).await?.is_none() {
            self.runtime.start_run(&self.repo, &rule).await?;
        }
    } else {
        self.runtime.start_run(&self.repo, &rule).await?;
    }
}
```

`live/src/api/handlers/strategies/rules.rs`:

- Add `PaperRun::Auto` and make it `#[default]` (replacing `Fresh` as default), map to `PaperActivation::Auto`. `Fresh`/`Continue` stay valid (still used explicitly by `▶ Resume` → `continue`, and available for any future explicit caller) but the plain `▷ Activate` click will simply omit the body / use the new default.

## Frontend changes

Applies identically to all 5 pages: `frontend-react/src/live/pages/strategies/Swing1Page.tsx`, `.../TpslPage.tsx`, `frontend-react/src/lab/pages/strategies/Tpsl1Page.tsx`, `Tpsl2Page.tsx`, `Swing1Page.tsx`.

### Stop: skip modal when `open_positions === 0`

In each page's `ruleControls`:

```ts
onStop: (r: RuleRecord) => (r.open_positions === 0 ? void handleStopConfirm(r) : setStopConfirm(r)),
```

(`handleStopConfirm` already exists — it's what the modal's confirm button calls today.)

### Activate: remove `ReactivateDialog`, always activate directly

- `handleActivateClick` collapses to just `handleActivate(rule)` for every rule (paper and real alike) — drop the `if (rule.trade_mode === 'paper') setReactivate(rule)` branch entirely.
- Drop the `paper_run` argument on this call path (or pass nothing) so the backend's new `Auto` default applies. `▶ Resume` keeps explicitly passing `'continue'` — unaffected.
- Delete the `ReactivateDialog` component, the `reactivate` state, and its modal render block in each of the 5 files.
- Lab pages currently prefetch `fetchTpsl1PaperResult`/equivalent inside `ReactivateDialog` just to decide Fresh vs Continue for display — that prefetch goes away with the dialog (the decision moves server-side, no extra round trip before activating). The `fetchXPaperResult` API function itself stays — it's still used by `PaperResultSection` to show the latest run.

### Types

`frontend-react/src/shared/services/api.ts`: `activateXRule(id, paperRun?)` — no signature change needed; just stop passing `'fresh'` explicitly from the plain-Activate path so the server default (`Auto`) takes over.

## Testing

- `cargo test -p live` — cover `finalize_run` purge behavior (empty run deleted, non-empty run status-updated) and the `Auto` activation arm (continue vs fresh based on position count).
- `cargo check -p live` / `-p lab` clean.
- Manual: activate a rule, let it sit with 0 entries, pause it → confirm the `strategy_runs` row is gone (not `Stopped`). Activate a rule, let it take one entry, close it, stop it → confirm the row remains as history and a later Activate defaults to Continue.
- `npm run build` clean; verify Stop/Activate buttons no longer flash a modal when there are 0 open positions, and still show `StopConfirmDialog` when there are open positions.

## Out of scope / noted but not changing here

- The `Draining` (`open_positions > 0`, inactive) → `▶ Resume` path is already modal-free and unaffected.
- `strategy_run_metrics` is still never populated on the live lifecycle path — unrelated pre-existing gap.
- Lab vs live `open_positions` definition divergence (live: `Holding` only; lab: `Holding|Arming|BuySubmitted`) is unrelated pre-existing behavior; this plan reuses whatever `open_positions` the rule payload already carries, consistent with existing `isSettled`/gating logic.
