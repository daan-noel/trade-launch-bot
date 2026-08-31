-- Cheap extract only. Hits are folded in Python (no slot self-join).
-- 3 days, all tokens, 2720 not a member. Schema ixb.

SET statement_timeout = 0;
SET work_mem = '1GB';
SET maintenance_work_mem = '256MB';
SET max_parallel_workers_per_gather = 0;
SET synchronous_commit = off;

DROP SCHEMA IF EXISTS ixb CASCADE;
CREATE SCHEMA ixb;

CREATE FUNCTION ixb.tmpl(ix jsonb) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT ixg.program(ix)
    || CASE WHEN ixg.has_prefix(ix, 'Compute Budget:%') THEN '|CU' ELSE '' END
    || CASE WHEN ixg.has_prefix(ix, 'Associated Token:%') THEN '|ATA' ELSE '' END
    || CASE WHEN ixg.has_label(ix, 'System Program: AdvanceNonceAccount') THEN '|N' ELSE '' END
    || CASE WHEN ixg.has_label(ix, 'System Program: CreateAccountWithSeed') THEN '|S' ELSE '' END
    || CASE WHEN ixg.has_label(ix, 'System Program: Transfer') THEN '|F' ELSE '' END
$$;

CREATE UNLOGGED TABLE ixb.tok AS
SELECT
  t.mint_address AS mint,
  t.created_at,
  t.creation_slot,
  t.initial_buy_lamports AS init_lp,
  COALESCE(i.first_slot_buy_lamports, 0) AS first_slot_lp,
  ixg.has_prefix(t.ix_labels, 'Associated Token:%') AS create_ata,
  ixg.has_prefix(t.ix_labels, 'Compute Budget:%') AS create_cu
FROM tokens t
LEFT JOIN tokens_info i ON i.mint_address = t.mint_address
WHERE t.created_at >= '2026-08-11'
  AND t.created_at <  '2026-08-14';
CREATE INDEX ON ixb.tok (mint);

CREATE UNLOGGED TABLE ixb.fall AS
SELECT
  t.mint_address AS mint,
  t.slot,
  t.tx_index,
  t.block_time AS ts,
  t.trade_type,
  t.wallet_id,
  t.amount_lamports AS sol_lp,
  t.reserve_lamports AS vsol_lp,
  t.ix_labels,
  CASE WHEN t.reserve_token > 0
    THEN t.reserve_lamports::double precision / t.reserve_token::double precision
  END AS px
FROM trades t
WHERE t.venue = 'curve'
  AND t.block_time >= '2026-08-11'
  AND t.block_time <  '2026-08-14 01:00:00'
  AND EXISTS (SELECT 1 FROM ixb.tok d WHERE d.mint = t.mint_address);
CREATE INDEX ON ixb.fall (mint, slot, tx_index);
CREATE INDEX ON ixb.fall (mint, ts);

CREATE UNLOGGED TABLE ixb.mem AS
SELECT
  f.mint, f.slot, f.tx_index, f.ts, f.wallet_id,
  f.sol_lp / 1e9::double precision AS sol,
  f.vsol_lp / 1e9::double precision AS vsol,
  f.px,
  ixg.program(f.ix_labels) AS program,
  ixb.tmpl(f.ix_labels) AS tmpl
FROM ixb.fall f
WHERE f.trade_type = 'buy'
  AND f.wallet_id IS DISTINCT FROM 2720
  AND f.ix_labels IS NOT NULL
  AND NOT ixg.has_prefix(f.ix_labels, 'Pump.Fun: Create%');
CREATE INDEX ON ixb.mem (mint, slot, tx_index);

CREATE UNLOGGED TABLE ixb.he AS
SELECT mint_address AS mint, slot, block_time AS ts
FROM trades
WHERE venue = 'curve'
  AND trade_type = 'buy'
  AND wallet_id = 2720
  AND block_time >= '2026-08-11'
  AND block_time <  '2026-08-14 01:00:00';
CREATE INDEX ON ixb.he (mint, slot);
