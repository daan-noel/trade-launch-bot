-- Persist run SOL knobs as DOUBLE PRECISION (f64), not REAL (f32).
--
-- `bucket_width_sol` / `buy_amount_sol` were REAL so a stored 0.1 widened back
-- to f64 as 0.10000000149011612. Promote / flow-discovery bind then copied that
-- noise into `fingerprints.bucket_size_amount`. Store f64 end-to-end and tidy
-- any rows already polluted by the f32 round-trip (real→text→float8 recovers
-- the shortest decimal, e.g. 0.1).

ALTER TABLE grouped_sweep_runs
    ALTER COLUMN bucket_width_sol TYPE DOUBLE PRECISION,
    ALTER COLUMN buy_amount_sol TYPE DOUBLE PRECISION;

UPDATE grouped_sweep_runs
SET bucket_width_sol = ((bucket_width_sol::real)::text)::double precision
WHERE bucket_width_sol IS NOT NULL
  AND bucket_width_sol IS DISTINCT FROM ((bucket_width_sol::real)::text)::double precision;

UPDATE grouped_sweep_runs
SET buy_amount_sol = ((buy_amount_sol::real)::text)::double precision
WHERE buy_amount_sol IS NOT NULL
  AND buy_amount_sol IS DISTINCT FROM ((buy_amount_sol::real)::text)::double precision;
