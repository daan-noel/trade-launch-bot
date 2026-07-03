-- tpsl2 scalp-entry param cleanup (see @plans/tpsl-strategy/tpsl2-entry-exit-params.md).
--
-- Rules persist their gate knobs inside the generic `strategy_rules.params` JSONB
-- blob (there are NO physical p_entry_* columns), so this is a JSONB-key migration,
-- not an ALTER COLUMN. Runs on live + lab (shared core DB) at boot.
--
-- 1) Rename the mislabeled demand gate. `p_entry_min_organic_sol` measured net SOL
--    BOUGHT (Σbuys − Σsells) — a demand *flow*, NOT pool liquidity — so it is
--    renamed to `p_entry_min_net_buy_sol` to say what it actually is.
-- 2) Drop `p_entry_min_organic_liq`. It read the exact same `real_reserve_sol`
--    snapshot as `p_entry_min_liquidity_sol`, so a single rule's effective floor
--    was always max(liq, organic_liq) — a pure duplicate. `p_entry_min_liquidity_sol`
--    stays as the one real-reserves floor.
--
-- Applied to both the live rules (`strategy_rules.params`) and the historical run
-- snapshots (`strategy_runs.params_snapshot`) so old rows read back consistently.
-- Idempotent via the `?` key guard.

UPDATE strategy_rules
SET params = (params - 'p_entry_min_organic_sol')
             || jsonb_build_object('p_entry_min_net_buy_sol', params -> 'p_entry_min_organic_sol')
WHERE params ? 'p_entry_min_organic_sol';

UPDATE strategy_rules
SET params = params - 'p_entry_min_organic_liq'
WHERE params ? 'p_entry_min_organic_liq';

UPDATE strategy_runs
SET params_snapshot = (params_snapshot - 'p_entry_min_organic_sol')
                      || jsonb_build_object('p_entry_min_net_buy_sol', params_snapshot -> 'p_entry_min_organic_sol')
WHERE params_snapshot ? 'p_entry_min_organic_sol';

UPDATE strategy_runs
SET params_snapshot = params_snapshot - 'p_entry_min_organic_liq'
WHERE params_snapshot ? 'p_entry_min_organic_liq';
