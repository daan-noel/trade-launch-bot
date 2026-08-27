-- Full-tape money events: first-on-mint completing prints.
-- Reuses ixg.dmint / fall / fquiet / fbuy. Does not drop fcand.
-- Window 2026-08-11 .. 2026-08-23. Door mints created in-window so
-- first-on-mint from fall is exact. Wallet 2720 is not a burst member.
-- Fire = the print that first makes the conjunction true.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

-- First buy-slot of each wallet on each door mint in this tape.
DROP TABLE IF EXISTS ixg.nwal0 CASCADE;
CREATE UNLOGGED TABLE ixg.nwal0 AS
SELECT mint, wallet_id, min(slot) AS slot0
FROM ixg.fall
WHERE trade_type = 'buy' AND wallet_id IS NOT NULL
GROUP BY 1, 2;
CREATE INDEX ON ixg.nwal0 (mint, wallet_id);

-- Quiet resume slots, tot in [0.9, 4), solos included, create slot out.
DROP TABLE IF EXISTS ixg.nslot CASCADE;
CREATE UNLOGGED TABLE ixg.nslot AS
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
HAVING sum(b.sol) >= 0.9 AND sum(b.sol) < 4
   AND count(*) >= 1 AND count(*) <= 20;
CREATE INDEX ON ixg.nslot (mint, slot);

-- Members: reuse fmem templates; fill remaining (mostly solos) from trades.
DROP TABLE IF EXISTS ixg.nmem CASCADE;
CREATE UNLOGGED TABLE ixg.nmem AS
SELECT
  b.mint, b.slot, b.tx_index, b.ts, b.wallet_id, b.sol, b.vsol,
  m.program, m.tmpl
FROM ixg.fbuy b
JOIN ixg.nslot s USING (mint, slot)
LEFT JOIN ixg.fmem m USING (mint, slot, tx_index);
CREATE INDEX ON ixg.nmem (mint, slot, tx_index);
CREATE INDEX ON ixg.nmem (mint, wallet_id);

UPDATE ixg.nmem n SET
  program = ixg.program(t.ix_labels),
  tmpl = ixg.program(t.ix_labels)
    || CASE WHEN ixg.has_prefix(t.ix_labels, 'Compute Budget:%') THEN '|CU' ELSE '' END
    || CASE WHEN ixg.has_prefix(t.ix_labels, 'Associated Token:%') THEN '|ATA' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: AdvanceNonceAccount') THEN '|N' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: CreateAccountWithSeed') THEN '|S' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: Transfer') THEN '|F' ELSE '' END
FROM trades t
WHERE n.tmpl IS NULL
  AND t.mint_address = n.mint
  AND t.slot = n.slot
  AND t.tx_index = n.tx_index
  AND t.venue = 'curve'
  AND t.trade_type = 'buy'
  AND t.block_time >= '2026-08-11'
  AND t.block_time <  '2026-08-23'
  AND t.ix_labels IS NOT NULL
  AND NOT ixg.has_prefix(t.ix_labels, 'Pump.Fun: Create%');

DELETE FROM ixg.nmem WHERE tmpl IS NULL;

ALTER TABLE ixg.nmem ADD COLUMN wal_new boolean;
ALTER TABLE ixg.nmem ADD COLUMN wal_rep boolean;
UPDATE ixg.nmem n SET
  wal_new = (n.wallet_id IS NOT NULL AND w.slot0 = n.slot),
  wal_rep = (n.wallet_id IS NOT NULL AND w.slot0 IS NOT NULL AND w.slot0 < n.slot)
FROM ixg.nwal0 w
WHERE w.mint = n.mint AND w.wallet_id = n.wallet_id;

UPDATE ixg.nmem SET wal_new = false WHERE wal_new IS NULL;
UPDATE ixg.nmem SET wal_rep = false WHERE wal_rep IS NULL;

-- Prefix state after each print.
DROP TABLE IF EXISTS ixg.npre CASCADE;
CREATE UNLOGGED TABLE ixg.npre AS
SELECT
  a.mint, a.slot, a.tx_index, a.ts, a.sol AS this_sol,
  a.tmpl AS this_tmpl, a.program AS this_prog, a.wallet_id,
  a.vsol AS vsol_now, a.wal_new AS this_new,
  count(*)::int AS rn,
  sum(b.sol) AS run_sol,
  count(DISTINCT b.wallet_id)::int AS run_nwal,
  count(DISTINCT b.tmpl)::int AS run_ntmpl,
  count(*) FILTER (WHERE b.tmpl = a.tmpl)::int AS fam_n,
  sum(b.sol) FILTER (WHERE b.tmpl = a.tmpl) AS fam_sol,
  count(DISTINCT b.wallet_id) FILTER (WHERE b.wal_new)::int AS nwal_new,
  count(DISTINCT b.wallet_id) FILTER (WHERE b.wal_rep)::int AS nwal_rep,
  bool_or(b.wallet_id IS NULL) AS has_unk
FROM ixg.nmem a
JOIN ixg.nmem b
  ON b.mint = a.mint AND b.slot = a.slot AND b.tx_index <= a.tx_index
GROUP BY
  a.mint, a.slot, a.tx_index, a.ts, a.sol,
  a.tmpl, a.program, a.wallet_id, a.vsol, a.wal_new;
CREATE INDEX ON ixg.npre (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.ncross CASCADE;
CREATE UNLOGGED TABLE ixg.ncross AS
SELECT
  p.*,
  CASE
    WHEN p.rn = 1 AND p.this_new AND NOT p.has_unk
         AND p.this_sol >= 0.9 AND p.this_sol < 4 THEN 'solo_new'
    WHEN p.rn = 1 AND NOT COALESCE(p.this_new, false)
         AND p.nwal_rep >= 1 AND p.nwal_new = 0 AND NOT p.has_unk
         AND p.this_sol >= 0.9 AND p.this_sol < 4 THEN 'solo_rep'
    WHEN p.run_ntmpl = 1 AND p.nwal_new >= 2 AND p.nwal_rep = 0 AND NOT p.has_unk
         AND p.fam_n >= 2 AND p.fam_sol >= 0.9 AND p.fam_sol < 4 THEN 'same_new'
    WHEN p.run_ntmpl = 1 AND p.nwal_rep >= 2 AND p.nwal_new = 0 AND NOT p.has_unk
         AND p.fam_n >= 2 AND p.fam_sol >= 0.9 AND p.fam_sol < 4 THEN 'same_rep'
    WHEN p.run_ntmpl >= 2 AND p.nwal_new >= 2 AND p.nwal_rep = 0 AND NOT p.has_unk
         AND p.run_nwal >= 2 AND p.run_sol >= 0.9 AND p.run_sol < 4 THEN 'mixed_new'
    WHEN p.run_ntmpl >= 2 AND p.nwal_rep >= 2 AND p.nwal_new = 0 AND NOT p.has_unk
         AND p.run_nwal >= 2 AND p.run_sol >= 0.9 AND p.run_sol < 4 THEN 'mixed_rep'
    ELSE NULL
  END AS fam
FROM ixg.npre p;
CREATE INDEX ON ixg.ncross (mint, slot, tx_index);

-- First print in the slot that completes each family (a slot can be in more
-- than one book if print 1 is a solo-sized new buy and a later print completes
-- a burst).
DROP TABLE IF EXISTS ixg.nev CASCADE;
CREATE UNLOGGED TABLE ixg.nev AS
SELECT DISTINCT ON (mint, slot, fam)
  mint, slot, tx_index, ts, this_tmpl, this_prog, fam,
  fam_n, fam_sol, run_sol, run_nwal, run_ntmpl, nwal_new, nwal_rep,
  this_sol, this_new, vsol_now
FROM ixg.ncross
WHERE fam IS NOT NULL
ORDER BY mint, slot, fam, tx_index;
CREATE INDEX ON ixg.nev (mint, ts);

ALTER TABLE ixg.nev ADD COLUMN vsol_pre double precision;
ALTER TABLE ixg.nev ADD COLUMN t0 timestamptz;

DROP TABLE IF EXISTS ixg.nprev CASCADE;
CREATE UNLOGGED TABLE ixg.nprev AS
SELECT
  mint, slot, tx_index,
  lag(vsol_lp) OVER (PARTITION BY mint ORDER BY slot, tx_index)
    / 1e9::double precision AS vsol_pre
FROM ixg.fall
WHERE mint IN (SELECT mint FROM ixg.nslot)
  AND vsol_lp IS NOT NULL AND vsol_lp > 0;
CREATE INDEX ON ixg.nprev (mint, slot, tx_index);

UPDATE ixg.nev e SET t0 = s.t0, vsol_pre = p.vsol_pre
FROM ixg.nslot s
JOIN ixg.nprev p
  ON p.mint = s.mint AND p.slot = s.slot AND p.tx_index = s.tx0
WHERE e.mint = s.mint AND e.slot = s.slot;

ALTER TABLE ixg.nev ADD COLUMN created_at timestamptz;
UPDATE ixg.nev e SET created_at = d.created_at
FROM ixg.dmint d WHERE d.mint = e.mint;

ALTER TABLE ixg.nev ADD COLUMN working boolean;
UPDATE ixg.nev SET working = this_tmpl IN (
  'Axiom Trade|CU|ATA|F',
  'Axiom Trade|CU|ATA|N|F',
  'Photon|CU|ATA|F',
  'Terminal|CU|ATA|F',
  'GMGN Bot|CU|ATA|F',
  'GMGN|CU|ATA|F',
  'Bloom Router|CU|F',
  'Bloom|CU|F'
);

DROP TABLE IF EXISTS ixg.ncand CASCADE;
CREATE UNLOGGED TABLE ixg.ncand AS
SELECT *
FROM ixg.nev
WHERE vsol_pre IS NOT NULL AND vsol_pre < 46;
CREATE INDEX ON ixg.ncand (fam, mint, ts);
