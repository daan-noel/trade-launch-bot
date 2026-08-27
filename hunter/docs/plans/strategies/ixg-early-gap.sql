-- Early fire: first working new print after a buy-gap, with gap length.
-- Door + vsol<46 + not create slot. Completing band is NOT required.
-- Later-slot shape is diagnostic (lookahead); the live fire is print 1.
-- Reuses ixg.dmint / fall / fbuy / nwal0 / nprev. Does not drop them.
-- Window 2026-08-11 .. 2026-08-23. Wallet 2720 is not a burst member.

SET statement_timeout = 0;
SET work_mem = '64MB';
SET maintenance_work_mem = '256MB';
SET max_parallel_workers_per_gather = 0;
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.eg_cand CASCADE;
DROP TABLE IF EXISTS ixg.eg_ev CASCADE;
DROP TABLE IF EXISTS ixg.eg_fire CASCADE;
DROP TABLE IF EXISTS ixg.eg_slot CASCADE;
DROP TABLE IF EXISTS ixg.eg_resume CASCADE;
DROP TABLE IF EXISTS ixg.eg_his CASCADE;

CREATE UNLOGGED TABLE ixg.eg_resume AS
SELECT mint, slot, dslot, tx_index AS tx0, ts AS t0
FROM (
  SELECT
    f.mint, f.slot, f.tx_index, f.ts,
    f.slot - lag(f.slot) OVER (PARTITION BY f.mint ORDER BY f.slot, f.tx_index)
      AS dslot,
    row_number() OVER (PARTITION BY f.mint, f.slot ORDER BY f.tx_index) AS rn
  FROM ixg.fall f
  JOIN ixg.dmint d ON d.mint = f.mint
  WHERE f.trade_type = 'buy'
    AND d.created_at >= '2026-08-11'
    AND d.created_at <  '2026-08-23'
) s
WHERE rn = 1 AND dslot >= 2;
CREATE INDEX ON ixg.eg_resume (mint, slot);

CREATE UNLOGGED TABLE ixg.eg_slot AS
SELECT
  r.mint, r.slot, r.dslot, r.tx0, r.t0,
  count(*)::int AS ntx,
  count(DISTINCT b.wallet_id)::int AS nwal,
  count(DISTINCT b.tmpl)::int AS ntmpl,
  sum(b.sol) AS tot,
  min(b.tx_index) AS tx_lo,
  max(b.tx_index) AS tx_hi,
  ((max(b.tx_index) - min(b.tx_index) + 1) = count(*)) AS tight
FROM ixg.eg_resume r
JOIN ixg.fbuy b USING (mint, slot)
JOIN ixg.dmint d ON d.mint = r.mint
WHERE (d.creation_slot IS NULL OR r.slot > d.creation_slot)
GROUP BY r.mint, r.slot, r.dslot, r.tx0, r.t0
HAVING count(*) >= 1 AND count(*) <= 20;
CREATE INDEX ON ixg.eg_slot (mint, slot);

CREATE UNLOGGED TABLE ixg.eg_fire AS
SELECT
  b.mint, b.slot, b.tx_index, b.ts, b.sol, b.tmpl, b.wallet_id, b.vsol,
  s.dslot, s.ntx, s.nwal, s.ntmpl, s.tot, s.tight, s.tx0,
  (w.slot0 IS NOT NULL AND w.slot0 = b.slot) AS wal_new,
  (b.tmpl IN (
    'Axiom Trade|CU|ATA|F',
    'Axiom Trade|CU|ATA|N|F',
    'Photon|CU|ATA|F',
    'Terminal|CU|ATA|F',
    'GMGN Bot|CU|ATA|F',
    'GMGN|CU|ATA|F',
    'Bloom Router|CU|F',
    'Bloom|CU|F'
  )) AS working
FROM ixg.fbuy b
JOIN ixg.eg_slot s
  ON s.mint = b.mint AND s.slot = b.slot AND s.tx0 = b.tx_index
LEFT JOIN ixg.nwal0 w
  ON w.mint = b.mint AND w.wallet_id = b.wallet_id;
CREATE INDEX ON ixg.eg_fire (mint, slot);

CREATE UNLOGGED TABLE ixg.eg_his AS
SELECT DISTINCT mint, slot
FROM w8.buys;
CREATE INDEX ON ixg.eg_his (mint, slot);

CREATE UNLOGGED TABLE ixg.eg_ev AS
SELECT
  f.mint, f.slot, f.tx_index, f.ts, f.sol AS this_sol, f.tmpl AS this_tmpl,
  f.dslot, f.ntx, f.nwal, f.ntmpl, f.tot, f.tight, f.wal_new, f.working,
  CASE
    WHEN f.dslot < 5 THEN '2-4'
    WHEN f.dslot < 10 THEN '5-9'
    WHEN f.dslot < 20 THEN '10-19'
    WHEN f.dslot < 40 THEN '20-39'
    ELSE '40+'
  END AS gap_band,
  CASE
    WHEN f.ntx = 1 THEN 'solo'
    WHEN f.ntmpl = 1 AND f.nwal >= 2 AND f.tot >= 0.9 AND f.tot < 4
         AND f.tight THEN 'bundle'
    WHEN f.ntmpl = 1 AND f.nwal >= 2 AND f.tot >= 0.9 AND f.tot < 4
         AND NOT f.tight THEN 'separated'
    WHEN f.ntmpl >= 2 AND f.nwal >= 2 AND f.tot >= 0.9 AND f.tot < 4
         AND f.tight THEN 'mixed_tight'
    WHEN f.ntmpl >= 2 AND f.nwal >= 2 AND f.tot >= 0.9 AND f.tot < 4
         AND NOT f.tight THEN 'mixed_gap'
    ELSE 'other'
  END AS later,
  (h0.mint IS NOT NULL OR h1.mint IS NOT NULL) AS he1,
  (h1.mint IS NOT NULL AND h0.mint IS NULL) AS he_causal
FROM ixg.eg_fire f
LEFT JOIN ixg.eg_his h0 ON h0.mint = f.mint AND h0.slot = f.slot
LEFT JOIN ixg.eg_his h1 ON h1.mint = f.mint AND h1.slot = f.slot + 1
WHERE f.working AND f.wal_new;
CREATE INDEX ON ixg.eg_ev (mint, slot);

ALTER TABLE ixg.eg_ev ADD COLUMN vsol_pre double precision;
ALTER TABLE ixg.eg_ev ADD COLUMN created_at timestamptz;

UPDATE ixg.eg_ev e SET vsol_pre = p.vsol_pre
FROM ixg.nprev p
WHERE p.mint = e.mint AND p.slot = e.slot AND p.tx_index = e.tx_index;

UPDATE ixg.eg_ev e SET vsol_pre = p.vsol_pre
FROM ixg.eg_slot s
JOIN LATERAL (
  SELECT f.vsol_lp / 1e9::double precision AS vsol_pre
  FROM ixg.fall f
  WHERE f.mint = s.mint
    AND (f.slot < s.slot OR (f.slot = s.slot AND f.tx_index < s.tx0))
    AND f.vsol_lp IS NOT NULL AND f.vsol_lp > 0
  ORDER BY f.slot DESC, f.tx_index DESC
  LIMIT 1
) p ON true
WHERE e.mint = s.mint AND e.slot = s.slot AND e.vsol_pre IS NULL;

UPDATE ixg.eg_ev e SET created_at = d.created_at
FROM ixg.dmint d WHERE d.mint = e.mint;

CREATE UNLOGGED TABLE ixg.eg_cand AS
SELECT *
FROM ixg.eg_ev
WHERE vsol_pre IS NOT NULL AND vsol_pre < 46;
CREATE INDEX ON ixg.eg_cand (gap_band, mint, ts);
CREATE INDEX ON ixg.eg_cand (later, mint, ts);
