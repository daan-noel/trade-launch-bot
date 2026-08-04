-- 0021: finish what 0020 started -- `fingerprints.bucket_size_amount` must
-- actually accept NULL (exact-lamports matching).
--
-- 0020 replaced the CHECK with `IS NULL OR (>= 1e-6 AND <= 1e6)` but left the
-- `NOT NULL` that 0004 declared alongside `DEFAULT 0.1`. A CHECK is not the only
-- gate on the column, so exact mode was legal to the constraint and impossible to
-- write: `Fingerprint::validate` passed, the INSERT/UPDATE bound NULL, and
-- Postgres rejected it -- surfacing as a 500 ("update fingerprint failed") from
-- the create/update handlers rather than anything that named the real cause.
--
-- The DEFAULT stays. It is the storage-side mirror of `Fingerprint::from_json`'s
-- rule that an ABSENT `bucket_size_amount` means the 0.1 default (only an
-- explicit `null` opts into exact), so a writer that omits the column must not
-- silently land in exact mode.
--
-- Backfill: none. Every existing row holds a real width; nothing changes how any
-- fingerprint matches today.

ALTER TABLE fingerprints
    ALTER COLUMN bucket_size_amount DROP NOT NULL;
