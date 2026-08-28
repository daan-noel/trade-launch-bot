-- ===========================================================================
-- 0009  fingerprint criteria: explicit integer RANGES, one JSONB column
-- ===========================================================================
-- A fingerprint stops being "nine value columns plus a row-wide bucket width"
-- and becomes one `criteria` map of axis -> predicate:
--
--   {"max_cost_lamports": {"kind":"range","min":"1500000000","max":"1599999999"},
--    "ix_labels":         {"kind":"sequence","labels":["Pump.Fun: Create"]}}
--
-- * A range is INCLUSIVE on both ends, over non-negative integers. Exact match is
--   the degenerate `min = max`, so no row carries a mode flag two readers can
--   disagree about.
-- * Bounds are DECIMAL STRINGS. `max_sol_cost = u64::MAX` is a real launch setting
--   ("fill at any price"), a JSON number is unsafe past 2^53, and a BIGINT column
--   could not hold it at all -- which is the bug that once made one value read as
--   -1 in the engine and +1.84e19 on the dashboard.
-- * A new axis is a registry entry, not a migration: `criteria` is one column.
--
-- The retired `bucket_size_amount` is a WIDTH, i.e. an infinite implicit lattice
-- `floor(v/width)` that the engine, the dashboard SQL and the frontend each had to
-- reproduce identically down to a 1e-9 boundary epsilon. Because lamports are
-- integers, a bucket `[lo, lo+width)` is exactly the inclusive range
-- `[lo, lo+width-1]`, so the backfill below is lossless.
--
-- MEASURED, not assumed: diffed over a 3-day window, 339,700 fingerprint x token match
-- rows, 115 fingerprints. Nothing is lost. THREE rows are gained, on 2 fingerprints at
-- widths 5 and 10 -- and those three are a CORRECTION, not a drift: the retired
-- epsilon was added in ratio units, so it was worth `width * 1e-9` LAMPORTS (5 at
-- width 5, 1000 at width 1000) and filed near-edge values into the next bucket. The
-- integer ranges below have no epsilon to be wrong by.
-- Detail: docs/history/2026-08-27-bucket-epsilon-scaled-with-width.md
--
-- Runs AFTER `0008_pin_the_all_wildcard.sql`, which is written against the columns
-- this migration drops and which collapses the duplicate plain wildcards a box may
-- hold. Reversing the two leaves those duplicates to collide on the identity index
-- below.
-- ===========================================================================

ALTER TABLE fingerprints ADD COLUMN IF NOT EXISTS criteria JSONB NOT NULL DEFAULT '{}'::jsonb;

-- ── Backfill ───────────────────────────────────────────────────────────────
-- All arithmetic in NUMERIC: the widths in use span 1e-5 .. 1000 SOL, and doing
-- this in float8 would reintroduce the very rounding the range model removes.
-- `floor(v/w)*w` is computed on the SOL value the matcher bucketed (lamports/1e9),
-- then converted back to whole lamports.
WITH axis AS (
    SELECT f.id, a.key, a.lamports::numeric AS lamports, f.bucket_size_amount::numeric AS w
    FROM fingerprints f
    CROSS JOIN LATERAL (VALUES
        ('init_buy_lamports',        f.init_buy_lamports),
        ('max_cost_lamports',        f.max_cost_lamports),
        ('spendable_lamports_in',    f.spendable_lamports_in),
        ('first_slot_buy_lamports',  f.first_slot_buy_lamports),
        ('first_slot_sell_lamports', f.first_slot_sell_lamports)
    ) AS a(key, lamports)
    WHERE a.lamports IS NOT NULL
),
windows AS (
    SELECT
        id,
        key,
        CASE
            -- Exact mode (NULL width): the axis named ONE amount.
            WHEN w IS NULL OR w <= 0 THEN lamports
            -- Bucket mode: the window floor, in lamports.
            ELSE floor((lamports / 1e9) / w) * w * 1e9
        END::numeric(40, 0) AS lo,
        CASE
            WHEN w IS NULL OR w <= 0 THEN lamports
            -- The last lamport strictly below the next window's floor.
            ELSE (floor((lamports / 1e9) / w) + 1) * w * 1e9 - 1
        END::numeric(40, 0) AS hi
    FROM axis
),
sol_axes AS (
    SELECT id, jsonb_object_agg(
        key,
        jsonb_build_object('kind', 'range', 'min', lo::text, 'max', hi::text)
    ) AS obj
    FROM windows
    GROUP BY id
)
UPDATE fingerprints f SET criteria =
      -- The two exact-integer axes were never bucketed, so they carry across as
      -- degenerate ranges.
      CASE WHEN f.cu_limit IS NULL THEN '{}'::jsonb ELSE jsonb_build_object(
           'cu_limit', jsonb_build_object('kind','range','min',f.cu_limit::text,'max',f.cu_limit::text)) END
   || CASE WHEN f.cu_price IS NULL THEN '{}'::jsonb ELSE jsonb_build_object(
           'cu_price', jsonb_build_object('kind','range','min',f.cu_price::text,'max',f.cu_price::text)) END
   || COALESCE(s.obj, '{}'::jsonb)
      -- An EMPTY label array is the same sentinel as absent (the matcher's own
      -- `configured_labels` rule), so it must not become a configured axis here.
   || CASE WHEN f.ix_labels IS NULL OR cardinality(f.ix_labels) = 0 THEN '{}'::jsonb
           ELSE jsonb_build_object('ix_labels',
                jsonb_build_object('kind','sequence','labels', to_jsonb(f.ix_labels))) END
FROM (SELECT id FROM fingerprints) t
LEFT JOIN sol_axes s ON s.id = t.id
WHERE f.id = t.id
  -- A wildcard row carries no axis by construction; leave its criteria empty.
  AND NOT f.wildcard;

-- ── Guard the backfill ─────────────────────────────────────────────────────
-- Every non-wildcard row that had at least one axis must come out with at least
-- one criterion. A row that lost its axes would match NOTHING while still reading
-- as configured, silently disarming every rule bound to it.
DO $$
DECLARE lost int;
BEGIN
    SELECT count(*) INTO lost FROM fingerprints
    WHERE NOT wildcard
      AND criteria = '{}'::jsonb
      AND (cu_limit IS NOT NULL OR cu_price IS NOT NULL OR init_buy_lamports IS NOT NULL
           OR max_cost_lamports IS NOT NULL OR spendable_lamports_in IS NOT NULL
           OR first_slot_buy_lamports IS NOT NULL OR first_slot_sell_lamports IS NOT NULL
           OR (ix_labels IS NOT NULL AND cardinality(ix_labels) > 0));
    IF lost > 0 THEN
        RAISE EXCEPTION 'criteria backfill dropped every axis on % fingerprint row(s)', lost;
    END IF;
END $$;

-- ── Rules gated on the two retired metrics ─────────────────────────────────
-- `m_snapshot.ix_count` and `m_snapshot.prior_launches` are fingerprint AXES now,
-- not metrics: both are fixed at creation, so they select WHICH tokens a rule arms
-- on, and a fact belongs to one vocabulary only.
--
-- Every affected rule is INACTIVE, and each shares its fingerprint with others, so
-- folding the gate into the shared row would silently narrow rules that never
-- asked for it. The gate is dropped from `params` instead and the rule is renamed
-- to say so -- dropping a gate WIDENS a rule, so leaving it unmarked would be the
-- dangerous direction. Re-express it as a fingerprint axis before re-activating.
--
-- The group is `m_snapshot` here and only here: `0010` renames it to `m_state`, and
-- this migration runs before it. Naming it `m_state` matches nothing, so the gate
-- survives the drop and `0010` carries it into `m_state` -- where no metric answers
-- to it and no guard in either migration reports it.
UPDATE strategy_rules SET
    rule_name = rule_name || ' [ix_count/prior_launches gate dropped - re-add as a fingerprint axis]',
    params = jsonb_set(
        params,
        '{entry,m_snapshot}',
        (params #> '{entry,m_snapshot}') - 'ix_count' - 'prior_launches'
    ),
    updated_at = now()
WHERE params #> '{entry,m_snapshot}' ?| ARRAY['ix_count', 'prior_launches'];

DO $$
DECLARE still int;
BEGIN
    SELECT count(*) INTO still FROM strategy_rules
    WHERE is_active AND params::text LIKE '%prior_launches%' OR is_active AND params::text LIKE '%ix_count%';
    IF still > 0 THEN
        RAISE EXCEPTION '% ACTIVE rule(s) still gate on a retired metric', still;
    END IF;
END $$;

-- ── Drop the retired shape ─────────────────────────────────────────────────
ALTER TABLE fingerprints DROP CONSTRAINT IF EXISTS fingerprints_bucket_size_amount_positive;
ALTER TABLE fingerprints DROP CONSTRAINT IF EXISTS fingerprints_bucket_width_needs_a_sol_axis;
ALTER TABLE fingerprints DROP CONSTRAINT IF EXISTS fingerprints_wildcard_excludes_axes;

ALTER TABLE fingerprints
    DROP COLUMN IF EXISTS cu_limit,
    DROP COLUMN IF EXISTS cu_price,
    DROP COLUMN IF EXISTS init_buy_lamports,
    DROP COLUMN IF EXISTS max_cost_lamports,
    DROP COLUMN IF EXISTS spendable_lamports_in,
    DROP COLUMN IF EXISTS first_slot_buy_lamports,
    DROP COLUMN IF EXISTS first_slot_sell_lamports,
    DROP COLUMN IF EXISTS bucket_size_amount,
    DROP COLUMN IF EXISTS ix_labels;

ALTER TABLE fingerprints ALTER COLUMN criteria DROP DEFAULT;

-- ── Invariants the model also enforces (`Fingerprint::validate`) ───────────
-- A wildcard already answers the match for every token, so an axis beside it is a
-- contradiction the matcher resolves silently in favour of the wildcard.
ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_wildcard_excludes_axes
    CHECK (NOT wildcard OR criteria = '{}'::jsonb);

-- An unconfigured row matches NOTHING on purpose (a half-filled form must not arm
-- on every token), so "every token" has to be spelled `wildcard` out loud.
ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_has_a_criterion
    CHECK (wildcard OR criteria <> '{}'::jsonb);

ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_criteria_is_an_object
    CHECK (jsonb_typeof(criteria) = 'object');

-- A collision here is a real finding, not a migration accident: two rows that were
-- distinct under the old per-axis columns describe the SAME token set under ranges
-- (bucket mode filed several raw amounts into one window). Name them rather than let
-- `CREATE UNIQUE INDEX` fail with an opaque "could not create unique index".
DO $$
DECLARE dupes text;
BEGIN
    SELECT string_agg(row_names, ' ; ') INTO dupes FROM (
        SELECT string_agg(name || ' (' || id || ')', ' = ' ORDER BY name) AS row_names
        FROM fingerprints GROUP BY criteria, wildcard, metric_config HAVING count(*) > 1
    ) x;
    IF dupes IS NOT NULL THEN
        RAISE EXCEPTION 'these fingerprint rows collapse onto one identity under ranges: %', dupes
        USING HINT = 'merge them and retarget their rules first, the way 0006-0008 do';
    END IF;
END $$;

-- ROW identity is the criteria, the wildcard flag AND `metric_config`. A UNIQUE
-- index makes the duplicate-fingerprint class impossible at the storage layer instead
-- of relying on every writer routing through `find_or_create` -- `jsonb` equality is
-- canonical (Postgres normalises key order), so two rows naming the same axes and the
-- same config collide here.
--
-- `metric_config` is in the key even though it is NOT match identity, because it is
-- LIVE per-row behaviour: `EngineState` compiles it into that fingerprint's
-- `m_flow_split` patterns keyed by `fp.id` at reload. Two rows selecting the same
-- tokens with different patterns classify flow differently, so they are different
-- fingerprints and must both exist -- which is exactly what `0006`-`0008` preserved
-- for the ten `8dtx - <router>` carriers and the `8dtx-derived` classifier. Keying
-- without it would have merged those eleven rows into the plain `ALL` wildcard and
-- silently rewritten the flow classification of every rule bound to them.
--
-- Indexed as DIGESTS, not raw values: a btree row is capped at ~2704 bytes and the
-- `8dtx` carriers' `volume_ix_patterns` are several KB on their own, so the plain
-- three-column index cannot be built at all. `jsonb` is stored decomposed and its
-- text output is deterministic, so equal `jsonb` always yields equal `md5` -- the
-- constraint is exactly as strict, and a digest collision could only REJECT a write,
-- never admit a duplicate.
--
-- `IDENTITY_WHERE` deliberately keeps comparing the values themselves. Reads stay
-- exact; only the write-side constraint is conservative.
CREATE UNIQUE INDEX IF NOT EXISTS fingerprints_identity_uniq
    ON fingerprints (md5(criteria::text), wildcard, md5(metric_config::text));
