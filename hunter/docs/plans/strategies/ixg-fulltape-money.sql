-- Full-tape crossing events for the named machine.
-- Window: 2026-08-11 inclusive .. 2026-08-23 exclusive.
-- Door + quiet gap + several-wallet burst + vsol<46. Fire = crossing print.
-- Wallet 2720's buys are not burst members. Launch rows are out. Create slot is out.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.dmint CASCADE;
CREATE UNLOGGED TABLE ixg.dmint AS
SELECT
  t.mint_address AS mint,
  t.created_at,
  t.creation_slot
FROM tokens t
LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
WHERE ixg.has_prefix(t.ix_labels, 'Associated Token:%')
  AND t.initial_buy_lamports >= 200000000
  AND COALESCE(i.first_slot_buy_lamports, 0) >= 500000000;
CREATE INDEX ON ixg.dmint (mint);

-- All curve buys on door mints (quiet + vsol + walk). Includes 2720.
DROP TABLE IF EXISTS ixg.fall CASCADE;
CREATE UNLOGGED TABLE ixg.fall AS
SELECT
  t.mint_address AS mint,
  t.slot,
  t.tx_index,
  t.block_time AS ts,
  t.trade_type,
  t.wallet_id,
  t.amount_lamports AS sol_lp,
  t.reserve_lamports AS vsol_lp,
  CASE WHEN t.reserve_token > 0
    THEN t.reserve_lamports::double precision / t.reserve_token::double precision
  END AS px
FROM trades t
WHERE t.venue = 'curve'
  AND t.block_time >= '2026-08-11'
  AND t.block_time <  '2026-08-23'
  AND EXISTS (SELECT 1 FROM ixg.dmint d WHERE d.mint = t.mint_address);
CREATE INDEX ON ixg.fall (mint, slot, tx_index);
CREATE INDEX ON ixg.fall (mint, ts);

-- Burst members: buys, not him, not launch.
DROP TABLE IF EXISTS ixg.fbuy CASCADE;
CREATE UNLOGGED TABLE ixg.fbuy AS
SELECT
  t.mint_address AS mint,
  t.slot,
  t.tx_index,
  t.block_time AS ts,
  t.wallet_id,
  t.amount_lamports / 1e9::double precision AS sol,
  t.reserve_lamports / 1e9::double precision AS vsol,
  ixg.program(t.ix_labels) AS program,
  (ixg.program(t.ix_labels)
    || CASE WHEN ixg.has_prefix(t.ix_labels, 'Compute Budget:%') THEN '|CU' ELSE '' END
    || CASE WHEN ixg.has_prefix(t.ix_labels, 'Associated Token:%') THEN '|ATA' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: AdvanceNonceAccount') THEN '|N' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: CreateAccountWithSeed') THEN '|S' ELSE '' END
    || CASE WHEN ixg.has_label(t.ix_labels, 'System Program: Transfer') THEN '|F' ELSE '' END
  ) AS tmpl,
  ixg.has_label(t.ix_labels, 'System Program: CreateAccountWithSeed') AS seed
FROM trades t
WHERE t.venue = 'curve'
  AND t.trade_type = 'buy'
  AND t.block_time >= '2026-08-11'
  AND t.block_time <  '2026-08-23'
  AND t.wallet_id <> 2720
  AND t.ix_labels IS NOT NULL
  AND NOT ixg.has_prefix(t.ix_labels, 'Pump.Fun: Create%')
  AND EXISTS (SELECT 1 FROM ixg.dmint d WHERE d.mint = t.mint_address);
CREATE INDEX ON ixg.fbuy (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.fquiet CASCADE;
CREATE UNLOGGED TABLE ixg.fquiet AS
SELECT mint, slot
FROM (
  SELECT mint, slot,
    slot - lag(slot) OVER (PARTITION BY mint ORDER BY slot, tx_index) AS dslot,
    row_number() OVER (PARTITION BY mint, slot ORDER BY tx_index) AS rn
  FROM ixg.fall
  WHERE trade_type = 'buy'
) s
WHERE rn = 1 AND (dslot IS NULL OR dslot >= 5);
CREATE INDEX ON ixg.fquiet (mint, slot);

-- Quiet slots that are not the create slot, with enough size to be a candidate.
DROP TABLE IF EXISTS ixg.fslot CASCADE;
CREATE UNLOGGED TABLE ixg.fslot AS
SELECT
  b.mint, b.slot,
  count(*)::int AS ntx,
  count(DISTINCT b.wallet_id)::int AS nwal,
  count(DISTINCT b.tmpl)::int AS ntmpl,
  sum(b.sol) AS tot,
  min(b.tx_index) AS tx0,
  min(b.ts) AS t0
FROM ixg.fbuy b
JOIN ixg.fquiet q USING (mint, slot)
JOIN ixg.dmint d ON d.mint = b.mint
WHERE d.creation_slot IS NULL OR b.slot > d.creation_slot
GROUP BY b.mint, b.slot
HAVING count(*) >= 2 AND count(*) <= 20 AND sum(b.sol) >= 0.9;
CREATE INDEX ON ixg.fslot (mint, slot);

DROP TABLE IF EXISTS ixg.fmem CASCADE;
CREATE UNLOGGED TABLE ixg.fmem AS
SELECT b.*
FROM ixg.fbuy b
JOIN ixg.fslot s USING (mint, slot);
CREATE INDEX ON ixg.fmem (mint, slot, tx_index);

-- Prefix = candidate crossing state after each print.
DROP TABLE IF EXISTS ixg.fpre CASCADE;
CREATE UNLOGGED TABLE ixg.fpre AS
SELECT
  a.mint, a.slot, a.tx_index, a.ts, a.sol AS this_sol,
  a.tmpl AS this_tmpl, a.program AS this_prog, a.wallet_id,
  a.vsol AS vsol_now,
  count(*)::int AS rn,
  sum(b.sol) AS run_sol,
  count(DISTINCT b.wallet_id)::int AS run_nwal,
  count(DISTINCT b.tmpl)::int AS run_ntmpl,
  count(*) FILTER (WHERE b.tmpl = a.tmpl)::int AS fam_n,
  sum(b.sol) FILTER (WHERE b.tmpl = a.tmpl) AS fam_sol
FROM ixg.fmem a
JOIN ixg.fmem b
  ON b.mint = a.mint AND b.slot = a.slot AND b.tx_index <= a.tx_index
GROUP BY a.mint, a.slot, a.tx_index, a.ts, a.sol, a.tmpl, a.program, a.wallet_id, a.vsol;
CREATE INDEX ON ixg.fpre (mint, slot, tx_index);

-- Previous prefix (to detect the crossing print).
DROP TABLE IF EXISTS ixg.fcross CASCADE;
CREATE UNLOGGED TABLE ixg.fcross AS
SELECT
  p.*,
  CASE
    WHEN p.run_ntmpl = 1 AND p.run_nwal >= 2 AND p.fam_n >= 2
         AND p.fam_sol >= 0.9 AND p.fam_sol < 4 THEN 'same'
    WHEN p.run_ntmpl >= 2 AND p.run_nwal >= 2
         AND p.run_sol >= 0.9 AND p.run_sol < 4 THEN 'mixed'
    WHEN p.run_ntmpl = 1 AND p.run_nwal = 1 AND p.fam_n >= 2
         AND p.fam_sol >= 0.9 AND p.fam_sol < 4 THEN 'onewal'
    ELSE NULL
  END AS fam
FROM ixg.fpre p;
CREATE INDEX ON ixg.fcross (mint, slot, tx_index);

-- One fire per slot: the first prefix that crosses any family band.
DROP TABLE IF EXISTS ixg.fev CASCADE;
CREATE UNLOGGED TABLE ixg.fev AS
SELECT DISTINCT ON (mint, slot)
  mint, slot, tx_index, ts, this_tmpl, this_prog, fam,
  fam_n, fam_sol, run_sol, run_nwal, run_ntmpl, vsol_now
FROM ixg.fcross
WHERE fam IS NOT NULL
ORDER BY mint, slot, tx_index;
CREATE INDEX ON ixg.fev (mint, ts);

-- vsol at last print before the burst starts (permission).
ALTER TABLE ixg.fev ADD COLUMN vsol_pre double precision;
ALTER TABLE ixg.fev ADD COLUMN t0 timestamptz;
UPDATE ixg.fev e SET t0 = s.t0, vsol_pre = pre.vsol
FROM ixg.fslot s
LEFT JOIN LATERAL (
  SELECT f.vsol_lp / 1e9::double precision AS vsol
  FROM ixg.fall f
  WHERE f.mint = s.mint
    AND (f.slot < s.slot OR (f.slot = s.slot AND f.tx_index < s.tx0))
    AND f.vsol_lp IS NOT NULL AND f.vsol_lp > 0
  ORDER BY f.slot DESC, f.tx_index DESC
  LIMIT 1
) pre ON true
WHERE e.mint = s.mint AND e.slot = s.slot;

ALTER TABLE ixg.fev ADD COLUMN created_at timestamptz;
UPDATE ixg.fev e SET created_at = d.created_at
FROM ixg.dmint d WHERE d.mint = e.mint;

-- Working same-template vs dead same-template vs mixed vs one-wallet.
ALTER TABLE ixg.fev ADD COLUMN book text;
UPDATE ixg.fev SET book = CASE
  WHEN fam = 'same' AND this_tmpl IN (
    'Axiom Trade|CU|ATA|F',
    'Photon|CU|ATA|F',
    'Terminal|CU|ATA|F',
    'GMGN Bot|CU|ATA|F',
    'Bloom Router|CU|F',
    'Axiom Trade|CU|ATA|N|F'
  ) THEN 'same_work'
  WHEN fam = 'same' AND this_tmpl IN (
    'Axiom Trade|CU|F',
    'Photon|CU|F'
  ) THEN 'same_dead'
  WHEN fam = 'mixed' THEN 'mixed'
  WHEN fam = 'onewal' THEN 'onewal'
  ELSE 'same_other'
END;

DROP TABLE IF EXISTS ixg.fcand CASCADE;
CREATE UNLOGGED TABLE ixg.fcand AS
SELECT *
FROM ixg.fev
WHERE vsol_pre IS NOT NULL AND vsol_pre < 46
  AND book IN ('same_work', 'same_dead', 'mixed', 'onewal');
CREATE INDEX ON ixg.fcand (book, mint, ts);
