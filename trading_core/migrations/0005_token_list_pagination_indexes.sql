-- ===========================================================================
-- Token-list DB-pagination indexes
--
-- The live bin now pages `GET /api/tokens` straight from Postgres over the WHOLE
-- token universe (100K+), instead of slicing a 25K in-RAM snapshot (see
-- `tokens-db-pagination-plan.md` / `api::handlers::tokens::sql`). The dynamic
-- ORDER BY sorts on `tokens_info` metric columns; without indexes each page does a
-- full sort node. Add indexes only for the sort columns the UI actually exposes as
-- defaults — the rest fall back to a sort node (acceptable; deep sorts are rare and
-- lab-heavy). Deep OFFSET still scans-and-discards; keyset paging is the future
-- upgrade if deep pages get used (noted in the plan).
--
-- IO-cost justification (EC2 4 GB guardrail): these back the sortable columns the
-- token table surfaces; a per-page sort of 100K+ rows is the alternative. The
-- volume / last_trade_at / is_dead indexes already exist (0001_init.sql) and are
-- reused. `created_at DESC` exists but is extended with `mint_address` for the
-- deterministic page tiebreak.
-- ===========================================================================

-- Default order + stable tiebreak (newest-first, mint as the tiebreak). Replaces
-- the created_at-only index for the paged path's exact ORDER BY.
CREATE INDEX IF NOT EXISTS idx_tokens_created_mint
    ON tokens (created_at DESC, mint_address DESC);

-- Numeric sort columns the token table exposes (tokens_info metrics). NULLS LAST
-- matches the query's `... NULLS LAST` so the index order serves the sort directly.
CREATE INDEX IF NOT EXISTS idx_tokens_info_trade_count
    ON tokens_info (trade_count DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_current_price
    ON tokens_info (current_price DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_price
    ON tokens_info (ath_price DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_tokens_info_ath_timestamp
    ON tokens_info (ath_timestamp DESC NULLS LAST);

-- NOTE (deliberately NOT indexed — documented):
--   * market_cap = current_price * initial_supply_token is a computed expression
--     across the join, so it can't be a plain column index; sorting it uses a sort
--     node. Acceptable (analysis-grade).
--   * first_slot_buy_sol / first_slot_sell_sol / cu_limit / cu_price / the buy-ix
--     JSON args are rarely sorted; left to a sort node until a real workload needs
--     them, to keep write-amplification (hot ingest path) minimal.
