-- Full-tape money: working Axiom/Photon CU+ATA templates vs dead same-tool no-ATA
-- control, fill at the first matching print in the slot. Re-entry allowed.
-- Window: 2026-08-11 inclusive .. 2026-08-23 exclusive (complete days before the gap).

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.tape CASCADE;
CREATE UNLOGGED TABLE ixg.tape AS
SELECT
  mint_address AS mint,
  slot,
  tx_index,
  block_time AS ts,
  trade_type,
  wallet_id,
  amount_lamports AS sol_lp,
  reserve_lamports AS vsol_lp,
  CASE WHEN reserve_token > 0
    THEN reserve_lamports::double precision / reserve_token::double precision
  END AS px,
  CASE
    WHEN ix_labels::text LIKE '%Axiom Trade:%' THEN 'Axiom Trade'
    WHEN ix_labels::text LIKE '%Photon:%' THEN 'Photon'
    ELSE NULL
  END AS program,
  (ix_labels @> '["Associated Token: CreateIdempotent"]'
    OR ix_labels @> '["Associated Token: Create"]') AS ata,
  (ix_labels @> '["Compute Budget: SetComputeUnitLimit"]'
    OR ix_labels @> '["Compute Budget: SetComputeUnitPrice"]') AS cu,
  (ix_labels @> '["System Program: CreateAccountWithSeed"]') AS seed
FROM trades
WHERE venue = 'curve'
  AND block_time >= '2026-08-11'
  AND block_time <  '2026-08-23'
  AND ix_labels IS NOT NULL;

CREATE INDEX ON ixg.tape (mint, slot, tx_index);
CREATE INDEX ON ixg.tape (mint, ts);

-- Buys that can complete a gate. His own fills excluded.
DROP TABLE IF EXISTS ixg.tbuy CASCADE;
CREATE UNLOGGED TABLE ixg.tbuy AS
SELECT mint, slot, tx_index, ts, wallet_id, sol_lp, vsol_lp, px, program, ata, cu, seed,
  (program IS NOT NULL AND cu AND ata AND NOT seed) AS work,
  (program IS NOT NULL AND cu AND NOT ata AND NOT seed) AS dead
FROM ixg.tape
WHERE trade_type = 'buy'
  AND wallet_id <> 2720
  AND program IS NOT NULL;

CREATE INDEX ON ixg.tbuy (mint, slot, tx_index);

-- Slot quiet: first buy on the mint in this slot has a >=5 slot gap (or is the first buy).
DROP TABLE IF EXISTS ixg.slot_q CASCADE;
CREATE UNLOGGED TABLE ixg.slot_q AS
SELECT mint, slot, (dslot IS NULL OR dslot >= 5) AS quiet5
FROM (
  SELECT mint, slot,
    slot - lag(slot) OVER (PARTITION BY mint ORDER BY slot, tx_index) AS dslot,
    row_number() OVER (PARTITION BY mint, slot ORDER BY tx_index) AS rn
  FROM ixg.tbuy
) s
WHERE rn = 1;
CREATE INDEX ON ixg.slot_q (mint, slot);

-- One event per (mint, slot, kind): first matching print completes the gate.
DROP TABLE IF EXISTS ixg.ev CASCADE;
CREATE UNLOGGED TABLE ixg.ev AS
SELECT DISTINCT ON (mint, slot, kind)
  mint, slot, kind, tx_index, ts, wallet_id, sol_lp, vsol_lp, px, program
FROM (
  SELECT mint, slot, tx_index, ts, wallet_id, sol_lp, vsol_lp, px, program,
    'work'::text AS kind FROM ixg.tbuy WHERE work
  UNION ALL
  SELECT mint, slot, tx_index, ts, wallet_id, sol_lp, vsol_lp, px, program,
    'dead'::text FROM ixg.tbuy WHERE dead
) u
ORDER BY mint, slot, kind, tx_index;

CREATE INDEX ON ixg.ev (mint, ts);
CREATE INDEX ON ixg.ev (kind);

ALTER TABLE ixg.ev ADD COLUMN quiet5 boolean;
UPDATE ixg.ev e SET quiet5 = q.quiet5
FROM ixg.slot_q q WHERE q.mint = e.mint AND q.slot = e.slot;

-- Creation-time door facts.
ALTER TABLE ixg.ev ADD COLUMN cashback boolean;
ALTER TABLE ixg.ev ADD COLUMN init_lp bigint;
ALTER TABLE ixg.ev ADD COLUMN created_at timestamptz;
UPDATE ixg.ev e SET
  cashback = t.is_cashback_enabled,
  init_lp  = t.initial_buy_lamports,
  created_at = t.created_at
FROM tokens t
WHERE t.mint_address = e.mint;

-- Fill at completing print. B = 0.10 SOL. Mark last print in (t, t+20s].
-- Net charges 125 bps/leg and own impact on both legs at entry vsol (conservative).
DROP TABLE IF EXISTS ixg.fwd CASCADE;
CREATE UNLOGGED TABLE ixg.fwd AS
SELECT e.*,
  f.px AS px20,
  f.ts AS ts20,
  f.vsol_lp AS vsol20_lp,
  EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
  e.vsol_lp / 1e9::double precision AS vsol
FROM ixg.ev e
LEFT JOIN LATERAL (
  SELECT t.px, t.ts, t.vsol_lp
  FROM ixg.tape t
  WHERE t.mint = e.mint
    AND t.ts > e.ts
    AND t.ts <= e.ts + interval '20 seconds'
    AND (t.slot > e.slot OR (t.slot = e.slot AND t.tx_index > e.tx_index))
    AND t.px IS NOT NULL
  ORDER BY t.ts DESC, t.slot DESC, t.tx_index DESC
  LIMIT 1
) f ON true;

ALTER TABLE ixg.fwd ADD COLUMN net20 double precision;
UPDATE ixg.fwd SET net20 =
  CASE WHEN px IS NULL OR px20 IS NULL OR vsol_lp IS NULL OR vsol_lp <= 0 THEN NULL
  ELSE
    ((px20 * (1.0 - 0.10e9 / GREATEST(vsol_lp, 1)))
     / (px  * (1.0 + 0.10e9 / GREATEST(vsol_lp, 1)))
     * 0.9875 / 1.0125) - 1.0
  END;
