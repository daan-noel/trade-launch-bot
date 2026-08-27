-- A fingerprint that matches every token.
--
-- The matcher refuses a row with no configured axis (an all-NULL row must not arm
-- on everything), so a rule whose decision is entirely tape-side has no way to say
-- "any token" through the existing axes. This spells it, rather than overloading
-- an unset axis with a second meaning.
ALTER TABLE fingerprints
    ADD COLUMN IF NOT EXISTS wildcard BOOLEAN NOT NULL DEFAULT FALSE;

-- A wildcard row ignores every other axis, so carrying axes alongside it is a
-- contradiction the matcher would silently resolve in favour of the wildcard.
ALTER TABLE fingerprints
    DROP CONSTRAINT IF EXISTS fingerprints_wildcard_excludes_axes;
ALTER TABLE fingerprints
    ADD CONSTRAINT fingerprints_wildcard_excludes_axes CHECK (
        NOT wildcard OR (
            cu_limit IS NULL
            AND cu_price IS NULL
            AND init_buy_lamports IS NULL
            AND max_cost_lamports IS NULL
            AND spendable_lamports_in IS NULL
            AND first_slot_buy_lamports IS NULL
            AND first_slot_sell_lamports IS NULL
            AND (ix_labels IS NULL OR cardinality(ix_labels) = 0)
        )
    );
