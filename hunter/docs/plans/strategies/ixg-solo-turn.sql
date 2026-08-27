-- Solo as a turn: one quiet-resume buy into selling / a dip already true
-- behind that print. Completing print = the solo buy. Gap sells = curve
-- sells in slots [S-5, S) (the buy-quiet can still print sells). Same-slot
-- sells with tx_index < the solo are also behind it.
-- Scratch in schema ixg. Does not drop burst / perm / new-wallet tables.
-- Thermometer only — not a money book.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.scand CASCADE;
CREATE UNLOGGED TABLE ixg.scand AS
SELECT
  b.mint, b.slot, b.tot, b.he1, b.he_causal,
  m.ts AS t0, m.tx_index AS tx0, m.sol, m.tmpl, m.program,
  m.wal_new, m.working, m.ata, m.wal_old, m.wal_born
FROM ixg.burst b
JOIN ixg.bmem_old m USING (mint, slot)
WHERE b.kind = 'solo';
CREATE INDEX ON ixg.scand (mint, slot);

DROP TABLE IF EXISTS ixg.stape CASCADE;
CREATE UNLOGGED TABLE ixg.stape AS
SELECT
  t.mint_address AS mint,
  t.slot,
  t.tx_index,
  t.block_time AS ts,
  t.trade_type,
  t.amount_lamports AS sol_lp,
  t.reserve_lamports AS vsol_lp,
  CASE WHEN t.reserve_token > 0
    THEN t.reserve_lamports::double precision / t.reserve_token::double precision
  END AS px
FROM trades t
WHERE t.venue = 'curve'
  AND t.block_time >= '2026-07-26'
  AND t.block_time <  '2026-08-23'
  AND EXISTS (SELECT 1 FROM ixg.mint0 m WHERE m.mint = t.mint_address);
CREATE INDEX ON ixg.stape (mint, slot, tx_index);
CREATE INDEX ON ixg.stape (mint, ts);

DROP TABLE IF EXISTS ixg.srun CASCADE;
CREATE UNLOGGED TABLE ixg.srun AS
SELECT
  mint, slot, tx_index, ts, trade_type, sol_lp, vsol_lp, px,
  max(px) FILTER (WHERE px IS NOT NULL)
    OVER (PARTITION BY mint ORDER BY slot, tx_index) AS px_peak,
  min(px) FILTER (WHERE px IS NOT NULL)
    OVER (PARTITION BY mint ORDER BY slot, tx_index) AS px_trough
FROM ixg.stape;
CREATE INDEX ON ixg.srun (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.sslot CASCADE;
CREATE UNLOGGED TABLE ixg.sslot AS
SELECT
  mint, slot,
  count(*) FILTER (WHERE trade_type = 'sell')::int AS n_sell,
  COALESCE(sum(sol_lp) FILTER (WHERE trade_type = 'sell'), 0)
    / 1e9::double precision AS sol_sell
FROM ixg.stape
GROUP BY mint, slot;
CREATE INDEX ON ixg.sslot (mint, slot);

DROP TABLE IF EXISTS ixg.sgap CASCADE;
CREATE UNLOGGED TABLE ixg.sgap AS
SELECT
  s.mint, s.slot,
  COALESCE(sum(g.n_sell), 0)::int AS n_sell_gap,
  COALESCE(sum(g.sol_sell), 0) AS sol_sell_gap
FROM ixg.scand s
LEFT JOIN ixg.sslot g
  ON g.mint = s.mint AND g.slot >= s.slot - 5 AND g.slot < s.slot
GROUP BY s.mint, s.slot;
CREATE INDEX ON ixg.sgap (mint, slot);

DROP TABLE IF EXISTS ixg.sbefore CASCADE;
CREATE UNLOGGED TABLE ixg.sbefore AS
SELECT
  s.mint, s.slot,
  count(*) FILTER (WHERE t.trade_type = 'sell')::int AS n_sell_before,
  COALESCE(sum(t.sol_lp) FILTER (WHERE t.trade_type = 'sell'), 0)
    / 1e9::double precision AS sol_sell_before
FROM ixg.scand s
LEFT JOIN ixg.stape t
  ON t.mint = s.mint AND t.slot = s.slot AND t.tx_index < s.tx0
GROUP BY s.mint, s.slot;
CREATE INDEX ON ixg.sbefore (mint, slot);

DROP TABLE IF EXISTS ixg.sturn CASCADE;
CREATE UNLOGGED TABLE ixg.sturn AS
SELECT
  s.*,
  EXTRACT(EPOCH FROM (s.t0 - tok.created_at)) AS age_s,
  g.n_sell_gap,
  g.sol_sell_gap,
  b.n_sell_before,
  b.sol_sell_before,
  pre.vsol,
  pre.px,
  pre.px_peak,
  pre.px_trough,
  pre.trade_type AS last_side,
  CASE WHEN pre.px_peak > 0 AND pre.px IS NOT NULL
    THEN 100.0 * (pre.px_peak - pre.px) / pre.px_peak END AS trail,
  CASE WHEN pre.px_trough > 0 AND pre.px IS NOT NULL
    THEN 100.0 * (pre.px - pre.px_trough) / pre.px_trough END AS rise
FROM ixg.scand s
JOIN tokens tok ON tok.mint_address = s.mint
JOIN ixg.sgap g USING (mint, slot)
JOIN ixg.sbefore b USING (mint, slot)
LEFT JOIN LATERAL (
  SELECT
    r.vsol_lp / 1e9::double precision AS vsol,
    r.px, r.px_peak, r.px_trough, r.trade_type
  FROM ixg.srun r
  WHERE r.mint = s.mint
    AND (r.slot < s.slot OR (r.slot = s.slot AND r.tx_index < s.tx0))
  ORDER BY r.slot DESC, r.tx_index DESC
  LIMIT 1
) pre ON true;
CREATE INDEX ON ixg.sturn (mint, slot);
