-- 0011_amm_pool_facts.sql — durable PumpSwap pool-layout facts per migrated token.
--
-- WHY. The executor caches the AMM pool layout (`needs_pool_v2`, the fee-share
-- marker, the creator vault pair, cashback flag) for a migrated token in an
-- in-memory map with no TTL, warmed for FREE by the live feed harvest
-- (`observe_amm_swap_accounts`, zero RPC). A restart wipes that map. A token whose
-- pool has since gone dead never re-harvests, so its sell falls to the cold RPC
-- path — and that path could not reconstruct the swap tail once the pool's recent
-- signatures were all failed exit attempts, stranding the position.
--
-- This table persists the harvested facts so they survive a restart: `live`
-- upserts them from a background loop as the trader learns them, and re-seeds the
-- trader cache for held migrated mints on boot — both with NO RPC. It is written
-- only when a NEW pool is first observed (not on the ~150 ms ingest flush), so it
-- carries none of the `tokens_info` write-amplification concerns (0009); the PK is
-- the only access path (point read + `= ANY($1)` batch seed), so no secondary
-- indexes.
--
-- All pubkeys are stored as base58 TEXT — this mirrors the executor's transport
-- DTO (`pump_trader::AmmPoolFacts`), which is the executor's own decoupled vocab;
-- the mint PK follows the hunter SSOT key name `mint_address`.

CREATE TABLE IF NOT EXISTS amm_pool_facts (
    mint_address                 TEXT PRIMARY KEY,
    pool                         TEXT        NOT NULL,
    base_mint                    TEXT        NOT NULL,
    quote_mint                   TEXT        NOT NULL,
    base_token_program           TEXT        NOT NULL,
    pool_base_token_account      TEXT        NOT NULL,
    pool_quote_token_account     TEXT        NOT NULL,
    coin_creator                 TEXT        NOT NULL,
    coin_creator_vault_ata       TEXT        NOT NULL,
    coin_creator_vault_authority TEXT        NOT NULL,
    is_cashback_coin             BOOLEAN     NOT NULL,
    fee_share_marker             TEXT,          -- NULL for cashback / pool_v2 pools
    needs_pool_v2                BOOLEAN     NOT NULL,
    updated_at                   TIMESTAMPTZ NOT NULL DEFAULT now()
);
