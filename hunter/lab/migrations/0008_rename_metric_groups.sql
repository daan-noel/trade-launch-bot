-- 0008_rename_metric_groups.sql — backfill renamed metric-group names inside the
-- generic grouped-sweep tables (`grouped_sweep_runs.axes_spec`,
-- `grouped_sweep_groups.best_params`, `grouped_sweep_combos.params`).
--
-- WHY. Companion to `hunter/core/migrations/0010_rename_metric_groups.sql` — same
-- three renames (see that file's header for the full rationale):
--   m_time_window       -> m_flow_window
--   m_flow_window (old) -> m_flow_split_window
--   m_price_path         -> m_price_lifetime
--
-- Two different persisted shapes need two different SQL patterns:
--  * `best_params` / `params` are `RuleParams::to_value()` output — the group name
--    is a JSON object KEY under `entry`/`exit` (same shape as `strategy_rules.params`).
--  * `axes_spec` is the raw sweep-request `{"axes": [...]}` body — the group name is
--    a plain string VALUE of each axis element's `"group"` field, not a key.
-- Ordered so the freed `m_flow_window` name never collides mid-migration: move the
-- old `m_flow_window` out of the way first, then promote `m_time_window` into it.

-- ── grouped_sweep_groups.best_params / grouped_sweep_combos.params (key rename) ──

UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{entry}', ((best_params->'entry') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (best_params->'entry')->'m_flow_window'))
WHERE best_params->'entry' ? 'm_flow_window';
UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{exit}', ((best_params->'exit') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (best_params->'exit')->'m_flow_window'))
WHERE best_params->'exit' ? 'm_flow_window';
UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{entry}', ((best_params->'entry') - 'm_time_window') || jsonb_build_object('m_flow_window', (best_params->'entry')->'m_time_window'))
WHERE best_params->'entry' ? 'm_time_window';
UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{exit}', ((best_params->'exit') - 'm_time_window') || jsonb_build_object('m_flow_window', (best_params->'exit')->'m_time_window'))
WHERE best_params->'exit' ? 'm_time_window';
UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{entry}', ((best_params->'entry') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (best_params->'entry')->'m_price_path'))
WHERE best_params->'entry' ? 'm_price_path';
UPDATE grouped_sweep_groups
SET best_params = jsonb_set(best_params, '{exit}', ((best_params->'exit') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (best_params->'exit')->'m_price_path'))
WHERE best_params->'exit' ? 'm_price_path';

UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (params->'entry')->'m_flow_window'))
WHERE params->'entry' ? 'm_flow_window';
UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (params->'exit')->'m_flow_window'))
WHERE params->'exit' ? 'm_flow_window';
UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_time_window') || jsonb_build_object('m_flow_window', (params->'entry')->'m_time_window'))
WHERE params->'entry' ? 'm_time_window';
UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_time_window') || jsonb_build_object('m_flow_window', (params->'exit')->'m_time_window'))
WHERE params->'exit' ? 'm_time_window';
UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (params->'entry')->'m_price_path'))
WHERE params->'entry' ? 'm_price_path';
UPDATE grouped_sweep_combos
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (params->'exit')->'m_price_path'))
WHERE params->'exit' ? 'm_price_path';

-- ── grouped_sweep_runs.axes_spec (string-value rename inside the `axes` array) ──

UPDATE grouped_sweep_runs
SET axes_spec = jsonb_set(
    axes_spec,
    '{axes}',
    (SELECT jsonb_agg(
        CASE WHEN elem->>'group' = 'm_flow_window' THEN jsonb_set(elem, '{group}', '"m_flow_split_window"'::jsonb)
             ELSE elem END
        ORDER BY ord
    ) FROM jsonb_array_elements(axes_spec->'axes') WITH ORDINALITY AS t(elem, ord))
)
WHERE axes_spec ? 'axes'
  AND EXISTS (SELECT 1 FROM jsonb_array_elements(axes_spec->'axes') e WHERE e->>'group' = 'm_flow_window');

UPDATE grouped_sweep_runs
SET axes_spec = jsonb_set(
    axes_spec,
    '{axes}',
    (SELECT jsonb_agg(
        CASE WHEN elem->>'group' = 'm_time_window' THEN jsonb_set(elem, '{group}', '"m_flow_window"'::jsonb)
             ELSE elem END
        ORDER BY ord
    ) FROM jsonb_array_elements(axes_spec->'axes') WITH ORDINALITY AS t(elem, ord))
)
WHERE axes_spec ? 'axes'
  AND EXISTS (SELECT 1 FROM jsonb_array_elements(axes_spec->'axes') e WHERE e->>'group' = 'm_time_window');

UPDATE grouped_sweep_runs
SET axes_spec = jsonb_set(
    axes_spec,
    '{axes}',
    (SELECT jsonb_agg(
        CASE WHEN elem->>'group' = 'm_price_path' THEN jsonb_set(elem, '{group}', '"m_price_lifetime"'::jsonb)
             ELSE elem END
        ORDER BY ord
    ) FROM jsonb_array_elements(axes_spec->'axes') WITH ORDINALITY AS t(elem, ord))
)
WHERE axes_spec ? 'axes'
  AND EXISTS (SELECT 1 FROM jsonb_array_elements(axes_spec->'axes') e WHERE e->>'group' = 'm_price_path');
