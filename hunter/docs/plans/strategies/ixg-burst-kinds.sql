-- Gap-then-burst kinds, named by build template.
-- Create/launch rows stay out of the burst. First slot of each mint stays out
-- (burst is a resume after quiet, not the opening tape).
-- Every kind is kept. Response = he buys this mint in S or S+1.
-- Scratch in schema ixg; does not drop the existing microscope tables.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

-- Mint's first observed buy-slot in this corpus.
DROP TABLE IF EXISTS ixg.mint0 CASCADE;
CREATE UNLOGGED TABLE ixg.mint0 AS
SELECT mint, min(slot) AS slot0
FROM ixg.pb
GROUP BY 1;
CREATE INDEX ON ixg.mint0 (mint);

-- Wallet on each buy (his fills already absent from ixg.pb).
DROP TABLE IF EXISTS ixg.wal CASCADE;
CREATE UNLOGGED TABLE ixg.wal AS
SELECT t.mint_address AS mint, t.slot, t.tx_index, t.wallet_id
FROM trades t
WHERE t.venue = 'curve'
  AND t.trade_type = 'buy'
  AND t.block_time >= '2026-07-26'
  AND t.block_time <  '2026-08-23'
  AND EXISTS (SELECT 1 FROM ixg.mint0 m WHERE m.mint = t.mint_address);
CREATE INDEX ON ixg.wal (mint, slot, tx_index);

-- One row per non-launch buy, with template + wallet.
DROP TABLE IF EXISTS ixg.bbuy CASCADE;
CREATE UNLOGGED TABLE ixg.bbuy AS
SELECT
  p.mint, p.ts, p.slot, p.tx_index, p.sol, p.pid,
  d.program, d.tmpl, d.actor, d.cu, d.ata, d.nonce, d.seed, d.fee,
  w.wallet_id
FROM ixg.pb p
JOIN ixg.dict d USING (pid)
LEFT JOIN ixg.wal w
  ON w.mint = p.mint AND w.slot = p.slot AND w.tx_index = p.tx_index
WHERE NOT d.launch;
CREATE INDEX ON ixg.bbuy (mint, slot, tx_index);

-- Silence-breaking slots that are not the mint's first slot.
DROP TABLE IF EXISTS ixg.bslot CASCADE;
CREATE UNLOGGED TABLE ixg.bslot AS
SELECT q.mint, q.slot
FROM ixg.quiet q
JOIN ixg.mint0 m ON m.mint = q.mint
WHERE q.quiet5
  AND q.slot > m.slot0;
CREATE INDEX ON ixg.bslot (mint, slot);

-- Buys that sit in those slots.
DROP TABLE IF EXISTS ixg.bmem CASCADE;
CREATE UNLOGGED TABLE ixg.bmem AS
SELECT b.*
FROM ixg.bbuy b
JOIN ixg.bslot s USING (mint, slot);
CREATE INDEX ON ixg.bmem (mint, slot, tx_index);

-- Completed-slot burst (all kinds kept).
DROP TABLE IF EXISTS ixg.burst CASCADE;
CREATE UNLOGGED TABLE ixg.burst AS
SELECT
  mint, slot,
  count(*)::int AS ntx,
  count(DISTINCT wallet_id)::int AS nwal,
  count(DISTINCT tmpl)::int AS ntmpl,
  count(DISTINCT program)::int AS nprog,
  count(DISTINCT actor)::int AS nact,
  sum(sol) AS tot,
  min(sol) AS mn,
  max(sol) AS mx,
  CASE WHEN min(sol) > 0 THEN max(sol) / min(sol) END AS spread,
  (array_agg(tmpl ORDER BY sol DESC))[1] AS top_tmpl,
  (array_agg(program ORDER BY sol DESC))[1] AS top_prog,
  (array_agg(actor ORDER BY sol DESC))[1] AS top_act,
  sum(sol) FILTER (WHERE actor = 'retail') AS sol_retail,
  sum(sol) FILTER (WHERE actor = 'app') AS sol_app,
  sum(sol) FILTER (WHERE actor = 'racer') AS sol_racer,
  sum(sol) FILTER (WHERE actor = 'harvester') AS sol_harv,
  sum(sol) FILTER (WHERE actor = 'prepared') AS sol_prep,
  bool_or(actor = 'racer') AS has_racer,
  bool_or(actor = 'harvester') AS has_harv,
  bool_or(seed) AS has_seed,
  bool_or(ata) AS has_ata,
  bool_or(cu) AS has_cu
FROM ixg.bmem
GROUP BY mint, slot;
CREATE INDEX ON ixg.burst (mint, slot);

ALTER TABLE ixg.burst ADD COLUMN kind text;
UPDATE ixg.burst SET kind = CASE
  WHEN ntx = 1 THEN 'solo'
  WHEN ntmpl = 1 AND nwal = 1 THEN 'same_tmpl_1wal'
  WHEN ntmpl = 1 AND nwal >= 2 THEN 'same_tmpl_nwal'
  WHEN ntmpl >= 2 AND nwal = 1 THEN 'multi_tmpl_1wal'
  ELSE 'multi_tmpl_nwal'
END;

ALTER TABLE ixg.burst ADD COLUMN he1 boolean;
ALTER TABLE ixg.burst ADD COLUMN he_causal boolean;
UPDATE ixg.burst b SET
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

-- Prefix of each burst: state after each print = a candidate crossing.
DROP TABLE IF EXISTS ixg.bpre CASCADE;
CREATE UNLOGGED TABLE ixg.bpre AS
SELECT
  a.mint, a.slot, a.tx_index, a.ts, a.sol AS this_sol,
  a.tmpl AS this_tmpl, a.program AS this_prog, a.actor AS this_act,
  a.wallet_id AS this_wal,
  count(*)::int AS rn,
  sum(b.sol) AS run_sol,
  count(DISTINCT b.wallet_id)::int AS run_nwal,
  count(DISTINCT b.tmpl)::int AS run_ntmpl,
  count(DISTINCT b.program)::int AS run_nprog,
  count(*) FILTER (WHERE b.tmpl = a.tmpl)::int AS fam_n,
  sum(b.sol) FILTER (WHERE b.tmpl = a.tmpl) AS fam_sol,
  count(DISTINCT b.wallet_id) FILTER (WHERE b.tmpl = a.tmpl)::int AS fam_nwal,
  count(*) FILTER (WHERE b.actor = a.actor)::int AS act_n,
  sum(b.sol) FILTER (WHERE b.actor = a.actor) AS act_sol,
  bool_or(b.actor = 'racer') AS run_has_racer,
  bool_or(b.seed) AS run_has_seed
FROM ixg.bmem a
JOIN ixg.bmem b
  ON b.mint = a.mint AND b.slot = a.slot AND b.tx_index <= a.tx_index
GROUP BY
  a.mint, a.slot, a.tx_index, a.ts, a.sol,
  a.tmpl, a.program, a.actor, a.wallet_id;
CREATE INDEX ON ixg.bpre (mint, slot, tx_index);

ALTER TABLE ixg.bpre ADD COLUMN kind text;
UPDATE ixg.bpre SET kind = CASE
  WHEN rn = 1 THEN 'solo'
  WHEN run_ntmpl = 1 AND run_nwal = 1 THEN 'same_tmpl_1wal'
  WHEN run_ntmpl = 1 AND run_nwal >= 2 THEN 'same_tmpl_nwal'
  WHEN run_ntmpl >= 2 AND run_nwal = 1 THEN 'multi_tmpl_1wal'
  ELSE 'multi_tmpl_nwal'
END;

ALTER TABLE ixg.bpre ADD COLUMN he_after boolean;
ALTER TABLE ixg.bpre ADD COLUMN he_causal boolean;
UPDATE ixg.bpre p SET
  he_after = EXISTS (
    SELECT 1 FROM ixg.his_slot h
    WHERE h.mint = p.mint AND h.slot = p.slot AND h.tx_index > p.tx_index
  ) OR EXISTS (
    SELECT 1 FROM ixg.his_slot h2
    WHERE h2.mint = p.mint AND h2.slot = p.slot + 1
  ),
  he_causal = EXISTS (
    SELECT 1 FROM ixg.his_slot h2
    WHERE h2.mint = p.mint AND h2.slot = p.slot + 1
  ) AND NOT EXISTS (
    SELECT 1 FROM ixg.his_slot h
    WHERE h.mint = p.mint AND h.slot = p.slot
  );
