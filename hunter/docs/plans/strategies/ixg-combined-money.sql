-- One machine: door + 5-slot gap + vsol<46 + not-all-repeat + working
-- completing print, crowd OR turn, with tight same-slot packs marked.
-- Reuses ixg.dmint / fall / fbuy / fquiet / nwal0. Does not drop them.
-- Window 2026-08-11 .. 2026-08-23. Wallet 2720 is not a burst member.
-- Fire = first print in the slot that makes any live family true.
-- Tight = prefix members occupy consecutive tx_index (no hole in the block).
-- Bundle = tight crowd; separated = same-ix crowd with a tx_index hole;
-- one = solo first-on-mint working print with trail >= 15.

SET statement_timeout = 0;
SET work_mem = '64MB';
SET maintenance_work_mem = '256MB';
SET max_parallel_workers_per_gather = 0;
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.cm_cand CASCADE;
DROP TABLE IF EXISTS ixg.cm_ev CASCADE;
DROP TABLE IF EXISTS ixg.cm_tagged CASCADE;
DROP TABLE IF EXISTS ixg.cm_hit CASCADE;
DROP TABLE IF EXISTS ixg.cm_cross CASCADE;
DROP TABLE IF EXISTS ixg.cm_pre CASCADE;
DROP TABLE IF EXISTS ixg.cm_mem CASCADE;
DROP TABLE IF EXISTS ixg.cm_slot CASCADE;
DROP TABLE IF EXISTS ixg.cm_run CASCADE;
DROP TABLE IF EXISTS ixg.cm_tlag CASCADE;
DROP TABLE IF EXISTS ixg.cm_prev CASCADE;
DROP TABLE IF EXISTS ixg.cm_smint CASCADE;

CREATE UNLOGGED TABLE ixg.cm_slot AS
SELECT
  b.mint, b.slot,
  count(*)::int AS ntx,
  count(DISTINCT b.wallet_id)::int AS nwal,
  sum(b.sol) AS tot,
  min(b.tx_index) AS tx0,
  min(b.ts) AS t0
FROM ixg.fbuy b
JOIN ixg.fquiet q USING (mint, slot)
JOIN ixg.dmint d ON d.mint = b.mint
WHERE (d.creation_slot IS NULL OR b.slot > d.creation_slot)
  AND d.created_at >= '2026-08-11'
  AND d.created_at <  '2026-08-23'
GROUP BY b.mint, b.slot
HAVING sum(b.sol) >= 0.9 AND count(*) >= 1 AND count(*) <= 20;
CREATE INDEX ON ixg.cm_slot (mint, slot);

CREATE UNLOGGED TABLE ixg.cm_mem AS
SELECT
  b.mint, b.slot, b.tx_index, b.ts, b.wallet_id, b.sol, b.vsol,
  b.program, b.tmpl,
  false AS wal_new,
  false AS wal_rep,
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
JOIN ixg.cm_slot s USING (mint, slot);
CREATE INDEX ON ixg.cm_mem (mint, slot, tx_index);
CREATE INDEX ON ixg.cm_mem (mint, wallet_id);

UPDATE ixg.cm_mem m SET
  wal_new = (w.slot0 = m.slot),
  wal_rep = (w.slot0 < m.slot)
FROM ixg.nwal0 w
WHERE w.mint = m.mint AND w.wallet_id = m.wallet_id;

CREATE UNLOGGED TABLE ixg.cm_pre AS
SELECT
  a.mint, a.slot, a.tx_index, a.ts, a.sol AS this_sol,
  a.tmpl AS this_tmpl, a.program AS this_prog, a.wallet_id,
  a.vsol AS vsol_now, a.wal_new AS this_new, a.working AS this_work,
  count(*)::int AS rn,
  sum(b.sol) AS run_sol,
  count(DISTINCT b.wallet_id)::int AS run_nwal,
  count(DISTINCT b.tmpl)::int AS run_ntmpl,
  count(*) FILTER (WHERE b.tmpl = a.tmpl)::int AS fam_n,
  sum(b.sol) FILTER (WHERE b.tmpl = a.tmpl) AS fam_sol,
  count(DISTINCT b.wallet_id) FILTER (WHERE b.wal_new)::int AS nwal_new,
  count(DISTINCT b.wallet_id) FILTER (WHERE b.wal_rep)::int AS nwal_rep,
  bool_or(b.wallet_id IS NULL) AS has_unk,
  min(b.tx_index) AS tx_lo,
  ((a.tx_index - min(b.tx_index) + 1) = count(*)) AS tight
FROM ixg.cm_mem a
JOIN ixg.cm_mem b
  ON b.mint = a.mint AND b.slot = a.slot AND b.tx_index <= a.tx_index
GROUP BY
  a.mint, a.slot, a.tx_index, a.ts, a.sol,
  a.tmpl, a.program, a.wallet_id, a.vsol, a.wal_new, a.working;
CREATE INDEX ON ixg.cm_pre (mint, slot, tx_index);

CREATE UNLOGGED TABLE ixg.cm_cross AS
SELECT
  p.*,
  CASE
    WHEN p.rn = 1 AND p.this_new AND p.this_work AND NOT p.has_unk
         AND p.this_sol >= 0.9 AND p.this_sol < 4 THEN 'solo_raw'
    WHEN p.run_ntmpl = 1 AND p.run_nwal >= 2 AND p.nwal_new >= 1
         AND NOT p.has_unk AND p.this_work
         AND p.fam_n >= 2 AND p.fam_sol >= 0.9 AND p.fam_sol < 4
         THEN 'crowd_same'
    WHEN p.run_ntmpl >= 2 AND p.run_nwal >= 2 AND p.nwal_new >= 1
         AND NOT p.has_unk AND p.this_work
         AND p.run_sol >= 0.9 AND p.run_sol < 4 THEN 'crowd_mixed'
    ELSE NULL
  END AS fam
FROM ixg.cm_pre p;
CREATE INDEX ON ixg.cm_cross (mint, slot, tx_index);

CREATE UNLOGGED TABLE ixg.cm_hit AS
SELECT DISTINCT ON (mint, slot, fam)
  mint, slot, tx_index, ts, this_tmpl, this_prog, fam,
  fam_n, fam_sol, run_sol, run_nwal, run_ntmpl, nwal_new, nwal_rep,
  this_sol, this_new, this_work, vsol_now, rn, tight, tx_lo
FROM ixg.cm_cross
WHERE fam IS NOT NULL
ORDER BY mint, slot, fam, tx_index;
CREATE INDEX ON ixg.cm_hit (mint, slot, tx_index);

CREATE UNLOGGED TABLE ixg.cm_tagged AS
SELECT
  h.mint, h.slot, h.tx_index, h.ts, h.this_tmpl, h.this_prog,
  h.fam_n, h.fam_sol, h.run_sol, h.run_nwal, h.run_ntmpl,
  h.nwal_new, h.nwal_rep, h.this_sol, h.this_new, h.this_work,
  h.vsol_now, h.rn, h.tight, h.tx_lo,
  CASE
    WHEN h.fam = 'solo_raw' THEN 'one'
    WHEN h.fam = 'crowd_same' AND h.tight THEN 'bundle'
    WHEN h.fam = 'crowd_same' AND NOT h.tight THEN 'separated'
    WHEN h.fam = 'crowd_mixed' AND h.tight THEN 'mixed_tight'
    WHEN h.fam = 'crowd_mixed' AND NOT h.tight THEN 'mixed_gap'
  END AS shape,
  CASE WHEN h.fam = 'solo_raw' THEN 'one' ELSE h.fam END AS fam,
  CASE WHEN l.peak_pre > 0 AND l.px_pre IS NOT NULL
    THEN 100.0 * (l.peak_pre - l.px_pre) / l.peak_pre END AS trail
FROM ixg.cm_hit h
LEFT JOIN ixg.tlag l USING (mint, slot, tx_index)
WHERE h.fam IN ('crowd_same', 'crowd_mixed')
   OR (h.fam = 'solo_raw' AND l.peak_pre > 0 AND l.px_pre IS NOT NULL
       AND 100.0 * (l.peak_pre - l.px_pre) / l.peak_pre >= 15);
CREATE INDEX ON ixg.cm_tagged (mint, slot, tx_index);

CREATE UNLOGGED TABLE ixg.cm_ev AS
SELECT DISTINCT ON (mint, slot)
  mint, slot, tx_index, ts, this_tmpl, this_prog, fam, shape,
  fam_n, fam_sol, run_sol, run_nwal, run_ntmpl, nwal_new, nwal_rep,
  this_sol, this_new, this_work, vsol_now, rn, tight, tx_lo, trail
FROM ixg.cm_tagged
WHERE shape IS NOT NULL
ORDER BY mint, slot, tx_index;
CREATE INDEX ON ixg.cm_ev (mint, slot);

ALTER TABLE ixg.cm_ev ADD COLUMN vsol_pre double precision;
ALTER TABLE ixg.cm_ev ADD COLUMN created_at timestamptz;
ALTER TABLE ixg.cm_ev ADD COLUMN fillable boolean;

UPDATE ixg.cm_ev e SET vsol_pre = p.vsol_pre
FROM ixg.cm_slot s
JOIN ixg.nprev p
  ON p.mint = s.mint AND p.slot = s.slot AND p.tx_index = s.tx0
WHERE e.mint = s.mint AND e.slot = s.slot;

UPDATE ixg.cm_ev e SET vsol_pre = p.vsol_pre
FROM ixg.cm_slot s
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

UPDATE ixg.cm_ev e SET created_at = d.created_at
FROM ixg.dmint d WHERE d.mint = e.mint;

UPDATE ixg.cm_ev SET fillable = shape IN ('one', 'separated', 'mixed_gap');

CREATE UNLOGGED TABLE ixg.cm_cand AS
SELECT *
FROM ixg.cm_ev
WHERE vsol_pre IS NOT NULL AND vsol_pre < 46;
CREATE INDEX ON ixg.cm_cand (shape, mint, ts);
CREATE INDEX ON ixg.cm_cand (fillable, mint, ts);
