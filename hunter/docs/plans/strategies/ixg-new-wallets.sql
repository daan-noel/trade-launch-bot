-- First-on-this-mint vs repeat, on the same gap-then-burst events.
-- Wallet grain: a burst member is NEW if that wallet has no curve buy on this
-- mint in any earlier slot. Second print of a new wallet in the same slot
-- still counts as a new wallet.
-- Scratch in schema ixg. Does not drop microscope / burst tables.
-- Thermometer only — not a money book.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

-- First observed buy-slot of each wallet on each mint (full curve tape on
-- these mints, including launch slots and mid-run). 2720 is in ixg.wal and
-- is a different wallet, so it does not flip a signal wallet to repeat.
DROP TABLE IF EXISTS ixg.wal0 CASCADE;
CREATE UNLOGGED TABLE ixg.wal0 AS
SELECT mint, wallet_id, min(slot) AS slot0
FROM ixg.wal
WHERE wallet_id IS NOT NULL
GROUP BY 1, 2;
CREATE INDEX ON ixg.wal0 (mint, wallet_id);

-- Per burst member.
DROP TABLE IF EXISTS ixg.bmem_new CASCADE;
CREATE UNLOGGED TABLE ixg.bmem_new AS
SELECT
  b.*,
  (b.wallet_id IS NOT NULL AND w0.slot0 = b.slot) AS wal_new,
  (b.wallet_id IS NOT NULL AND w0.slot0 IS NOT NULL AND w0.slot0 < b.slot) AS wal_rep,
  (
    (b.program IN ('Axiom Trade', 'Photon', 'Terminal', 'GMGN')
      AND b.cu AND b.ata AND b.fee AND NOT b.seed)
    OR (b.program = 'Bloom' AND b.cu AND b.fee AND NOT b.ata AND NOT b.seed)
  ) AS working
FROM ixg.bmem b
LEFT JOIN ixg.wal0 w0
  ON w0.mint = b.mint AND w0.wallet_id = b.wallet_id;
CREATE INDEX ON ixg.bmem_new (mint, slot);

-- Completed-slot: how many distinct new vs repeat wallets sit in the burst.
DROP TABLE IF EXISTS ixg.bnew CASCADE;
CREATE UNLOGGED TABLE ixg.bnew AS
SELECT
  mint, slot,
  count(DISTINCT wallet_id) FILTER (WHERE wal_new)::int AS nwal_new,
  count(DISTINCT wallet_id) FILTER (WHERE wal_rep)::int AS nwal_rep,
  count(*) FILTER (WHERE wallet_id IS NULL)::int AS n_unk,
  sum(sol) FILTER (WHERE wal_new) AS sol_new,
  sum(sol) FILTER (WHERE wal_rep) AS sol_rep,
  count(DISTINCT wallet_id) FILTER (WHERE wal_new AND working)::int AS nwal_new_w,
  count(DISTINCT wallet_id) FILTER (WHERE wal_rep AND working)::int AS nwal_rep_w,
  sum(sol) FILTER (WHERE wal_new AND working) AS sol_new_w,
  sum(sol) FILTER (WHERE wal_rep AND working) AS sol_rep_w,
  bool_or(ata) AS has_ata,
  count(*) FILTER (WHERE ata)::int AS n_ata,
  count(*) FILTER (WHERE ata AND wal_new)::int AS n_ata_new,
  count(*) FILTER (WHERE ata AND wal_rep)::int AS n_ata_rep
FROM ixg.bmem_new
GROUP BY mint, slot;
CREATE INDEX ON ixg.bnew (mint, slot);

ALTER TABLE ixg.bnew ADD COLUMN new_kind text;
UPDATE ixg.bnew SET new_kind = CASE
  WHEN nwal_new > 0 AND nwal_rep = 0 THEN 'all_new'
  WHEN nwal_new > 0 AND nwal_rep > 0 THEN 'mixed'
  WHEN nwal_new = 0 AND nwal_rep > 0 THEN 'all_rep'
  ELSE 'unk'
END;

ALTER TABLE ixg.bnew ADD COLUMN new_kind_w text;
UPDATE ixg.bnew SET new_kind_w = CASE
  WHEN nwal_new_w > 0 AND nwal_rep_w = 0 THEN 'all_new'
  WHEN nwal_new_w > 0 AND nwal_rep_w > 0 THEN 'mixed'
  WHEN nwal_new_w = 0 AND nwal_rep_w > 0 THEN 'all_rep'
  ELSE 'none'
END;
