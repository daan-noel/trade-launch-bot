-- Creation-time door: he arms vs never touches.
-- Universe = tokens created in the w8 window. Not scored on ixg.burst (that set is
-- already his mints). Create-tx template is a door fact; it is not a burst member.
-- Do not pick a band by return.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.tok CASCADE;
CREATE UNLOGGED TABLE ixg.tok AS
SELECT
  t.mint_address AS mint,
  t.created_at,
  t.is_cashback_enabled AS cashback,
  t.is_mayhem_mode AS mayhem,
  t.initial_buy_lamports AS init_lp,
  t.cu_limit,
  t.cu_price,
  t.ix_labels AS create_ix,
  jsonb_array_length(t.ix_labels) AS nix,
  ixg.head(t.ix_labels) AS c_head,
  ixg.has_prefix(t.ix_labels, 'Compute Budget:%') AS c_cu,
  ixg.has_prefix(t.ix_labels, 'Associated Token:%') AS c_ata,
  ixg.has_label(t.ix_labels, 'System Program: AdvanceNonceAccount') AS c_nonce,
  ixg.has_label(t.ix_labels, 'System Program: CreateAccountWithSeed') AS c_seed,
  ixg.has_label(t.ix_labels, 'System Program: Transfer') AS c_fee,
  i.first_slot_buy_lamports AS fs_buy_lp,
  (t.meta ? 'uri') AS has_uri,
  (h.mint IS NOT NULL) AS his
FROM tokens t
LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = t.mint_address
WHERE t.created_at >= '2026-07-26'
  AND t.created_at <  '2026-08-23';

ALTER TABLE ixg.tok ADD COLUMN c_tmpl text;
UPDATE ixg.tok SET c_tmpl =
  CASE WHEN c_cu THEN 'CU' ELSE '' END
  || CASE WHEN c_ata THEN '|ATA' ELSE '' END
  || CASE WHEN c_nonce THEN '|N' ELSE '' END
  || CASE WHEN c_seed THEN '|S' ELSE '' END
  || CASE WHEN c_fee THEN '|F' ELSE '' END;
UPDATE ixg.tok SET c_tmpl = NULLIF(c_tmpl, '');

ALTER TABLE ixg.tok ADD COLUMN init_band text;
UPDATE ixg.tok SET init_band = CASE
  WHEN init_lp IS NULL THEN 'unk'
  WHEN init_lp <  200000000 THEN 'lt0.2'
  WHEN init_lp < 1000000000 THEN '0.2-1'
  WHEN init_lp < 2000000000 THEN '1-2'
  WHEN init_lp < 5000000000 THEN '2-5'
  WHEN init_lp < 10000000000 THEN '5-10'
  ELSE 'ge10'
END;

ALTER TABLE ixg.tok ADD COLUMN fs_band text;
UPDATE ixg.tok SET fs_band = CASE
  WHEN fs_buy_lp IS NULL THEN 'unk'
  WHEN fs_buy_lp <  500000000 THEN 'lt0.5'
  WHEN fs_buy_lp < 2000000000 THEN '0.5-2'
  WHEN fs_buy_lp < 5000000000 THEN '2-5'
  WHEN fs_buy_lp < 20000000000 THEN '5-20'
  ELSE 'ge20'
END;

CREATE INDEX ON ixg.tok (mint);
CREATE INDEX ON ixg.tok (his);
