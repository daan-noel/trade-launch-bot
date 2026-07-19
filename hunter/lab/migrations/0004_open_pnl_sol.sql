-- Unrealized (still-`Open`) PnL alongside the realized `total_pnl_sol`.
--
-- Every headline sweep stat is realized-only: `RunAgg` folds a still-`Open`
-- position into `n_fired`/`n_open` but keeps its mark-to-last-price PnL out of
-- `total_pnl_sol`/`win_rate`/`score` (parity plan C2 — an unrealized mark isn't a
-- trade outcome, and folding it in made headline numbers depend on exactly when
-- the corpus window happened to end). That invariant stands, but it left the
-- group/combo tables unable to distinguish "made 10◎" from "made 10◎ while
-- sitting on 40◎ of open losses" — the realized total read as pure profit.
--
-- `open_pnl_sol` carries that excluded mark as its own column so the UI can show
-- `total_pnl_sol + open_pnl_sol` (mark-to-market) beside the realized figure.
-- It is NEVER summed into `total_pnl_sol`; the realized columns are untouched.
--
-- Note this is a *display* fix only: `best_combo`'s ranking still scores on the
-- realized `score`, so a combo that leaves its losers open still ranks as if they
-- didn't exist. Changing the ranking is a separate, deliberate decision.
--
-- The shared `GroupedSweepRepo` interpolates one static column list over every
-- table set, so the legacy tables gain the column too (always 0 there — the
-- legacy sweeps are dropped in phase 7). `ADD COLUMN IF NOT EXISTS` keeps this
-- idempotent beside the `_lab_migrations` ledger that already gates re-application.
--
-- `REAL` (f32) matches the narrowed storage convention of the other PnL columns
-- (migration `0007` of the main chain); the repo widens back to f64 at the
-- mapping boundary. `DEFAULT 0` backfills pre-existing rows — historical sweeps
-- report no open PnL rather than a NULL the UI would have to special-case.

ALTER TABLE grouped_sweep_results         ADD COLUMN IF NOT EXISTS open_pnl_sol REAL NOT NULL DEFAULT 0;
ALTER TABLE tpsl1_grouped_sweep_results   ADD COLUMN IF NOT EXISTS open_pnl_sol REAL NOT NULL DEFAULT 0;
ALTER TABLE tpsl2_grouped_sweep_results   ADD COLUMN IF NOT EXISTS open_pnl_sol REAL NOT NULL DEFAULT 0;
ALTER TABLE swing_1_grouped_sweep_results ADD COLUMN IF NOT EXISTS open_pnl_sol REAL NOT NULL DEFAULT 0;
