-- 0012_exit_redrive_park.sql — bound the reaper's ExitFailed auto-redrive.
--
-- WHY. The reaper (redrive_exit_failed_bags) re-attempts every real ExitFailed
-- position that still has a bag, every ~60s (5-tick backoff), UNBOUNDED and
-- forever. Once a transient failure (a resolved layout/2006/RPC blip) clears
-- this recovers the position for free — but on a dead/rugged pool every retry
-- reverts and burns tip+fees indefinitely. Policy (user): retry a bounded number
-- of times, then PARK (leave ExitFailed, stop auto-retrying, surface for a manual
-- decision) — never auto-write-off.
--
-- These two dedicated columns hold that state. They are intentionally NOT part of
-- StrategyRepo::update_position's fixed column list, so the engine fold re-booking
-- ExitFailed after a failed redrive can never clobber the counter (a jsonb `extra`
-- key would be overwritten by that full-row write). Both default to the
-- "fresh, not yet retried, not parked" state for every existing row.

ALTER TABLE strategy_positions
    ADD COLUMN IF NOT EXISTS exit_redrive_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS exit_parked        BOOLEAN NOT NULL DEFAULT false;
