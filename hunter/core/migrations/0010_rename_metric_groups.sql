-- 0010_rename_metric_groups.sql — backfill renamed metric-group JSON keys inside
-- `strategy_rules.params` (`entry`/`exit` sub-objects).
--
-- WHY. Three metric-group names were clarified because they didn't match what they
-- actually compute (see `hunter/docs/arch/strategies.md`):
--   m_time_window       -> m_flow_window        (it aggregates SOL flow, not time)
--   m_flow_window (old) -> m_flow_split_window   (windowed sibling of m_flow_split;
--                                                  the freed `m_flow_window` name
--                                                  goes to the group above)
--   m_price_path         -> m_price_lifetime      (pairs with m_price_window; "path"
--                                                  didn't signal "lifetime peak")
-- `MetricGroupId` has no serde derive — group identity is persisted as the literal
-- JSON name (`RuleParams::to_value()` writes `group_spec(id).name` as the object key
-- under `entry`/`exit`). A code-only rename makes `RuleParams::parse` reject every
-- stored rule using the old name on next load (`group_by_name` lookup fails), which
-- would break live real-money rules on the next `hunter-live` boot. Ordered so the
-- freed name never collides mid-migration: rename the old `m_flow_window` out of the
-- way first, then promote `m_time_window` into the vacated name.

UPDATE strategy_rules
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (params->'entry')->'m_flow_window'))
WHERE params->'entry' ? 'm_flow_window';

UPDATE strategy_rules
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_flow_window') || jsonb_build_object('m_flow_split_window', (params->'exit')->'m_flow_window'))
WHERE params->'exit' ? 'm_flow_window';

UPDATE strategy_rules
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_time_window') || jsonb_build_object('m_flow_window', (params->'entry')->'m_time_window'))
WHERE params->'entry' ? 'm_time_window';

UPDATE strategy_rules
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_time_window') || jsonb_build_object('m_flow_window', (params->'exit')->'m_time_window'))
WHERE params->'exit' ? 'm_time_window';

UPDATE strategy_rules
SET params = jsonb_set(params, '{entry}', ((params->'entry') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (params->'entry')->'m_price_path'))
WHERE params->'entry' ? 'm_price_path';

UPDATE strategy_rules
SET params = jsonb_set(params, '{exit}', ((params->'exit') - 'm_price_path') || jsonb_build_object('m_price_lifetime', (params->'exit')->'m_price_path'))
WHERE params->'exit' ? 'm_price_path';
