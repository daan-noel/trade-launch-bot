-- Grouped-sweep finding #2/#3: the group's headline pick now ranks on the
-- robust *realized* score (μ−Z·σ/√n over closed trades) gated by a coverage
-- floor — the same metric the drill-in combo table sorts by — instead of raw
-- expectancy (a variance-blind mean that folded in open-position mark-to-last).
-- Persist that score per group so the group list can order + display it.
--
-- Nullable: a winning combo with < 2 closed trades has no score (NULL), and any
-- rows written before this migration keep NULL (they predate the metric).

ALTER TABLE tpsl2_grouped_sweep_groups
    ADD COLUMN IF NOT EXISTS best_score DOUBLE PRECISION;

ALTER TABLE tpsl1_grouped_sweep_groups
    ADD COLUMN IF NOT EXISTS best_score DOUBLE PRECISION;
