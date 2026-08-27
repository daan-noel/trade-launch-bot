-- ixg: per-structure microscope -> build templates -> quiet-then-burst events
-- Scratch schema, safe to DROP SCHEMA ixg CASCADE.
-- Reads w8.pb / w8.pbx / w8.buys (8dtx's mints, his own buys already excluded from pb).

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP SCHEMA IF EXISTS ixg CASCADE;
CREATE SCHEMA ixg;

-- First non-boilerplate instruction names the client. Boilerplate is compute-budget,
-- ATA, token-program account setup, nonce/seed/fee transfers, memos.
CREATE FUNCTION ixg.head(ix jsonb) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT COALESCE(
    (SELECT x
     FROM jsonb_array_elements_text(ix) AS t(x)
     WHERE x NOT LIKE 'Compute Budget:%'
       AND x NOT LIKE 'Associated Token:%'
       AND x NOT LIKE 'Token Program:%'
       AND x NOT LIKE 'Token 2022:%'
       AND x NOT LIKE 'Memo Program:%'
       AND x NOT IN (
         'System Program: Transfer',
         'System Program: AdvanceNonceAccount',
         'System Program: CreateAccountWithSeed',
         'System Program: CreateAccount'
       )
     LIMIT 1),
    '(direct)'
  );
$$;

CREATE FUNCTION ixg.program(ix jsonb) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT CASE
    WHEN h = '(direct)' THEN '(direct)'
    WHEN h LIKE 'Pump.Fun: Create%' THEN 'launch'
    WHEN h LIKE 'Pump.Fun:%' THEN 'Pump.Fun'
    ELSE split_part(h, ':', 1)
  END
  FROM (SELECT ixg.head(ix) AS h) s;
$$;

CREATE FUNCTION ixg.has_label(ix jsonb, lab text) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT EXISTS (
    SELECT 1 FROM jsonb_array_elements_text(ix) AS t(x) WHERE x = lab
  );
$$;

CREATE FUNCTION ixg.has_prefix(ix jsonb, pfx text) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT EXISTS (
    SELECT 1 FROM jsonb_array_elements_text(ix) AS t(x) WHERE x LIKE pfx
  );
$$;

-- One row per distinct ix_labels hash on his mints.
CREATE UNLOGGED TABLE ixg.dict AS
SELECT
  pid,
  (array_agg(ix_labels))[1] AS ix_labels,
  ixg.head((array_agg(ix_labels))[1]) AS head,
  ixg.program((array_agg(ix_labels))[1]) AS program,
  ixg.has_prefix((array_agg(ix_labels))[1], 'Associated Token:%') AS ata,
  ixg.has_label((array_agg(ix_labels))[1], 'System Program: AdvanceNonceAccount') AS nonce,
  ixg.has_label((array_agg(ix_labels))[1], 'System Program: CreateAccountWithSeed') AS seed,
  ixg.has_label((array_agg(ix_labels))[1], 'System Program: Transfer') AS fee,
  ixg.has_prefix((array_agg(ix_labels))[1], 'Pump.Fun: Create%') AS launch,
  count(*) AS n_buys
FROM w8.pb
GROUP BY pid;

CREATE INDEX ON ixg.dict (pid);
CREATE INDEX ON ixg.dict (program);

-- Durable template: program + mechanical flags. Exact hashes rotate; these flags do not.
ALTER TABLE ixg.dict ADD COLUMN tmpl text;
UPDATE ixg.dict SET tmpl =
  program
  || CASE WHEN launch THEN '|LAUNCH' ELSE '' END
  || CASE WHEN ata THEN '|ATA' ELSE '' END
  || CASE WHEN nonce THEN '|N' ELSE '' END
  || CASE WHEN seed THEN '|S' ELSE '' END
  || CASE WHEN fee THEN '|F' ELSE '' END;

-- Actor class from flags, independent of program name.
-- racer = nonce AND seed (pre-signed disposable account).
-- harvester = seed without nonce.
-- launch = create+buy.
-- retail-ish = ATA + fee, no nonce/seed (first-time buyer paying an app).
ALTER TABLE ixg.dict ADD COLUMN actor text;
UPDATE ixg.dict SET actor = CASE
  WHEN launch THEN 'launch'
  WHEN nonce AND seed THEN 'racer'
  WHEN seed THEN 'harvester'
  WHEN nonce THEN 'prepared'
  WHEN ata AND fee THEN 'retail'
  WHEN fee THEN 'app'
  ELSE 'other'
END;

-- Slot x structure (his buys already absent from pb).
CREATE UNLOGGED TABLE ixg.slot_pid AS
SELECT
  mint, slot, pid,
  sum(sol) AS sol,
  count(*)::int AS n,
  min(tx_index) AS first_tx,
  max(tx_index) AS last_tx
FROM w8.pb
GROUP BY 1,2,3;

CREATE INDEX ON ixg.slot_pid (pid);
CREATE INDEX ON ixg.slot_pid (mint, slot);

CREATE UNLOGGED TABLE ixg.slot AS
SELECT
  mint, slot,
  count(*)::int AS ntx,
  count(DISTINCT pid)::int AS npid,
  sum(sol) AS tot
FROM w8.pb
GROUP BY 1,2;

CREATE INDEX ON ixg.slot (mint, slot);

CREATE UNLOGGED TABLE ixg.his_slot AS
SELECT mint, slot, min(tx_index) AS tx_index, count(*)::int AS n
FROM w8.buys
GROUP BY 1,2;

CREATE INDEX ON ixg.his_slot (mint, slot);

-- Quiet: no buy of any size on this mint in the previous 5 slots. Measuring device.
CREATE UNLOGGED TABLE ixg.quiet AS
SELECT mint, slot, (min(dslot_prev) IS NULL OR min(dslot_prev) >= 5) AS quiet5
FROM w8.pbx
GROUP BY 1,2;

CREATE INDEX ON ixg.quiet (mint, slot);

-- Per-structure response. he1 = he buys this mint in S or S+1.
-- ahead = structure strictly before his fill (same slot, lower tx_index, or he in S+1).
-- behind = structure strictly after his fill.
CREATE UNLOGGED TABLE ixg.pid_score AS
SELECT
  d.pid, d.program, d.tmpl, d.actor, d.ata, d.nonce, d.seed, d.fee, d.launch,
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
  -- causal: he not in S, fires in S+1
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
GROUP BY d.pid, d.program, d.tmpl, d.actor, d.ata, d.nonce, d.seed, d.fee, d.launch, d.n_buys;

CREATE INDEX ON ixg.pid_score (program);
CREATE INDEX ON ixg.pid_score (tmpl);

-- Universe base rates (his mints, every buy-slot).
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
  lift        = CASE WHEN b.resp        > 0 THEN p.resp        / b.resp        END,
  lift_quiet  = CASE WHEN b.resp_quiet  > 0 THEN p.resp_quiet  / b.resp_quiet  END,
  lift_solo   = CASE WHEN b.resp_solo   > 0 THEN p.resp_solo   / b.resp_solo   END,
  lift_causal = CASE WHEN b.resp_causal > 0 THEN p.resp_causal / b.resp_causal END,
  ahead_behind = CASE WHEN p.n_behind > 0 THEN p.n_ahead::numeric / p.n_behind END
FROM ixg.base b;
