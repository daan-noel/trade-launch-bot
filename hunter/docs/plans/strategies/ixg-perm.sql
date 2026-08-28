-- Permissions at the burst, looking only backward.
-- Inside door-passed named families only. Metrics match the engine:
--   m_flow_window(10/30).buy/sell/gross_flow/net_flow
--   m_state.liquidity = vsol - 30
--   m_price_lifetime.trail / rise
--   age from tokens.created_at
-- Quiet window is [t0-W, t0): the burst's own prints are out.

SET statement_timeout = 0;
SET work_mem = '2GB';
SET synchronous_commit = off;

DROP TABLE IF EXISTS ixg.candb CASCADE;
CREATE UNLOGGED TABLE ixg.candb AS
SELECT b.*
FROM ixg.burst b
JOIN ixg.tok t ON t.mint = b.mint
WHERE t.c_ata
  AND t.init_band NOT IN ('lt0.2', 'unk')
  AND t.fs_band NOT IN ('lt0.5', 'unk')
  AND b.kind IN ('same_tmpl_nwal', 'multi_tmpl_nwal')
  AND b.tot >= 0.9 AND b.tot < 4;
CREATE INDEX ON ixg.candb (mint, slot);

DROP TABLE IF EXISTS ixg.bst CASCADE;
CREATE UNLOGGED TABLE ixg.bst AS
SELECT c.mint, c.slot, c.kind, c.tot, c.ntx, c.nwal, c.ntmpl, c.top_tmpl, c.top_act,
       c.has_racer, c.he1, c.he_causal,
       min(m.ts) AS t0,
       min(m.tx_index) AS tx0
FROM ixg.candb c
JOIN ixg.bmem m USING (mint, slot)
GROUP BY c.mint, c.slot, c.kind, c.tot, c.ntx, c.nwal, c.ntmpl, c.top_tmpl, c.top_act,
         c.has_racer, c.he1, c.he_causal;
CREATE INDEX ON ixg.bst (mint, t0);

DROP TABLE IF EXISTS ixg.mtape CASCADE;
CREATE UNLOGGED TABLE ixg.mtape AS
SELECT
  t.mint_address AS mint,
  t.slot,
  t.tx_index,
  t.block_time AS ts,
  t.trade_type,
  t.amount_lamports AS sol_lp,
  t.reserve_lamports AS vsol_lp,
  CASE WHEN t.reserve_token > 0
    THEN t.reserve_lamports::double precision / t.reserve_token::double precision
  END AS px
FROM trades t
WHERE t.venue = 'curve'
  AND t.block_time >= '2026-07-26'
  AND t.block_time <  '2026-08-23'
  AND EXISTS (SELECT 1 FROM ixg.bst b WHERE b.mint = t.mint_address);
CREATE INDEX ON ixg.mtape (mint, ts);
CREATE INDEX ON ixg.mtape (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.mrun CASCADE;
CREATE UNLOGGED TABLE ixg.mrun AS
SELECT
  mint, slot, tx_index, ts, trade_type, sol_lp, vsol_lp, px,
  max(px) FILTER (WHERE px IS NOT NULL)
    OVER (PARTITION BY mint ORDER BY slot, tx_index) AS px_peak,
  min(px) FILTER (WHERE px IS NOT NULL)
    OVER (PARTITION BY mint ORDER BY slot, tx_index) AS px_trough
FROM ixg.mtape;
CREATE INDEX ON ixg.mrun (mint, slot, tx_index);

DROP TABLE IF EXISTS ixg.perm CASCADE;
CREATE UNLOGGED TABLE ixg.perm AS
SELECT
  b.*,
  EXTRACT(EPOCH FROM (b.t0 - tok.created_at)) AS age_s,
  pre.vsol,
  pre.px,
  pre.px_peak,
  pre.px_trough,
  CASE WHEN pre.px_peak > 0 AND pre.px IS NOT NULL
    THEN 100.0 * (pre.px_peak - pre.px) / pre.px_peak END AS trail,
  CASE WHEN pre.px_trough > 0 AND pre.px IS NOT NULL
    THEN 100.0 * (pre.px - pre.px_trough) / pre.px_trough END AS rise,
  COALESCE(w.buy_10, 0) AS buy_10,
  COALESCE(w.sell_10, 0) AS sell_10,
  COALESCE(w.buy_10, 0) + COALESCE(w.sell_10, 0) AS gross_10,
  COALESCE(w.buy_10, 0) - COALESCE(w.sell_10, 0) AS net_10,
  COALESCE(w.buy_30, 0) AS buy_30,
  COALESCE(w.sell_30, 0) AS sell_30,
  COALESCE(w.buy_30, 0) + COALESCE(w.sell_30, 0) AS gross_30
FROM ixg.bst b
JOIN ixg.tok tok ON tok.mint = b.mint
LEFT JOIN LATERAL (
  SELECT
    r.vsol_lp / 1e9::double precision AS vsol,
    r.px, r.px_peak, r.px_trough
  FROM ixg.mrun r
  WHERE r.mint = b.mint
    AND (r.slot < b.slot OR (r.slot = b.slot AND r.tx_index < b.tx0))
  ORDER BY r.slot DESC, r.tx_index DESC
  LIMIT 1
) pre ON true
LEFT JOIN LATERAL (
  SELECT
    sum(sol_lp) FILTER (WHERE trade_type = 'buy'  AND ts >= b.t0 - interval '10 seconds')
      / 1e9::double precision AS buy_10,
    sum(sol_lp) FILTER (WHERE trade_type = 'sell' AND ts >= b.t0 - interval '10 seconds')
      / 1e9::double precision AS sell_10,
    sum(sol_lp) FILTER (WHERE trade_type = 'buy'  AND ts >= b.t0 - interval '30 seconds')
      / 1e9::double precision AS buy_30,
    sum(sol_lp) FILTER (WHERE trade_type = 'sell' AND ts >= b.t0 - interval '30 seconds')
      / 1e9::double precision AS sell_30
  FROM ixg.mtape x
  WHERE x.mint = b.mint
    AND x.ts >= b.t0 - interval '30 seconds'
    AND x.ts <  b.t0
) w ON true;

CREATE INDEX ON ixg.perm (kind);
