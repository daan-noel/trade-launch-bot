-- 0014: `fingerprints.bucket_size_amount` must be a real, positive SOL width.
--
-- Why this is a correctness fix, not tidying: the engine matcher
-- (`hunter_engine::fingerprint::matches_phase`) divides by this width RAW via
-- `grouping::bucket_index` = `floor(v / width + 1e-9)`. At width 0 every
-- positive amount divides to +inf and saturates to the same `i64::MAX` bucket
-- index, so a configured SOL axis stops discriminating entirely and the
-- fingerprint arms on ANY non-zero value -- the match-everything hazard the
-- `has_any_criterion` guard exists to prevent, on the live arming path.
--
-- The bug was invisible because a second reader disagreed: the creation-stats
-- SQL mirror (`fingerprint_bucket_width`) treated 0 as "unset" and silently
-- substituted the 0.1 default, so the dashboard's matched-token count looked
-- plausible while the live engine armed far more. That fallback is deleted in
-- the same change; this CHECK is what lets it be deleted safely.
--
-- Note the asymmetry with the SOL AXES on the same row: there, 0 is a perfectly
-- valid value (`spendable_lamports_in = 0` at width 1 means the bucket [0, 1)),
-- and `NULL` is the only way to say "axis not part of identity". Those columns
-- stay nullable with no CHECK. Zero-as-unbound is reserved for caps and limits
-- where 0 is not a meaningful value of the domain.
--
-- 1e-6 SOL = MIN_BUCKET_WIDTH_SOL (hunter/engine/src/grouping.rs): below it the
-- 1e-9 ratio-epsilon in `bucket_index` stops being negligible. The 1e6 ceiling
-- exists to exclude NaN and Infinity, which DOUBLE PRECISION accepts and which
-- Postgres orders ABOVE every finite value (so a `>= 1e-6` bound alone lets
-- both through).

UPDATE fingerprints
SET bucket_size_amount = 0.1
WHERE NOT (bucket_size_amount >= 1e-6 AND bucket_size_amount <= 1e6);

ALTER TABLE fingerprints
    ADD CONSTRAINT fingerprints_bucket_size_amount_positive
    CHECK (bucket_size_amount >= 1e-6 AND bucket_size_amount <= 1e6);
