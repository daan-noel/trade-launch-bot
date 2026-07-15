-- Per-run bucket width (SOL) for the continuous SOL grouping fields.
--
-- Grouped sweeps used to partition the continuous SOL fingerprint axes (initial
-- buy, max cost, spendable-in, first-slot buy/sell) at a hardcoded 0.1-SOL bucket
-- width. That width is now a per-run knob (the same one the created rule's live
-- matcher + the creation-stats dashboard bucket by — "what you swept = what you
-- run"), so it must be persisted for display, re-run, and rule promotion.
--
-- REAL to match `buy_amount_sol` (the model widens back to f64). NULL on existing
-- rows → callers fall back to the default width (`grouping::SOL_BUCKET_WIDTH`, 0.1),
-- which is exactly the width those rows were swept at, so history stays correct.
-- `IF NOT EXISTS` keeps this idempotent (the `_lab_migrations` ledger already gates
-- re-application; the guard just makes a hand-run safe too).

ALTER TABLE tpsl1_grouped_sweep_runs   ADD COLUMN IF NOT EXISTS bucket_width_sol REAL;
ALTER TABLE tpsl2_grouped_sweep_runs   ADD COLUMN IF NOT EXISTS bucket_width_sol REAL;
ALTER TABLE swing_1_grouped_sweep_runs ADD COLUMN IF NOT EXISTS bucket_width_sol REAL;
