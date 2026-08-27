-- Rebuild slot scoring with 8dtx's own fills removed from the candidate pool.
-- w8.pb still contains them; anti-join on (mint, slot, tx_index).

SET statement_timeout = 0;
SET work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.pb CASCADE;
CREATE UNLOGGED TABLE ixg.pb AS
SELECT p.mint, p.ts, p.slot, p.tx_index, p.sol, p.ix_labels, p.pid
FROM w8.pb p
LEFT JOIN w8.buys b
  ON b.mint = p.mint AND b.slot = p.slot AND b.tx_index = p.tx_index
WHERE b.mint IS NULL;

CREATE INDEX ON ixg.pb (pid);
CREATE INDEX ON ixg.pb (mint, slot, tx_index);

-- CU flag on dict; rebuild tmpl to include it.
ALTER TABLE ixg.dict ADD COLUMN IF NOT EXISTS cu boolean;
UPDATE ixg.dict SET cu = ixg.has_prefix(ix_labels, 'Compute Budget:%');

UPDATE ixg.dict SET tmpl =
  program
  || CASE WHEN launch THEN '|LAUNCH' ELSE '' END
  || CASE WHEN cu THEN '|CU' ELSE '' END
  || CASE WHEN ata THEN '|ATA' ELSE '' END
  || CASE WHEN nonce THEN '|N' ELSE '' END
  || CASE WHEN seed THEN '|S' ELSE '' END
  || CASE WHEN fee THEN '|F' ELSE '' END;

DROP TABLE IF EXISTS ixg.slot_pid CASCADE;
CREATE UNLOGGED TABLE ixg.slot_pid AS
SELECT mint, slot, pid,
  sum(sol) AS sol, count(*)::int AS n,
  min(tx_index) AS first_tx, max(tx_index) AS last_tx
FROM ixg.pb
GROUP BY 1,2,3;
CREATE INDEX ON ixg.slot_pid (pid);
CREATE INDEX ON ixg.slot_pid (mint, slot);

DROP TABLE IF EXISTS ixg.slot CASCADE;
CREATE UNLOGGED TABLE ixg.slot AS
SELECT mint, slot,
  count(*)::int AS ntx,
  count(DISTINCT pid)::int AS npid,
  sum(sol) AS tot
FROM ixg.pb
GROUP BY 1,2;
CREATE INDEX ON ixg.slot (mint, slot);

DROP TABLE IF EXISTS ixg.quiet CASCADE;
CREATE UNLOGGED TABLE ixg.quiet AS
SELECT mint, slot, (dslot IS NULL OR dslot >= 5) AS quiet5
FROM (
  SELECT mint, slot,
    slot - lag(slot) OVER (PARTITION BY mint ORDER BY slot, tx_index) AS dslot,
    row_number() OVER (PARTITION BY mint, slot ORDER BY tx_index) AS rn
  FROM ixg.pb
) s
WHERE rn = 1;
CREATE INDEX ON ixg.quiet (mint, slot);

DROP TABLE IF EXISTS ixg.pid_score CASCADE;
CREATE UNLOGGED TABLE ixg.pid_score AS
SELECT
  d.pid, d.program, d.tmpl, d.actor, d.ata, d.nonce, d.seed, d.fee, d.launch, d.cu,
  d.n_buys,
  count(*) AS n_slots,
  count(*) FILTER (WHERE q.quiet5) AS n_quiet,
  count(*) FILTER (WHERE s.ntx = 1) AS n_solo,
  count(*) FILTER (WHERE s.ntx >= 2) AS n_bundle,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)::numeric(8,5) AS resp,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE q.quiet5)::numeric(8,5) AS resp_quiet,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE s.ntx = 1)::numeric(8,5) AS resp_solo,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE s.ntx >= 2)::numeric(8,5) AS resp_bundle,
  avg((h.mint IS NULL AND h2.mint IS NOT NULL)::int)::numeric(8,5) AS resp_causal,
  count(*) FILTER (
    WHERE (h.mint IS NOT NULL AND h.tx_index > sp.first_tx) OR h2.mint IS NOT NULL
  ) AS n_ahead,
  count(*) FILTER (
    WHERE (h.mint IS NOT NULL AND h.tx_index < sp.first_tx) OR h0.mint IS NOT NULL
  ) AS n_behind
FROM ixg.slot_pid sp
JOIN ixg.dict d USING (pid)
JOIN ixg.slot s USING (mint, slot)
LEFT JOIN ixg.quiet q USING (mint, slot)
LEFT JOIN ixg.his_slot h  ON h.mint = sp.mint AND h.slot = sp.slot
LEFT JOIN ixg.his_slot h2 ON h2.mint = sp.mint AND h2.slot = sp.slot + 1
LEFT JOIN ixg.his_slot h0 ON h0.mint = sp.mint AND h0.slot = sp.slot - 1
GROUP BY d.pid, d.program, d.tmpl, d.actor, d.ata, d.nonce, d.seed, d.fee, d.launch, d.cu, d.n_buys;

DROP TABLE IF EXISTS ixg.base CASCADE;
CREATE UNLOGGED TABLE ixg.base AS
SELECT
  count(*) AS n_slots,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)::numeric(8,5) AS resp,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE q.quiet5)::numeric(8,5) AS resp_quiet,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE s.ntx = 1)::numeric(8,5) AS resp_solo,
  avg((h.mint IS NOT NULL OR h2.mint IS NOT NULL)::int)
    FILTER (WHERE s.ntx >= 2)::numeric(8,5) AS resp_bundle,
  avg((h.mint IS NULL AND h2.mint IS NOT NULL)::int)::numeric(8,5) AS resp_causal
FROM ixg.slot s
LEFT JOIN ixg.quiet q USING (mint, slot)
LEFT JOIN ixg.his_slot h  ON h.mint = s.mint AND h.slot = s.slot
LEFT JOIN ixg.his_slot h2 ON h2.mint = s.mint AND h2.slot = s.slot + 1;

ALTER TABLE ixg.pid_score ADD COLUMN lift numeric(8,3);
ALTER TABLE ixg.pid_score ADD COLUMN lift_quiet numeric(8,3);
ALTER TABLE ixg.pid_score ADD COLUMN lift_solo numeric(8,3);
ALTER TABLE ixg.pid_score ADD COLUMN lift_causal numeric(8,3);
ALTER TABLE ixg.pid_score ADD COLUMN ahead_behind numeric(8,3);

UPDATE ixg.pid_score p SET
  lift         = CASE WHEN b.resp        > 0 THEN p.resp        / b.resp        END,
  lift_quiet   = CASE WHEN b.resp_quiet  > 0 THEN p.resp_quiet  / b.resp_quiet  END,
  lift_solo    = CASE WHEN b.resp_solo   > 0 THEN p.resp_solo   / b.resp_solo   END,
  lift_causal  = CASE WHEN b.resp_causal > 0 THEN p.resp_causal / b.resp_causal END,
  ahead_behind = CASE WHEN p.n_behind > 0 THEN p.n_ahead::numeric / p.n_behind END
FROM ixg.base b;
