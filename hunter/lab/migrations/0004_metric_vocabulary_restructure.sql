-- ===========================================================================
-- 0004  lab: the metric vocabulary restructure, applied to stored sweeps
-- ===========================================================================
-- The core chain's `0010` rewrites live rules; this is the same rewrite over the
-- lab-only sweep tables, which are never created on EC2 (see @arch/database.md).
--
-- Two different kinds of stale here, and only one of them is loud:
--
-- * `grouped_sweep_runs.volume_ix_patterns` is a COLUMN. `GroupedSweepRun` now
--   spells it `ix_patterns`, so every read and write of the table fails outright
--   until this rename lands -- the lab does not start degraded, it errors.
-- * The JSONB specs and params are quiet. A promoted `best_params` naming
--   `m_snapshot` is rejected only when someone tries to save the rule it becomes,
--   which is long after the run that produced it looked fine.
--
-- Runs are NOT re-scored. A stored run's numbers were produced by the engine of
-- its day; renaming the vocabulary it was expressed in does not change what it
-- measured, and re-running is the only thing that would.
-- ===========================================================================

ALTER TABLE grouped_sweep_runs RENAME COLUMN volume_ix_patterns TO ix_patterns;

-- ---------------------------------------------------------------------------
-- The same side-conditions rewrite core `0010` uses. Duplicated rather than
-- shared because the two migration chains are independent by construction (this
-- file never runs on a box that has the core one), and because a migration is a
-- frozen snapshot -- a shared helper that later changed would silently rewrite
-- history differently on two databases.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.lab_migrate_side(side jsonb) RETURNS jsonb AS $$
DECLARE
    out_groups jsonb := '{}'::jsonb;
    grp        text;
    val        jsonb;
    was_array  boolean;
    inst       jsonb;
    new_name   text;
    flow       jsonb := '[]'::jsonb;
    crowd      jsonb := '[]'::jsonb;
    others     jsonb := '{}'::jsonb;
    crowd_inst jsonb;
    flow_inst  jsonb;
    k          text;
    v          jsonb;
    flow_was_array boolean := false;
BEGIN
    IF side IS NULL OR jsonb_typeof(side) <> 'object' THEN
        RETURN side;
    END IF;

    FOR grp, val IN SELECT * FROM jsonb_each(side) LOOP
        was_array := jsonb_typeof(val) = 'array';
        IF NOT was_array THEN
            val := jsonb_build_array(val);
        END IF;

        FOR inst IN SELECT * FROM jsonb_array_elements(val) LOOP
            SELECT COALESCE(jsonb_object_agg(nk, nv), '{}'::jsonb) INTO inst
            FROM (
                SELECT CASE e.key
                           WHEN 'vol_buy'    THEN 'tagged_buy'
                           WHEN 'vol_sell'   THEN 'tagged_sell'
                           WHEN 'vol_net'    THEN 'tagged_net'
                           WHEN 'vol_gross'  THEN 'tagged_gross'
                           WHEN 'vol_share'  THEN 'tagged_share'
                           WHEN 'nonvol_buy'   THEN 'untagged_buy'
                           WHEN 'nonvol_sell'  THEN 'untagged_sell'
                           WHEN 'nonvol_net'   THEN 'untagged_net'
                           WHEN 'nonvol_gross' THEN 'untagged_gross'
                           WHEN 'burst_size_sec'    THEN 'slice_size_sec'
                           WHEN 'burst_size_slots'  THEN 'slice_size_slots'
                           WHEN 'burst_size_prints' THEN 'slice_size_prints'
                           ELSE e.key
                       END AS nk,
                       e.value AS nv
                FROM jsonb_each(inst) AS e
            ) AS renamed;

            -- `ix_count` / `prior_launches` are FINGERPRINT AXES now (core 0009), so
            -- they are not metrics of any group and a params object naming one does
            -- not parse. Core 0009 dropped them from live rules; a stored sweep is
            -- the same stale shape and would fail the save gate at PROMOTION -- long
            -- after the run that produced it looked fine. Dropped here, and the run
            -- is labelled below so nobody promotes it believing it is still the rule
            -- that was fitted.
            inst := inst - 'ix_count' - 'prior_launches';

            IF grp IN ('m_flow_window', 'm_flow_burst') THEN
                crowd_inst := '{}'::jsonb;
                flow_inst  := '{}'::jsonb;
                FOR k, v IN SELECT * FROM jsonb_each(inst) LOOP
                    IF k IN ('unique_wallets', 'trades_per_wallet') THEN
                        crowd_inst := crowd_inst || jsonb_build_object(k, v);
                    ELSIF k IN ('window_size_sec', 'window_size_slots',
                                'window_size_prints', 'window_lag') THEN
                        crowd_inst := crowd_inst || jsonb_build_object(k, v);
                        flow_inst  := flow_inst  || jsonb_build_object(k, v);
                    ELSE
                        flow_inst := flow_inst || jsonb_build_object(k, v);
                    END IF;
                END LOOP;

                IF EXISTS (
                    SELECT 1 FROM jsonb_each(crowd_inst) AS e
                    WHERE e.key IN ('unique_wallets', 'trades_per_wallet')
                ) THEN
                    crowd := crowd || jsonb_build_array(crowd_inst);
                END IF;
                IF EXISTS (
                    SELECT 1 FROM jsonb_each(flow_inst) AS e
                    WHERE e.key NOT IN ('window_size_sec', 'window_size_slots',
                                        'window_size_prints', 'window_lag',
                                        'slice_size_sec', 'slice_size_slots',
                                        'slice_size_prints')
                ) THEN
                    flow := flow || jsonb_build_array(flow_inst);
                END IF;
                IF was_array THEN
                    flow_was_array := true;
                END IF;
            ELSE
                new_name := CASE grp
                                WHEN 'm_snapshot'          THEN 'm_state'
                                WHEN 'm_flow_split'        THEN 'm_flow_ix'
                                WHEN 'm_flow_split_window' THEN 'm_flow_ix_window'
                                ELSE grp
                            END;
                -- A group left with nothing but strict params after the strip
                -- constrains nothing, and `validate_group` rejects it outright.
                IF EXISTS (
                    SELECT 1 FROM jsonb_each(inst) AS e WHERE jsonb_typeof(e.value) = 'array'
                ) THEN
                    others := jsonb_set(
                        others, ARRAY[new_name],
                        COALESCE(others -> new_name, '[]'::jsonb) || jsonb_build_array(inst),
                        true
                    );
                END IF;
                IF was_array THEN
                    others := jsonb_set(others, ARRAY[new_name || '#arr'], 'true'::jsonb, true);
                END IF;
            END IF;
        END LOOP;
    END LOOP;

    FOR grp, val IN SELECT * FROM jsonb_each(others) LOOP
        CONTINUE WHEN grp LIKE '%#arr';
        IF jsonb_array_length(val) = 1 AND (others -> (grp || '#arr')) IS NULL THEN
            out_groups := out_groups || jsonb_build_object(grp, val -> 0);
        ELSE
            out_groups := out_groups || jsonb_build_object(grp, val);
        END IF;
    END LOOP;

    IF jsonb_array_length(flow) = 1 AND NOT flow_was_array THEN
        out_groups := out_groups || jsonb_build_object('m_flow_window', flow -> 0);
    ELSIF jsonb_array_length(flow) > 0 THEN
        out_groups := out_groups || jsonb_build_object('m_flow_window', flow);
    END IF;

    IF jsonb_array_length(crowd) = 1 AND NOT flow_was_array THEN
        out_groups := out_groups || jsonb_build_object('m_crowd_window', crowd -> 0);
    ELSIF jsonb_array_length(crowd) > 0 THEN
        out_groups := out_groups || jsonb_build_object('m_crowd_window', crowd);
    END IF;

    RETURN out_groups;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE OR REPLACE FUNCTION pg_temp.lab_migrate_params(p jsonb) RETURNS jsonb AS $$
DECLARE
    out_p  jsonb := p;
    stages jsonb;
    stage  jsonb;
    acc    jsonb;
BEGIN
    IF p IS NULL OR jsonb_typeof(p) <> 'object' THEN
        RETURN p;
    END IF;
    IF out_p ? 'entry' THEN
        out_p := jsonb_set(out_p, '{entry}', pg_temp.lab_migrate_side(out_p -> 'entry'));
    END IF;
    IF out_p ? 'exit' THEN
        out_p := jsonb_set(out_p, '{exit}', pg_temp.lab_migrate_side(out_p -> 'exit'));
    END IF;
    IF out_p ? 'scale_out' AND jsonb_typeof(out_p -> 'scale_out') = 'array' THEN
        acc := '[]'::jsonb;
        FOR stage IN SELECT * FROM jsonb_array_elements(out_p -> 'scale_out') LOOP
            IF stage ? 'conditions' THEN
                stage := jsonb_set(stage, '{conditions}',
                                   pg_temp.lab_migrate_side(stage -> 'conditions'));
            END IF;
            acc := acc || jsonb_build_array(stage);
        END LOOP;
        out_p := jsonb_set(out_p, '{scale_out}', acc);
    END IF;
    IF out_p ? 'disabled' AND jsonb_typeof(out_p -> 'disabled') = 'object' THEN
        stages := out_p -> 'disabled';
        IF stages ? 'entry' THEN
            stages := jsonb_set(stages, '{entry}', pg_temp.lab_migrate_side(stages -> 'entry'));
        END IF;
        IF stages ? 'exit' THEN
            stages := jsonb_set(stages, '{exit}', pg_temp.lab_migrate_side(stages -> 'exit'));
        END IF;
        IF stages ? 'scale_out' AND jsonb_typeof(stages -> 'scale_out') = 'array' THEN
            acc := '[]'::jsonb;
            FOR stage IN SELECT * FROM jsonb_array_elements(stages -> 'scale_out') LOOP
                IF stage ? 'conditions' THEN
                    stage := jsonb_set(stage, '{conditions}',
                                       pg_temp.lab_migrate_side(stage -> 'conditions'));
                END IF;
                acc := acc || jsonb_build_array(stage);
            END LOOP;
            stages := jsonb_set(stages, '{scale_out}', acc);
        END IF;
        out_p := jsonb_set(out_p, '{disabled}', stages);
    END IF;
    RETURN out_p;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- ---------------------------------------------------------------------------
-- An axis names its group and metric as two flat fields, so the regroup is a
-- rewrite of the PAIR, not of the group alone: a `m_flow_window` axis on
-- `unique_wallets` is an `m_crowd_window` axis now.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.lab_migrate_axes(spec jsonb) RETURNS jsonb AS $$
DECLARE
    axes jsonb := '[]'::jsonb;
    ax   jsonb;
    grp  text;
    met  text;
BEGIN
    IF spec IS NULL OR jsonb_typeof(spec) <> 'object' OR NOT spec ? 'axes' THEN
        RETURN spec;
    END IF;
    FOR ax IN SELECT * FROM jsonb_array_elements(spec -> 'axes') LOOP
        grp := ax ->> 'group';
        met := ax ->> 'metric';
        IF grp IS NOT NULL THEN
            grp := CASE grp
                       WHEN 'm_snapshot'          THEN 'm_state'
                       WHEN 'm_flow_split'        THEN 'm_flow_ix'
                       WHEN 'm_flow_split_window' THEN 'm_flow_ix_window'
                       WHEN 'm_flow_burst'        THEN 'm_flow_window'
                       ELSE grp
                   END;
            IF met IN ('unique_wallets', 'trades_per_wallet') AND grp = 'm_flow_window' THEN
                grp := 'm_crowd_window';
            END IF;
            ax := jsonb_set(ax, '{group}', to_jsonb(grp));
        END IF;
        IF met IS NOT NULL THEN
            met := CASE met
                       WHEN 'vol_buy'      THEN 'tagged_buy'
                       WHEN 'vol_sell'     THEN 'tagged_sell'
                       WHEN 'vol_net'      THEN 'tagged_net'
                       WHEN 'vol_gross'    THEN 'tagged_gross'
                       WHEN 'vol_share'    THEN 'tagged_share'
                       WHEN 'nonvol_buy'   THEN 'untagged_buy'
                       WHEN 'nonvol_sell'  THEN 'untagged_sell'
                       WHEN 'nonvol_net'   THEN 'untagged_net'
                       WHEN 'nonvol_gross' THEN 'untagged_gross'
                       ELSE met
                   END;
            ax := jsonb_set(ax, '{metric}', to_jsonb(met));
        END IF;
        -- An axis on a retired metric swept a dimension that is a fingerprint axis
        -- now. Kept out of the rebuilt list rather than renamed: there is no metric
        -- to point it at.
        IF met IS NULL OR met NOT IN ('ix_count', 'prior_launches') THEN
            axes := axes || jsonb_build_array(ax);
        END IF;
    END LOOP;
    RETURN jsonb_set(spec, '{axes}', axes);
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- A run whose search swept `ix_count` / `prior_launches` was FITTED with a term the
-- rule vocabulary no longer has -- the quantity moved to the fingerprint, so the run
-- cannot be re-expressed as a rule at all. Its stored numbers still describe what it
-- measured, but its params no longer describe the rule that produced them, and
-- promotion would hand someone a strictly WIDER rule than the one that was scored.
-- Marked in the label, the same direction core 0009 marks a live rule it widened.
UPDATE grouped_sweep_runs
SET label = COALESCE(label, '')
    || ' [swept a retired ix_count/prior_launches term - re-run before promoting]'
WHERE axes_spec::text ~ '(ix_count|prior_launches)'
  AND COALESCE(label, '') NOT LIKE '%retired ix_count%';

UPDATE grouped_sweep_runs
SET axes_spec = pg_temp.lab_migrate_axes(axes_spec),
    scale_out = pg_temp.lab_migrate_params(scale_out)
WHERE axes_spec::text ~ '(m_snapshot|m_flow_split|m_flow_burst|vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_|unique_wallets|trades_per_wallet)'
   OR scale_out::text ~ '(m_snapshot|m_flow_split|m_flow_burst|vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_|burst_size_|unique_wallets|trades_per_wallet)';

UPDATE grouped_sweep_combos
SET params = pg_temp.lab_migrate_params(params)
WHERE params::text ~ '(m_snapshot|m_flow_split|m_flow_burst|vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_|burst_size_|unique_wallets|trades_per_wallet)';

UPDATE grouped_sweep_groups
SET best_params = pg_temp.lab_migrate_params(best_params)
WHERE best_params::text ~ '(m_snapshot|m_flow_split|m_flow_burst|vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_|burst_size_|unique_wallets|trades_per_wallet)';

-- ---------------------------------------------------------------------------
-- Guards. A stored sweep is only worth keeping if it can still be PROMOTED, and
-- promotion runs `best_params` through the same save gate a hand-authored rule
-- takes -- so a stale name here is a run whose result can never be acted on.
-- ---------------------------------------------------------------------------
DO $$
DECLARE stale int;
BEGIN
    SELECT count(*) INTO stale FROM grouped_sweep_runs
    WHERE axes_spec::text ~ '"(m_snapshot|m_flow_split|m_flow_split_window|m_flow_burst)"'
       OR axes_spec::text ~ '"(vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_buy|nonvol_sell|nonvol_net|nonvol_gross)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% sweep run(s) still name a retired group or metric', stale;
    END IF;

    SELECT count(*) INTO stale FROM grouped_sweep_groups
    WHERE best_params::text ~ '"(m_snapshot|m_flow_split|m_flow_split_window|m_flow_burst)"'
       OR best_params::text ~ '"burst_size_(sec|slots|prints)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% promotable group result(s) still name a retired group', stale;
    END IF;

    SELECT count(*) INTO stale FROM grouped_sweep_combos
    WHERE params::text ~ '"(m_snapshot|m_flow_split|m_flow_split_window|m_flow_burst)"'
       OR params::text ~ '"burst_size_(sec|slots|prints)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% stored combo(s) still name a retired group', stale;
    END IF;

    -- The metrics core 0009 retired must be gone from every stored shape. A survivor
    -- is a run whose result can never be promoted.
    SELECT count(*) INTO stale FROM grouped_sweep_runs
    WHERE axes_spec::text ~ '"(ix_count|prior_launches)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% sweep run(s) still sweep a retired metric', stale;
    END IF;

    SELECT count(*) INTO stale FROM grouped_sweep_groups
    WHERE best_params::text ~ '"(ix_count|prior_launches)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% group result(s) still gate on a retired metric', stale;
    END IF;

    SELECT count(*) INTO stale FROM grouped_sweep_combos
    WHERE params::text ~ '"(ix_count|prior_launches)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% stored combo(s) still gate on a retired metric', stale;
    END IF;
END $$;
