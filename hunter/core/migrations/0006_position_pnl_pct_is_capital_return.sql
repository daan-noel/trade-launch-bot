-- ===========================================================================
-- 0006_position_pnl_pct_is_capital_return — `strategy_position_pnl.pnl_pct`
-- becomes a MONEY return instead of a PRICE return.
--
-- Was:  (exit_price - entry_price) / entry_price * 100
-- Now:  realized_pnl_lamports / entry_lamports * 100
--
-- WHY (two independent defects, both of which made the % disagree with the ◎
-- sitting next to it on the same row):
--
-- 1. NO EXECUTION COST. The price ratio charges nothing. A round trip pays the
--    venue fee on BOTH legs (125 bps/leg, measured 2026-07-28) plus a fixed tip
--    per leg plus our own price impact, so a position needs roughly a +4% price
--    move just to break even. Every trade between 0% and break-even therefore
--    rendered a GREEN percent beside a RED SOL figure. `realized_pnl_sol` is
--    measured from the lamports that actually moved, so it already carries every
--    one of those costs; dividing it by the capital deployed keeps them.
--
-- 2. SCALE-OUT BLINDNESS. `exit_price` stamps the LAST sell leg only, while the
--    SOL figure sums every leg via `exit_sol_lamports_total`. On a laddered exit
--    the two headline numbers described different trades. Both now read the same
--    CASE the `realized_pnl_sol` column above them already uses.
--
-- Because the denominator is capital (always > 0), the sign of `pnl_pct` can no
-- longer disagree with the sign of `realized_pnl_sol` — the same guarantee
-- `hunter_core::strategies::kernel::weighted_return_pct` gives the aggregate
-- surfaces, now at per-position grain.
--
-- SSOT: this view, `StrategyPosition::pnl_pct` (models/strategy.rs), and
-- `PNL_PCT_SQL` (the positions table's sort/filter expression in
-- strategy_repo.rs) are three spellings of ONE formula. Change one, change all
-- three; `pnl_sql_columns_share_one_numerator` guards the two SQL copies.
--
-- FORWARD-ONLY, no data rewrite: the view is derived, never stored. Every
-- historical position's percent is simply recomputed on read, so already-closed
-- rows will show a SMALLER (and for sub-break-even winners, a NEGATIVE) percent
-- than they did before — that is the correction, not a regression. Nothing in
-- `strategy_run_metrics` is touched: those columns were stamped at rollup time
-- from the old formula and are not recomputable from a view (a run finalized
-- before this migration keeps its price-based mean; runs finalized after it get
-- the money-based one).
--
-- NULL semantics are unchanged: NULL whenever the chosen exit column or
-- `entry_lamports` is absent, so an open position still has no percent.
-- ===========================================================================

DROP VIEW IF EXISTS strategy_position_pnl;
CREATE VIEW strategy_position_pnl AS
SELECT
    p.*,
    -- exit_lamports/entry_lamports are lamports (BIGINT); divide back to human SOL
    -- so realized_pnl_sol matches StrategyPosition::realized_pnl_sol() (f64 SOL).
    -- Prefer the running aggregate once any sell leg has landed (scale-out or
    -- single-leg); legacy End rows keep sold_token_amount = 0 and use exit_lamports.
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8 / 1e9) AS realized_pnl_sol,
    -- Same numerator as realized_pnl_sol, over the capital deployed.
    ((CASE WHEN p.sold_token_amount > 0
           THEN p.exit_sol_lamports_total
           ELSE p.exit_lamports
      END - p.entry_lamports)::float8
     / NULLIF(p.entry_lamports, 0) * 100.0)   AS pnl_pct,
    CASE WHEN p.entry_time IS NOT NULL AND p.exit_time IS NOT NULL
         THEN EXTRACT(EPOCH FROM (p.exit_time - p.entry_time)) END AS holding_secs,
    (p.status = 'End' AND p.exit_time IS NOT NULL)                 AS is_closed
FROM strategy_positions p;
