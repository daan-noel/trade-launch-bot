-- Collapse f32 (REAL) round-trip noise on fingerprint bucket widths.
--
-- Promote / bind used to copy `grouped_sweep_runs.bucket_width_sol` (REAL) into
-- `fingerprints.bucket_size_amount` (DOUBLE), leaving values like
-- 0.10000000149011612 for an intended 0.1. Cast through real→text→float8 recovers
-- the shortest decimal representation.

UPDATE fingerprints
SET bucket_size_amount = ((bucket_size_amount::real)::text)::double precision
WHERE bucket_size_amount IS DISTINCT FROM ((bucket_size_amount::real)::text)::double precision;
