-- Live facts at the combined-machine completing print.
-- Does not drop ixg.cm_cand / fall / dmint. Fillable rows only.
-- he1 = 8dtx buys this mint in S or S+1 (habit, not a gate).
-- dslot = buy-gap length ending at this slot.
-- n_sell_gap = curve sells in [S-5, S).
-- init_sol / cashback are create-time, live at the decision.

SET statement_timeout = 0;
SET work_mem = '64MB';
SET maintenance_work_mem = '256MB';
SET max_parallel_workers_per_gather = 0;
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.cm_fact CASCADE;
DROP TABLE IF EXISTS ixg.cm_sgap CASCADE;
DROP TABLE IF EXISTS ixg.cm_sslot CASCADE;
DROP TABLE IF EXISTS ixg.cm_dslot CASCADE;
DROP TABLE IF EXISTS ixg.cm_his CASCADE;
DROP TABLE IF EXISTS ixg.cm_fmint CASCADE;

CREATE UNLOGGED TABLE ixg.cm_fmint AS
SELECT DISTINCT mint FROM ixg.cm_cand;
CREATE INDEX ON ixg.cm_fmint (mint);

CREATE UNLOGGED TABLE ixg.cm_his AS
SELECT DISTINCT mint, slot
FROM w8.buys;
CREATE INDEX ON ixg.cm_his (mint, slot);

CREATE UNLOGGED TABLE ixg.cm_dslot AS
SELECT mint, slot, dslot
FROM (
  SELECT
    f.mint, f.slot,
    f.slot - lag(f.slot) OVER (PARTITION BY f.mint ORDER BY f.slot, f.tx_index)
      AS dslot,
    row_number() OVER (PARTITION BY f.mint, f.slot ORDER BY f.tx_index) AS rn
  FROM ixg.fall f
  JOIN ixg.cm_fmint m ON m.mint = f.mint
  WHERE f.trade_type = 'buy'
) s
WHERE rn = 1;
CREATE INDEX ON ixg.cm_dslot (mint, slot);

CREATE UNLOGGED TABLE ixg.cm_sslot AS
SELECT
  f.mint, f.slot,
  count(*) FILTER (WHERE f.trade_type = 'sell')::int AS n_sell,
  COALESCE(sum(f.sol_lp) FILTER (WHERE f.trade_type = 'sell'), 0)
    / 1e9::double precision AS sol_sell
FROM ixg.fall f
JOIN ixg.cm_fmint m ON m.mint = f.mint
GROUP BY f.mint, f.slot;
CREATE INDEX ON ixg.cm_sslot (mint, slot);

CREATE UNLOGGED TABLE ixg.cm_sgap AS
SELECT
  c.mint, c.slot,
  COALESCE(sum(s.n_sell), 0)::int AS n_sell_gap,
  COALESCE(sum(s.sol_sell), 0) AS sol_sell_gap
FROM (SELECT DISTINCT mint, slot FROM ixg.cm_cand WHERE fillable) c
LEFT JOIN ixg.cm_sslot s
  ON s.mint = c.mint AND s.slot >= c.slot - 5 AND s.slot < c.slot
GROUP BY c.mint, c.slot;
CREATE INDEX ON ixg.cm_sgap (mint, slot);

CREATE UNLOGGED TABLE ixg.cm_fact AS
SELECT
  e.mint, e.slot, e.tx_index, e.ts, e.fam, e.shape, e.fillable,
  e.this_tmpl, e.this_prog, e.fam_n, e.fam_sol, e.run_sol, e.run_nwal,
  e.run_ntmpl, e.nwal_new, e.nwal_rep, e.this_sol, e.this_new, e.this_work,
  e.vsol_now, e.vsol_pre, e.rn, e.tight, e.trail, e.created_at,
  EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
  g.dslot,
  COALESCE(sg.n_sell_gap, 0) AS n_sell_gap,
  COALESCE(sg.sol_sell_gap, 0) AS sol_sell_gap,
  tok.initial_buy_lamports / 1e9::double precision AS init_sol,
  tok.is_cashback_enabled AS cashback,
  (h0.mint IS NOT NULL OR h1.mint IS NOT NULL) AS he1,
  (h1.mint IS NOT NULL AND h0.mint IS NULL) AS he_causal
FROM ixg.cm_cand e
LEFT JOIN ixg.cm_dslot g ON g.mint = e.mint AND g.slot = e.slot
LEFT JOIN ixg.cm_sgap sg ON sg.mint = e.mint AND sg.slot = e.slot
LEFT JOIN tokens tok ON tok.mint_address = e.mint
LEFT JOIN ixg.cm_his h0 ON h0.mint = e.mint AND h0.slot = e.slot
LEFT JOIN ixg.cm_his h1 ON h1.mint = e.mint AND h1.slot = e.slot + 1
WHERE e.fillable;
CREATE INDEX ON ixg.cm_fact (mint, ts);
CREATE INDEX ON ixg.cm_fact (mint, slot, tx_index);
