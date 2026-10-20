-- A fingerprint is `(wildcard | criteria OR alt_criteria...) AND NOT exclude_criteria...`.
-- `criteria` stays the first include shape, so every existing row keeps its meaning
-- with both lists empty. Each list is a JSON array of the same `{axis: predicate}`
-- object `criteria` holds, stored in the one canonical order the model writes
-- (`Fingerprint::canonicalize`).
ALTER TABLE fingerprints
    ADD COLUMN IF NOT EXISTS alt_criteria     JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS exclude_criteria JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_shape_lists_are_arrays
    CHECK (jsonb_typeof(alt_criteria) = 'array' AND jsonb_typeof(exclude_criteria) = 'array');

-- A wildcard already includes every token, so an extra include shape beside it is a
-- contradiction; an exclude shape beside it is the point ("every token except ...").
ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_wildcard_excludes_alt_shapes
    CHECK (NOT wildcard OR alt_criteria = '[]'::jsonb);

-- `criteria` is the first include shape: an alternative without it is a list whose
-- head is missing.
ALTER TABLE fingerprints ADD CONSTRAINT fingerprints_alt_shapes_need_a_first_shape
    CHECK (alt_criteria = '[]'::jsonb OR criteria <> '{}'::jsonb);

-- Row identity gains both lists: two rows with the same first shape and different
-- alternatives or exclusions select different tokens. Same digest form as before
-- (a btree row is capped at ~2704 bytes).
DROP INDEX IF EXISTS fingerprints_identity_uniq;
CREATE UNIQUE INDEX IF NOT EXISTS fingerprints_identity_uniq
    ON fingerprints (md5(criteria::text), md5(alt_criteria::text), md5(exclude_criteria::text), wildcard, md5(tags::text));
