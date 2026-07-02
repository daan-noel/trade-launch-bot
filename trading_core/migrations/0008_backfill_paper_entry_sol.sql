-- ===========================================================================
-- strategy_positions — backfill corrupted paper entry/exit SOL amounts.
--
-- Before 2026-07-02 (commit c34de13), `resolve_paper_entry_tpsl1` recorded the
-- rule's SOL `buy_amount` (e.g. 0.01) directly as the raw token count instead
-- of computing `buy_amount / entry_price`. Rounded to the `BIGINT` raw-unit
-- column, that collapsed `entry_token_amount` to 0 — and since exit reused
-- `entry_token_amount` to price the sell, `exit_sol` collapsed to 0 too. Both
-- SOL columns being 0 made `exit_sol > entry_sol` (the win/loss predicate in
-- `positions_summary`/`is_win`) false for every affected row, regardless of
-- the real price move — undercounting wins in the summary while the
-- price-basis positions table showed the trade as a winner.
--
-- `buy_amount` is not frozen per run (`strategy_runs.params_snapshot` only
-- freezes strategy params, not `buy_amount`) and rules are editable, so there
-- is no exact historical record. Affected rows are assumed to have used the
-- rule's *current* `buy_amount` throughout (rules are rarely resized
-- mid-history) — acceptable for paper (non-capital) data.
-- ===========================================================================

WITH corrupted AS (
    SELECT sp.id,
           sp.entry_price,
           sp.exit_price,
           sr.buy_amount
    FROM strategy_positions sp
    JOIN strategy_rules sr ON sr.id = sp.rule_id
    WHERE sp.mode = 'paper'
      AND sp.entry_token_amount = 0
      AND sp.entry_price IS NOT NULL
      AND sp.entry_price > 0
),
recomputed AS (
    SELECT id,
           ROUND(buy_amount / entry_price)::BIGINT AS new_token_amount,
           ROUND(buy_amount * 1000000000.0)::BIGINT AS new_entry_sol,
           exit_price
    FROM corrupted
)
UPDATE strategy_positions sp
SET entry_token_amount = r.new_token_amount,
    entry_sol = r.new_entry_sol,
    exit_token_amount = CASE WHEN r.exit_price IS NOT NULL THEN r.new_token_amount ELSE sp.exit_token_amount END,
    exit_sol = CASE WHEN r.exit_price IS NOT NULL
                     THEN ROUND(r.new_token_amount * r.exit_price * 1000000000.0)::BIGINT
                     ELSE sp.exit_sol
               END
FROM recomputed r
WHERE sp.id = r.id;
