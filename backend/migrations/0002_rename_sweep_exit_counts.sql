-- ============================================================================
-- Rename the grouped-sweep exit-reason *count* columns with an `n_` prefix.
--
-- These columns hold per-reason trade counts (how a combo's closed trades
-- terminated), not param values — but the bare `exit_take_profit` /
-- `exit_stop_loss` names collided with the TP/SL *threshold* knobs carried in
-- the `params` JSONB, which was confusing on the wire and in the UI. The `n_`
-- prefix matches the existing count columns (n_fired / n_open / n_closed).
--
-- Idempotent: each rename only fires if the old column still exists, so the
-- migration is a no-op on a DB created from the (already-renamed) squashed init.
-- Only tpsl2 has a grouped-sweep results table today; add sibling blocks here if
-- another strategy's `*_grouped_sweep_results` table is introduced.
-- ============================================================================

DO $$
DECLARE
    old_to_new text[][] := ARRAY[
        ['exit_take_profit', 'n_exit_take_profit'],
        ['exit_stop_loss',   'n_exit_stop_loss'],
        ['exit_trailing',    'n_exit_trailing'],
        ['exit_stall',       'n_exit_stall'],
        ['exit_time',        'n_exit_time'],
        ['exit_liquidity',   'n_exit_liquidity'],
        ['exit_cohort',      'n_exit_cohort'],
        ['exit_open',        'n_exit_open']
    ];
    pair text[];
BEGIN
    FOREACH pair SLICE 1 IN ARRAY old_to_new LOOP
        IF EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'tpsl2_grouped_sweep_results'
              AND column_name = pair[1]
        ) THEN
            EXECUTE format(
                'ALTER TABLE tpsl2_grouped_sweep_results RENAME COLUMN %I TO %I',
                pair[1], pair[2]
            );
        END IF;
    END LOOP;
END $$;
