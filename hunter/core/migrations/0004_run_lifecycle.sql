-- ===========================================================================
-- 0004_run_lifecycle — exit buckets the generic engine actually produces.
--
-- Context: `strategy_runs.status` used to be written exactly once ('Running' at
-- insert) and never again — `set_run_status` had no caller — so pausing a rule
-- and re-activating it reused the SAME run row (`Sink::ensure_run` reuses any
-- latest run whose status is 'Running'). Every position a rule ever took piled
-- into run #1 and the Evidence pane's History scope ("all runs except the current
-- one") stayed permanently empty. A run now ends when its rule stops being
-- active, and `strategy_run_metrics` is finally written at that point.
--
-- Rolling those metrics up needs somewhere to put a redesigned rule's exits. The
-- table was shaped for the retired tpsl/swing ladder, so `ExitReason::Metrics`
-- (where EVERY generic-engine exit lands), the analysis-only death-close, and the
-- operator/migration closes had no column — the persisted histogram would have
-- read all-zero beside a non-zero n_closed.
--
-- Runs left 'Running' by the old behavior are NOT healed here: the engine
-- finalizes them at boot (`StrategyRepo::orphan_running_runs`), which also rolls
-- their metrics up and keeps healing later drift (a db-sync from another box, a
-- hand-edited `is_active`) instead of fixing it once at migration time.
-- ===========================================================================

ALTER TABLE strategy_run_metrics
    ADD COLUMN IF NOT EXISTS n_exit_dead     INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_metrics  INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_manual   INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS n_exit_migrated INTEGER NOT NULL DEFAULT 0;
