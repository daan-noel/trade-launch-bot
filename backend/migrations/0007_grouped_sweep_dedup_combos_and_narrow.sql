-- Shrink the grouped-sweep results tables — the dominant on-disk cost (a single
-- run's `_results` can reach tens of GB because it is `groups × combos × tokens`-
-- shaped). Two independent wins, applied per strategy in ONE table rewrite each
-- (the ALTER below forces a rewrite, which also physically reclaims the dropped
-- `params` column instead of leaving dead space for VACUUM FULL to chase):
--
--   #2  Dedup the swept `params` JSON. `params` is identical for a given
--       `(run_id, combo_id)` across *every* group in a run (the combo set is
--       fixed before the first group folds), yet it was stored on every
--       `(group, combo)` results row — i.e. `group_count`× redundant. Move it to
--       a per-run `<strategy>_grouped_sweep_combos(run_id, combo_id, params)`
--       dictionary and JOIN on `(run_id, combo_id)` to read it back.
--
--   #3  Narrow the per-(group, combo) metric columns. The PnL/score floats are
--       display/ranking-only → `double precision` (8 B) → `real` (4 B, ~7 sig
--       digits, computed in f64 and narrowed only at the storage boundary). The
--       fired/open/closed counts are bounded by a group's token count (≤ corpus,
--       well under 2.1 B) → `bigint` (8 B) → `integer` (4 B).
--
-- Backfill MUST populate `_combos` before dropping `_results.params`. Idempotent
-- (CREATE/DROP IF [NOT] EXISTS, ON CONFLICT) though sqlx runs each migration once.
-- NOTE: the ALTER rewrites the (possibly multi-GB) results table under an ACCESS
-- EXCLUSIVE lock and transiently needs free disk ≈ the table size. Sweeps are an
-- offline, single-flight tool, so the one-time lock is acceptable.

-- ===========================================================================
-- TPSL2
-- ===========================================================================
CREATE TABLE IF NOT EXISTS tpsl2_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl2_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

INSERT INTO tpsl2_grouped_sweep_combos (run_id, combo_id, params)
    SELECT DISTINCT ON (run_id, combo_id) run_id, combo_id, params
    FROM tpsl2_grouped_sweep_results
    ORDER BY run_id, combo_id
ON CONFLICT (run_id, combo_id) DO NOTHING;

ALTER TABLE tpsl2_grouped_sweep_results
    DROP COLUMN IF EXISTS params,
    ALTER COLUMN n_fired             TYPE integer USING n_fired::integer,
    ALTER COLUMN n_open              TYPE integer USING n_open::integer,
    ALTER COLUMN n_closed            TYPE integer USING n_closed::integer,
    ALTER COLUMN win_rate            TYPE real    USING win_rate::real,
    ALTER COLUMN total_pnl_sol       TYPE real    USING total_pnl_sol::real,
    ALTER COLUMN mean_pnl_pct        TYPE real    USING mean_pnl_pct::real,
    ALTER COLUMN median_pnl_pct      TYPE real    USING median_pnl_pct::real,
    ALTER COLUMN p90_pnl_pct         TYPE real    USING p90_pnl_pct::real,
    ALTER COLUMN best_pnl_pct        TYPE real    USING best_pnl_pct::real,
    ALTER COLUMN worst_pnl_pct       TYPE real    USING worst_pnl_pct::real,
    ALTER COLUMN std_pnl_pct         TYPE real    USING std_pnl_pct::real,
    ALTER COLUMN profit_factor       TYPE real    USING profit_factor::real,
    ALTER COLUMN score               TYPE real    USING score::real,
    ALTER COLUMN expectancy_sol      TYPE real    USING expectancy_sol::real,
    ALTER COLUMN avg_holding_secs    TYPE real    USING avg_holding_secs::real,
    ALTER COLUMN median_holding_secs TYPE real    USING median_holding_secs::real;

-- ===========================================================================
-- TPSL1  (identical shape — same generic repo writes both)
-- ===========================================================================
CREATE TABLE IF NOT EXISTS tpsl1_grouped_sweep_combos (
    run_id   UUID    NOT NULL REFERENCES tpsl1_grouped_sweep_runs(id) ON DELETE CASCADE,
    combo_id INTEGER NOT NULL,
    params   JSONB   NOT NULL,
    PRIMARY KEY (run_id, combo_id)
);

INSERT INTO tpsl1_grouped_sweep_combos (run_id, combo_id, params)
    SELECT DISTINCT ON (run_id, combo_id) run_id, combo_id, params
    FROM tpsl1_grouped_sweep_results
    ORDER BY run_id, combo_id
ON CONFLICT (run_id, combo_id) DO NOTHING;

ALTER TABLE tpsl1_grouped_sweep_results
    DROP COLUMN IF EXISTS params,
    ALTER COLUMN n_fired             TYPE integer USING n_fired::integer,
    ALTER COLUMN n_open              TYPE integer USING n_open::integer,
    ALTER COLUMN n_closed            TYPE integer USING n_closed::integer,
    ALTER COLUMN win_rate            TYPE real    USING win_rate::real,
    ALTER COLUMN total_pnl_sol       TYPE real    USING total_pnl_sol::real,
    ALTER COLUMN mean_pnl_pct        TYPE real    USING mean_pnl_pct::real,
    ALTER COLUMN median_pnl_pct      TYPE real    USING median_pnl_pct::real,
    ALTER COLUMN p90_pnl_pct         TYPE real    USING p90_pnl_pct::real,
    ALTER COLUMN best_pnl_pct        TYPE real    USING best_pnl_pct::real,
    ALTER COLUMN worst_pnl_pct       TYPE real    USING worst_pnl_pct::real,
    ALTER COLUMN std_pnl_pct         TYPE real    USING std_pnl_pct::real,
    ALTER COLUMN profit_factor       TYPE real    USING profit_factor::real,
    ALTER COLUMN score               TYPE real    USING score::real,
    ALTER COLUMN expectancy_sol      TYPE real    USING expectancy_sol::real,
    ALTER COLUMN avg_holding_secs    TYPE real    USING avg_holding_secs::real,
    ALTER COLUMN median_holding_secs TYPE real    USING median_holding_secs::real;
