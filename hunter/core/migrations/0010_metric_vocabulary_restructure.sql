-- ===========================================================================
-- 0010  the metric vocabulary: one name per thing
-- ===========================================================================
-- Every stored rule, exit reason and fingerprint config is rewritten into the
-- restructured registry. The engine reads group and metric names from
-- `hunter_engine::metrics::REGISTRY`, so a params object naming a group that no
-- longer exists FAILS TO PARSE and its rule stops loading -- there is no version
-- of this that can be skipped and no partial state that is safe.
--
-- What changes, and why each one:
--
-- * `m_snapshot` -> `m_state`. What is left of the group after `ix_count` and
--   `prior_launches` became fingerprint axes (0009) is token STATE: age,
--   liquidity, and the launch total. "Snapshot" named the implementation.
--
-- * `m_flow_split` -> `m_flow_ix`, `m_flow_split_window` -> `m_flow_ix_window`,
--   and `vol_*` / `nonvol_*` -> `tagged_*` / `untagged_*`. The split is by
--   INSTRUCTION STRUCTURE, and "volume" vs "organic" named one use of it while
--   reading as the unrelated quantity `gross_flow` measures.
--
-- * `m_flow_burst` DISSOLVES into `m_flow_window`, and `burst_size_*` becomes
--   `slice_size_*`. Its one metric is a ratio over the same tape across two spans,
--   not a different subject, so it was a second group for one family. The second
--   axis is now declared by `m_flow_window` and required PER METRIC
--   (`is_two_window`), which is what keeps it off the instances that never read it.
--
-- * `unique_wallets` / `trades_per_wallet` LEAVE `m_flow_window` for a new
--   `m_crowd_window`, carrying their instance's window params. Not cosmetic: those
--   two are the only metrics that need the lake's wallet column, and an offline read
--   without it folds every trade as one anonymous wallet -- so the gate reads false
--   forever and the run looks strict rather than broken. One group, one load
--   obligation.
--
-- A rule that gated on flow AND crowd metrics in one instance therefore comes out
-- as two instances over the same window. That is the same set of conditions: both
-- groups' instances are ANDed, and each keeps the window it was authored at.
--
-- Exit reasons carry the METRIC name only (`event::format_metric_exit_name`), so
-- only the `vol_*`/`nonvol_*` renames touch them; a moved group leaves the label
-- alone.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- One side-conditions bag: `{group: instance | [instance, ...]}`.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.migrate_side(side jsonb) RETURNS jsonb AS $$
DECLARE
    out_groups jsonb := '{}'::jsonb;
    grp        text;
    val        jsonb;
    was_array  boolean;
    inst       jsonb;
    new_name   text;
    flow       jsonb := '[]'::jsonb;   -- m_flow_window instances (incl. dissolved burst)
    crowd      jsonb := '[]'::jsonb;   -- m_crowd_window instances split out of them
    others     jsonb := '{}'::jsonb;
    crowd_inst jsonb;
    flow_inst  jsonb;
    k          text;
    v          jsonb;
    flow_was_array  boolean := false;
    crowd_from_flow boolean := false;
BEGIN
    IF side IS NULL OR jsonb_typeof(side) <> 'object' THEN
        RETURN side;
    END IF;

    FOR grp, val IN SELECT * FROM jsonb_each(side) LOOP
        was_array := jsonb_typeof(val) = 'array';
        IF NOT was_array THEN
            val := jsonb_build_array(val);
        END IF;

        -- The metric-key renames are global: `vol_*` only ever appeared in the two
        -- split groups, so applying them everywhere cannot hit an unrelated key.
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

            IF grp IN ('m_flow_window', 'm_flow_burst') THEN
                -- Split the wallet-keyed metrics out, keeping this instance's window
                -- params on BOTH sides so each reads the span it was authored at.
                crowd_inst := '{}'::jsonb;
                flow_inst  := '{}'::jsonb;
                FOR k, v IN SELECT * FROM jsonb_each(inst) LOOP
                    IF k IN ('unique_wallets', 'trades_per_wallet') THEN
                        crowd_inst := crowd_inst || jsonb_build_object(k, v);
                    ELSIF k IN ('window_size_sec', 'window_size_slots',
                                'window_size_prints', 'window_lag') THEN
                        -- A window param belongs to whichever halves survive.
                        crowd_inst := crowd_inst || jsonb_build_object(k, v);
                        flow_inst  := flow_inst  || jsonb_build_object(k, v);
                    ELSE
                        flow_inst := flow_inst || jsonb_build_object(k, v);
                    END IF;
                END LOOP;

                -- Keep a half only if it still carries a metric condition; a bag of
                -- nothing but window params is a group with no conditions, which
                -- `validate_group` rejects outright.
                IF EXISTS (
                    SELECT 1 FROM jsonb_each(crowd_inst) AS e
                    WHERE e.key IN ('unique_wallets', 'trades_per_wallet')
                ) THEN
                    crowd := crowd || jsonb_build_array(crowd_inst);
                    crowd_from_flow := true;
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
                                WHEN 'm_snapshot'           THEN 'm_state'
                                WHEN 'm_flow_split'         THEN 'm_flow_ix'
                                WHEN 'm_flow_split_window'  THEN 'm_flow_ix_window'
                                ELSE grp
                            END;
                others := jsonb_set(
                    others,
                    ARRAY[new_name],
                    COALESCE(others -> new_name, '[]'::jsonb) || jsonb_build_array(inst),
                    true
                );
                IF was_array THEN
                    others := jsonb_set(others, ARRAY[new_name || '#arr'], 'true'::jsonb, true);
                END IF;
            END IF;
        END LOOP;
    END LOOP;

    -- Collapse each group back to a bare object when it holds exactly one instance
    -- and was not authored as an array; both spellings parse, and this keeps a rule
    -- that never had an array from growing one.
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

-- ---------------------------------------------------------------------------
-- A whole params object: the six paths a side-conditions bag can sit at.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.migrate_params(p jsonb) RETURNS jsonb AS $$
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
        out_p := jsonb_set(out_p, '{entry}', pg_temp.migrate_side(out_p -> 'entry'));
    END IF;
    IF out_p ? 'exit' THEN
        out_p := jsonb_set(out_p, '{exit}', pg_temp.migrate_side(out_p -> 'exit'));
    END IF;

    -- The ladder: each stage carries its own bag under `conditions`.
    IF out_p ? 'scale_out' AND jsonb_typeof(out_p -> 'scale_out') = 'array' THEN
        acc := '[]'::jsonb;
        FOR stage IN SELECT * FROM jsonb_array_elements(out_p -> 'scale_out') LOOP
            IF stage ? 'conditions' THEN
                stage := jsonb_set(stage, '{conditions}',
                                   pg_temp.migrate_side(stage -> 'conditions'));
            END IF;
            acc := acc || jsonb_build_array(stage);
        END LOOP;
        out_p := jsonb_set(out_p, '{scale_out}', acc);
    END IF;

    -- The parked half is the same shape, and it is validated like a live side --
    -- so a group name left stale here fails the save exactly as a live one would.
    IF out_p ? 'disabled' AND jsonb_typeof(out_p -> 'disabled') = 'object' THEN
        stages := out_p -> 'disabled';
        IF stages ? 'entry' THEN
            stages := jsonb_set(stages, '{entry}', pg_temp.migrate_side(stages -> 'entry'));
        END IF;
        IF stages ? 'exit' THEN
            stages := jsonb_set(stages, '{exit}', pg_temp.migrate_side(stages -> 'exit'));
        END IF;
        IF stages ? 'scale_out' AND jsonb_typeof(stages -> 'scale_out') = 'array' THEN
            acc := '[]'::jsonb;
            FOR stage IN SELECT * FROM jsonb_array_elements(stages -> 'scale_out') LOOP
                IF stage ? 'conditions' THEN
                    stage := jsonb_set(stage, '{conditions}',
                                       pg_temp.migrate_side(stage -> 'conditions'));
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

-- -- Rules ------------------------------------------------------------------
UPDATE strategy_rules
SET params = pg_temp.migrate_params(params),
    updated_at = now()
WHERE params::text ~ '(m_snapshot|m_flow_split|m_flow_burst|vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_|burst_size_|unique_wallets|trades_per_wallet)';

-- -- Exit reasons -----------------------------------------------------------
-- `strategy_positions.exit_reason` is the stored label (`strategy_position_pnl` is a
-- VIEW over it and follows). It carries the METRIC name only
-- (`event::format_metric_exit_name`), so a moved group is invisible here and only the
-- split renames touch it.
--
-- `nonvol_` FIRST: it contains `vol_`, and a plain left-to-right replace would turn
-- `nonvol_buy` into `nontagged_buy`.
UPDATE strategy_positions
SET exit_reason = regexp_replace(
        regexp_replace(exit_reason, 'nonvol_', 'untagged_', 'g'),
        'vol_', 'tagged_', 'g')
WHERE exit_reason LIKE '%vol\_%';

-- -- Fingerprint metric config ----------------------------------------------
-- `metric_config` is keyed by GROUP name and compiled into the classifier at
-- reload, so a stale key here does not fail loudly -- it silently configures
-- nothing and every `m_flow_ix` metric reads NaN.
UPDATE fingerprints
SET metric_config = (
        SELECT jsonb_object_agg(
                   CASE e.key WHEN 'm_flow_split' THEN 'm_flow_ix' ELSE e.key END,
                   (
                       SELECT COALESCE(jsonb_object_agg(
                           CASE f.key
                               WHEN 'volume_ix_patterns' THEN 'ix_patterns'
                               WHEN 'volume_ix_markers'  THEN 'tagged_ix_markers'
                               WHEN 'organic_ix_markers' THEN 'untagged_ix_markers'
                               WHEN 'creator_is_volume'  THEN 'creator_is_tagged'
                               ELSE f.key
                           END, f.value), '{}'::jsonb)
                       FROM jsonb_each(e.value) AS f
                   )
               )
        FROM jsonb_each(metric_config) AS e
    ),
    updated_at = now()
WHERE metric_config::text ~ '(m_flow_split|volume_ix_|organic_ix_markers|creator_is_volume)';

-- -- Guards -----------------------------------------------------------------
-- Nothing may still name a retired group or metric. These are the exact strings
-- `RuleParams::parse` rejects, so a survivor here is a rule that stops loading.
DO $$
DECLARE stale int;
BEGIN
    SELECT count(*) INTO stale FROM strategy_rules
    WHERE params::text ~ '"(m_snapshot|m_flow_split|m_flow_split_window|m_flow_burst)"'
       OR params::text ~ '"(vol_buy|vol_sell|vol_net|vol_gross|vol_share|nonvol_buy|nonvol_sell|nonvol_net|nonvol_gross)"'
       OR params::text ~ '"burst_size_(sec|slots|prints)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% rule(s) still name a retired group, metric or param', stale;
    END IF;

    SELECT count(*) INTO stale FROM strategy_positions WHERE exit_reason LIKE '%vol\_%';
    IF stale > 0 THEN
        RAISE EXCEPTION '% position(s) still record a retired metric name', stale;
    END IF;

    SELECT count(*) INTO stale FROM fingerprints
    WHERE metric_config::text ~ '"(m_flow_split|volume_ix_patterns|volume_ix_markers|organic_ix_markers|creator_is_volume)"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% fingerprint(s) still carry a retired metric_config key', stale;
    END IF;
END $$;

-- The wallet-keyed metrics must have LANDED somewhere, not been dropped: every rule
-- that named one before still names one, in `m_crowd_window` now. A rule silently
-- losing a gate is the failure mode this whole migration is most exposed to, since
-- the result still parses and still trades.
DO $$
DECLARE orphan int;
BEGIN
    SELECT count(*) INTO orphan FROM strategy_rules
    WHERE params::text ~ '"(unique_wallets|trades_per_wallet)"'
      AND params::text NOT LIKE '%m_crowd_window%';
    IF orphan > 0 THEN
        RAISE EXCEPTION
            '% rule(s) still hold a wallet-keyed metric outside m_crowd_window', orphan;
    END IF;
END $$;
