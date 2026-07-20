-- Per-run volume-ix pattern set for volume-flow-split sweeps (V2.2).
--
-- When a sweep axes set references `m_flow_split` / `m_flow_window`, the run
-- carries an optional `volume_ix_patterns` JSON array-of-arrays (ordered label
-- sequences). The fold compiles them corpus-wide into FlowPatterns; Promote
-- writes the same set into the created fingerprint's `metric_config`.
--
-- NULL on legacy / non-flow rows. `IF NOT EXISTS` keeps this idempotent.

ALTER TABLE grouped_sweep_runs
    ADD COLUMN IF NOT EXISTS volume_ix_patterns JSONB;
