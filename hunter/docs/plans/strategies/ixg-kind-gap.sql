-- Kind-gap: silence of first-on-mint working-template prints, not of all buys.
-- Completing print = the first such real print after >= 5 slots with no real
-- print. Create/first slot out. His fills already absent from ixg.bbuy.
-- Thermometer on his mints. Does not drop burst / new-wallet tables.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET synchronous_commit = off;

-- All non-launch buys on his mints, with first-on-mint + working flags.
DROP TABLE IF EXISTS ixg.kall CASCADE;
CREATE UNLOGGED TABLE ixg.kall AS
SELECT
  b.*,
  (b.wallet_id IS NOT NULL AND w0.slot0 = b.slot) AS wal_new,
  (
    (
      (b.program IN ('Axiom Trade', 'Photon', 'Terminal', 'GMGN', 'GMGN Bot')
        AND b.cu AND b.ata AND b.fee AND NOT b.seed)
      OR (b.program IN ('Bloom', 'Bloom Router')
        AND b.cu AND b.fee AND NOT b.ata AND NOT b.seed)
    )
  ) AS working
FROM ixg.bbuy b
LEFT JOIN ixg.wal0 w0
  ON w0.mint = b.mint AND w0.wallet_id = b.wallet_id;
CREATE INDEX ON ixg.kall (mint, slot, tx_index);

ALTER TABLE ixg.kall ADD COLUMN is_real boolean;
UPDATE ixg.kall SET is_real = (COALESCE(wal_new, false) AND working);

-- Real prints only, with slot-gap back to the previous real print.
DROP TABLE IF EXISTS ixg.kreal CASCADE;
CREATE UNLOGGED TABLE ixg.kreal AS
SELECT
  k.*,
  slot - lag(slot) OVER (PARTITION BY mint ORDER BY slot, tx_index) AS dslot_real
FROM ixg.kall k
WHERE is_real;
CREATE INDEX ON ixg.kreal (mint, slot, tx_index);

-- Completing print: first real print after a kind-gap of >= 5 slots
-- (or the mint's first real print), not on the mint's first buy-slot.
DROP TABLE IF EXISTS ixg.kbrk CASCADE;
CREATE UNLOGGED TABLE ixg.kbrk AS
SELECT
  r.mint, r.slot, r.tx_index, r.ts, r.sol, r.tmpl, r.program, r.wallet_id,
  r.dslot_real,
  (r.dslot_real IS NULL OR r.dslot_real >= 5) AS kind5,
  COALESCE(q.quiet5, false) AS buy5
FROM ixg.kreal r
JOIN ixg.mint0 m ON m.mint = r.mint AND r.slot > m.slot0
LEFT JOIN ixg.quiet q ON q.mint = r.mint AND q.slot = r.slot
WHERE r.dslot_real IS NULL OR r.dslot_real >= 5;
CREATE INDEX ON ixg.kbrk (mint, slot, tx_index);

ALTER TABLE ixg.kbrk ADD COLUMN he1 boolean;
ALTER TABLE ixg.kbrk ADD COLUMN he_causal boolean;
UPDATE ixg.kbrk b SET
  he1 = EXISTS (
    SELECT 1 FROM ixg.his_slot h
    WHERE h.mint = b.mint AND h.slot IN (b.slot, b.slot + 1)
  ),
  he_causal = EXISTS (
    SELECT 1 FROM ixg.his_slot h2
    WHERE h2.mint = b.mint AND h2.slot = b.slot + 1
  ) AND NOT EXISTS (
    SELECT 1 FROM ixg.his_slot h
    WHERE h.mint = b.mint AND h.slot = b.slot
  );
