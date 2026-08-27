-- Old-on-chain vs born-this-mint, on the same first-on-mint burst members.
-- A wal_new wallet is OLD if its first tape buy is an earlier slot (must be
-- another mint). It is BORN if that first buy is this slot.
-- pre_mint = first buy is before this token's created_at (wallet predates mint).
-- hopper  = old, but first buy is after this mint launched (other tokens since).
-- Scratch in schema ixg. Does not drop burst / new-wallet tables.
-- Thermometer only — not a money book.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.ow_ids CASCADE;
CREATE UNLOGGED TABLE ixg.ow_ids AS
SELECT DISTINCT wallet_id
FROM ixg.bmem_new
WHERE wallet_id IS NOT NULL;
CREATE INDEX ON ixg.ow_ids (wallet_id);

-- First buy anywhere on the tape. One seq scan + hash agg, not 132k
-- compressed-chunk lookups. min(slot) and min(block_time) are the same
-- first buy in practice (slot is the chain order).
DROP TABLE IF EXISTS ixg.ow_fs CASCADE;
CREATE UNLOGGED TABLE ixg.ow_fs AS
SELECT
  t.wallet_id,
  min(t.block_time) AS ts0,
  min(t.slot) AS slot0
FROM trades t
JOIN ixg.ow_ids w ON w.wallet_id = t.wallet_id
WHERE t.trade_type = 'buy'
GROUP BY t.wallet_id;
CREATE INDEX ON ixg.ow_fs (wallet_id);

DROP TABLE IF EXISTS ixg.bmem_old CASCADE;
CREATE UNLOGGED TABLE ixg.bmem_old AS
SELECT
  b.*,
  fs.ts0 AS wal_ts0,
  fs.slot0 AS wal_slot0,
  tok.created_at,
  (b.wal_new AND fs.slot0 IS NOT NULL AND fs.slot0 < b.slot) AS wal_old,
  (b.wal_new AND (fs.slot0 IS NULL OR fs.slot0 >= b.slot)) AS wal_born,
  (b.wal_new AND fs.ts0 IS NOT NULL AND tok.created_at IS NOT NULL
    AND fs.ts0 < tok.created_at) AS wal_pre
FROM ixg.bmem_new b
LEFT JOIN ixg.ow_fs fs ON fs.wallet_id = b.wallet_id
LEFT JOIN tokens tok ON tok.mint_address = b.mint;
CREATE INDEX ON ixg.bmem_old (mint, slot);

DROP TABLE IF EXISTS ixg.bold CASCADE;
CREATE UNLOGGED TABLE ixg.bold AS
SELECT
  mint, slot,
  count(DISTINCT wallet_id) FILTER (WHERE wal_new)::int AS nwal_new,
  count(DISTINCT wallet_id) FILTER (WHERE wal_old)::int AS nwal_old,
  count(DISTINCT wallet_id) FILTER (WHERE wal_born)::int AS nwal_born,
  count(DISTINCT wallet_id) FILTER (WHERE wal_pre)::int AS nwal_pre,
  count(DISTINCT wallet_id) FILTER (WHERE wal_old AND NOT COALESCE(wal_pre, false))::int AS nwal_hop,
  count(DISTINCT wallet_id) FILTER (WHERE wal_new AND working)::int AS nwal_new_w,
  count(DISTINCT wallet_id) FILTER (WHERE wal_old AND working)::int AS nwal_old_w,
  count(DISTINCT wallet_id) FILTER (WHERE wal_born AND working)::int AS nwal_born_w,
  count(DISTINCT wallet_id) FILTER (WHERE wal_pre AND working)::int AS nwal_pre_w
FROM ixg.bmem_old
GROUP BY mint, slot;
CREATE INDEX ON ixg.bold (mint, slot);

ALTER TABLE ixg.bold ADD COLUMN age_kind text;
UPDATE ixg.bold SET age_kind = CASE
  WHEN nwal_new = 0 THEN 'no_new'
  WHEN nwal_old > 0 AND nwal_born = 0 THEN 'all_old'
  WHEN nwal_old = 0 AND nwal_born > 0 THEN 'all_born'
  WHEN nwal_old > 0 AND nwal_born > 0 THEN 'mixed_age'
  ELSE 'unk'
END;

ALTER TABLE ixg.bold ADD COLUMN age_kind_w text;
UPDATE ixg.bold SET age_kind_w = CASE
  WHEN nwal_new_w = 0 THEN 'no_new'
  WHEN nwal_old_w > 0 AND nwal_born_w = 0 THEN 'all_old'
  WHEN nwal_old_w = 0 AND nwal_born_w > 0 THEN 'all_born'
  WHEN nwal_old_w > 0 AND nwal_born_w > 0 THEN 'mixed_age'
  ELSE 'unk'
END;

ALTER TABLE ixg.bold ADD COLUMN origin_kind text;
UPDATE ixg.bold SET origin_kind = CASE
  WHEN nwal_new = 0 THEN 'no_new'
  WHEN nwal_pre > 0 AND nwal_born = 0 AND nwal_hop = 0 THEN 'all_pre'
  WHEN nwal_hop > 0 AND nwal_born = 0 AND nwal_pre = 0 THEN 'all_hop'
  WHEN nwal_born > 0 AND nwal_pre = 0 AND nwal_hop = 0 THEN 'all_born'
  ELSE 'mixed_origin'
END;
