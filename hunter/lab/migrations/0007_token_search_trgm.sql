-- ============================================================================
-- Trigram indexes for the Tokens page search (lab only)
--
-- The Tokens page search box matches `mint OR symbol` as a case-insensitive
-- substring (`handlers::tokens::sql::search_clause`: `LOWER(col) LIKE '%x%'`).
-- Without an index every search reads the whole `tokens` heap (1.3 GB at 1.8M
-- tokens): 0.6 s warm, past the api pool's 8 s statement timeout cold. With
-- both indexes the count is a BitmapOr of two GIN scans: ~20-70 ms. The OR
-- needs both — one alone still scans the heap.
--
-- Lab-only: on the live box every index on `tokens` is a write on the ingest
-- path. Expressions must match `search_clause` exactly (`LOWER(col)`).
-- Cost: ~540 MB on disk at 1.8M tokens; ~36 s to build once.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_tokens_symbol_lower_trgm
    ON tokens USING gin (LOWER(symbol) gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_tokens_mint_lower_trgm
    ON tokens USING gin (LOWER(mint_address) gin_trgm_ops);
