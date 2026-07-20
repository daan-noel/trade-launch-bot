-- 0005_retire_legacy_sweep_tables.sql — drop the retired per-strategy sweep tables (Phase 7).
--
-- The grouped sweep is generic-only now: `sweep::registry::sweep_tables` resolves
-- exclusively to the unprefixed `grouped_sweep_*` tables (`"generic"`), and the
-- per-strategy sweep families (tpsl1 / tpsl2 / swing_1) were deleted from the lab
-- code in Phase 7. Their result tables (created by `0001_grouped_sweep.sql`) are
-- now unreferenced dead weight — drop them.
--
-- `CASCADE` because the `_groups`/`_combos`/`_results` children FK back to the
-- `_runs` parent; order-independent with the cascade.

DROP TABLE IF EXISTS tpsl1_grouped_sweep_results CASCADE;
DROP TABLE IF EXISTS tpsl1_grouped_sweep_combos  CASCADE;
DROP TABLE IF EXISTS tpsl1_grouped_sweep_groups  CASCADE;
DROP TABLE IF EXISTS tpsl1_grouped_sweep_runs    CASCADE;

DROP TABLE IF EXISTS tpsl2_grouped_sweep_results CASCADE;
DROP TABLE IF EXISTS tpsl2_grouped_sweep_combos  CASCADE;
DROP TABLE IF EXISTS tpsl2_grouped_sweep_groups  CASCADE;
DROP TABLE IF EXISTS tpsl2_grouped_sweep_runs    CASCADE;

DROP TABLE IF EXISTS swing_1_grouped_sweep_results CASCADE;
DROP TABLE IF EXISTS swing_1_grouped_sweep_combos  CASCADE;
DROP TABLE IF EXISTS swing_1_grouped_sweep_groups  CASCADE;
DROP TABLE IF EXISTS swing_1_grouped_sweep_runs    CASCADE;
