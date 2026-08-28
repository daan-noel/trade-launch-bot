-- ===========================================================================
-- 0011  the creation-slot buy total is an AXIS, and only an axis
-- ===========================================================================
-- `m_state.first_slot_buy` leaves the metric registry. It measured the same
-- quantity the fingerprint axis `first_slot_buy_lamports` does, and a fact fixed by
-- the creation slot selects WHICH tokens a rule arms on rather than WHEN it fires --
-- the exact reading that moved `ix_count` and `prior_launches` onto the axes in 0009.
--
-- It survived that move on one argument: a fingerprint pinned a bucket
-- `floor(v/width)`, so a threshold like `>= 6.41` had no axis spelling. 0009 retired
-- the bucket. An `AxisPredicate` is an inclusive `[min, max]` with either bound open,
-- plus `Spans` for `!=` and `|`, so the threshold IS an axis predicate
-- (`{"kind":"range","min":"6410000000"}`) and the axis expresses strictly more than a
-- condition list could. The argument died with the bucket; the metric outlived it.
--
-- The engine reads metric names from `hunter_engine::metrics::REGISTRY`, so a params
-- object naming `first_slot_buy` after this FAILS TO PARSE and its rule stops loading.
-- ===========================================================================

-- ── Rules gated on the retired metric ──────────────────────────────────────
-- One rule, INACTIVE: `6ix crowd-acceleration (runner-up)`, gate `>= 6.41`.
--
-- Its fingerprint (`6ix:Transfer`) is shared by three rules, so folding the axis into
-- that row would silently NARROW the two that never asked for it. The gate is dropped
-- from `params` instead and the rule renamed to say so -- dropping a gate WIDENS a
-- rule, so leaving it unmarked is the dangerous direction. Re-express it as
-- `first_slot_buy_lamports` on a fingerprint of its own before re-activating; the
-- predicate is pinned in `engine/tests/six_ix_cohort_rules.rs::RUNNER_UP_CRITERIA`.
UPDATE strategy_rules SET
    rule_name = rule_name || ' [first_slot_buy gate dropped - re-add as a fingerprint axis]',
    params = jsonb_set(
        params,
        '{entry,m_state}',
        (params #> '{entry,m_state}') - 'first_slot_buy'
    ),
    updated_at = now()
WHERE params #> '{entry,m_state}' ? 'first_slot_buy';

-- An instance emptied by that drop is a group with no conditions, which parses but
-- says nothing. Remove it rather than store a shape no reader has a meaning for.
UPDATE strategy_rules SET
    params = params #- '{entry,m_state}',
    updated_at = now()
WHERE jsonb_typeof(params #> '{entry,m_state}') = 'object'
  AND (params #> '{entry,m_state}') = '{}'::jsonb;

UPDATE strategy_rules SET
    rule_name = rule_name || ' [first_slot_buy gate dropped - re-add as a fingerprint axis]',
    params = jsonb_set(
        params,
        '{exit,m_state}',
        (params #> '{exit,m_state}') - 'first_slot_buy'
    ),
    updated_at = now()
WHERE params #> '{exit,m_state}' ? 'first_slot_buy';

UPDATE strategy_rules SET
    params = params #- '{exit,m_state}',
    updated_at = now()
WHERE jsonb_typeof(params #> '{exit,m_state}') = 'object'
  AND (params #> '{exit,m_state}') = '{}'::jsonb;

-- ── Stored sweeps ──────────────────────────────────────────────────────────
-- A swept `first_slot_buy` axis would no longer parse either. Label rather than
-- rewrite: the numbers were measured through a term the combos can no longer carry,
-- so the run is not comparable to a re-run, and only a human can decide it is worth
-- re-running.
UPDATE grouped_sweep_runs SET
    label = coalesce(label, '')
            || ' [swept a retired first_slot_buy term - re-run before promoting]'
WHERE axes_spec::text LIKE '%first_slot_buy%'
  AND coalesce(label, '') NOT LIKE '%retired first_slot_buy%';

-- ── Guards ─────────────────────────────────────────────────────────────────
-- Nothing may still name the metric. `m_state` is the only group it ever lived in,
-- and `first_slot_buy_lamports` (the axis) is a different key in a different column,
-- so the pattern below cannot match an axis by accident.
DO $$
DECLARE stale int;
BEGIN
    SELECT count(*) INTO stale FROM strategy_rules
    WHERE params::text ~ '"first_slot_buy"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% rule(s) still gate on the retired metric first_slot_buy', stale;
    END IF;

    SELECT count(*) INTO stale FROM grouped_sweep_combos
    WHERE params::text ~ '"first_slot_buy"';
    IF stale > 0 THEN
        RAISE EXCEPTION '% sweep combo(s) still carry the retired metric', stale;
    END IF;

    SELECT count(*) INTO stale FROM strategy_positions
    WHERE exit_reason LIKE '%first\_slot\_buy%';
    IF stale > 0 THEN
        RAISE EXCEPTION '% position(s) record an exit on the retired metric', stale;
    END IF;
END $$;

-- The axis must be untouched: this migration retires a metric, and a fingerprint
-- losing its launch-size criterion would WIDEN every rule on it. 55 rows carry it.
DO $$
DECLARE axes int;
BEGIN
    SELECT count(*) INTO axes FROM fingerprints
    WHERE criteria ? 'first_slot_buy_lamports';
    IF axes = 0 THEN
        RAISE EXCEPTION 'no fingerprint carries first_slot_buy_lamports - the axis was dropped, not the metric';
    END IF;
END $$;
