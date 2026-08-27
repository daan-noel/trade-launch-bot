-- Full-tape money events: solo first-on-mint working print as a turn.
-- Reuses ixg.ncand / fall. Does not drop burst / new-wallet tables.
-- Trail = m_price_lifetime.trail at the last print before the completing
-- print. Gap sells = curve sells in slots [S-5, S).
-- Window and door are already on ncand (2026-08-11 .. 2026-08-23, vsol < 46).

SET statement_timeout = 0;
SET work_mem = '2GB';
SET maintenance_work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.tmint CASCADE;
CREATE UNLOGGED TABLE ixg.tmint AS
SELECT DISTINCT mint
FROM ixg.ncand
WHERE fam = 'solo_new' AND COALESCE(working, false);
CREATE INDEX ON ixg.tmint (mint);

DROP TABLE IF EXISTS ixg.trun CASCADE;
CREATE UNLOGGED TABLE ixg.trun AS
SELECT
  f.mint, f.slot, f.tx_index, f.ts, f.trade_type, f.sol_lp, f.vsol_lp, f.px,
  max(f.px) FILTER (WHERE f.px IS NOT NULL)
    OVER (PARTITION BY f.mint ORDER BY f.slot, f.tx_index) AS px_peak
FROM ixg.fall f
JOIN ixg.tmint m USING (mint);
CREATE INDEX ON ixg.trun (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.tlag CASCADE;
CREATE UNLOGGED TABLE ixg.tlag AS
SELECT
  mint, slot, tx_index,
  lag(px) OVER w AS px_pre,
  lag(px_peak) OVER w AS peak_pre,
  lag(trade_type) OVER w AS last_side
FROM ixg.trun
WINDOW w AS (PARTITION BY mint ORDER BY slot, tx_index);
CREATE INDEX ON ixg.tlag (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.tslot CASCADE;
CREATE UNLOGGED TABLE ixg.tslot AS
SELECT
  mint, slot,
  count(*) FILTER (WHERE trade_type = 'sell')::int AS n_sell,
  COALESCE(sum(sol_lp) FILTER (WHERE trade_type = 'sell'), 0)
    / 1e9::double precision AS sol_sell
FROM ixg.trun
GROUP BY mint, slot;
CREATE INDEX ON ixg.tslot (mint, slot);

DROP TABLE IF EXISTS ixg.tgap CASCADE;
CREATE UNLOGGED TABLE ixg.tgap AS
SELECT
  e.mint, e.slot,
  COALESCE(sum(g.n_sell), 0)::int AS n_sell_gap,
  COALESCE(sum(g.sol_sell), 0) AS sol_sell_gap
FROM (
  SELECT DISTINCT mint, slot
  FROM ixg.ncand
  WHERE fam = 'solo_new' AND COALESCE(working, false)
) e
LEFT JOIN ixg.tslot g
  ON g.mint = e.mint AND g.slot >= e.slot - 5 AND g.slot < e.slot
GROUP BY e.mint, e.slot;
CREATE INDEX ON ixg.tgap (mint, slot);

DROP TABLE IF EXISTS ixg.tcand CASCADE;
CREATE UNLOGGED TABLE ixg.tcand AS
SELECT
  e.mint, e.slot, e.tx_index, e.ts, e.fam, e.working,
  e.this_tmpl, e.vsol_pre, e.created_at,
  g.n_sell_gap,
  g.sol_sell_gap,
  l.last_side,
  l.px_pre,
  l.peak_pre,
  CASE WHEN l.peak_pre > 0 AND l.px_pre IS NOT NULL
    THEN 100.0 * (l.peak_pre - l.px_pre) / l.peak_pre END AS trail
FROM ixg.ncand e
JOIN ixg.tgap g USING (mint, slot)
LEFT JOIN ixg.tlag l USING (mint, slot, tx_index)
WHERE e.fam = 'solo_new' AND COALESCE(e.working, false);
CREATE INDEX ON ixg.tcand (mint, ts);
