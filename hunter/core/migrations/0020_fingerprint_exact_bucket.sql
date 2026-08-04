-- 0020: allow `fingerprints.bucket_size_amount` to be NULL, meaning the row's SOL
-- axes match on their EXACT lamports amount instead of a bucket range.
--
-- Why NULL and not 0: a bucket width is a *measured* quantity, so "not bucketed"
-- is spelled NULL. `0` is a division by zero inside `grouping::bucket_index` and,
-- worse, would be a second sentinel in a field that already caused one live bug
-- (two readers disagreeing about whether `0` meant "default 0.1" or "literally
-- zero", which saturated every amount into one bucket and armed on any non-zero
-- value). Keeping `0` invalid means `precision()` has exactly one thing to read.
--
-- 0014 established `CHECK (>= 1e-6 AND <= 1e6)`. A NOT-NULL check constraint in
-- Postgres evaluates to NULL — i.e. passes — for a NULL column, so 0014 would in
-- fact already admit NULL; it is replaced anyway so the intent is explicit in the
-- schema rather than incidental, and so the column comment carries the contract.
--
-- Backfill: none. Every existing row has a real width and keeps it, so this
-- migration cannot change how any existing fingerprint matches. Exact mode is
-- opt-in per row from here on.

ALTER TABLE fingerprints
    DROP CONSTRAINT IF EXISTS fingerprints_bucket_size_amount_positive;

ALTER TABLE fingerprints
    ADD CONSTRAINT fingerprints_bucket_size_amount_positive
    CHECK (
        bucket_size_amount IS NULL
        OR (bucket_size_amount >= 1e-6 AND bucket_size_amount <= 1e6)
    );

COMMENT ON COLUMN fingerprints.bucket_size_amount IS
    'SOL bucket width for every SOL match axis, or NULL for exact-lamports matching. '
    'Never 0 (division by zero in bucket_index, and a second sentinel). '
    'Read via hunter_engine::fingerprint::Fingerprint::precision().';
