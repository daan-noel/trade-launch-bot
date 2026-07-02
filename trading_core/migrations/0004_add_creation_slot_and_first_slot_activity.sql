-- ===========================================================================
-- creation_slot (tokens) + first-slot buy/sell SOL (tokens_info)
--
-- `tokens.creation_slot` is a write-once creation fact: the Solana slot of the
-- token's create transaction (known the instant TokenCreated fires). Nothing
-- persisted it before (only `created_at`, a timestamp).
--
-- `tokens_info.first_slot_buy_sol` / `first_slot_sell_sol` are derived hot-metrics
-- (same category as `volume`/`trade_count`): the total buy/sell SOL, in lamports,
-- across every trade landing in the SAME slot as `creation_slot`. Accumulated
-- streaming in-memory in `TokenState::apply_aggregates` — no backfill query.
--
-- Edge case (documented, accepted): a same-slot trade delivered by the gRPC feed
-- AFTER a later-slot trade already closed the accumulation window under-counts,
-- matching how `is_dead`/`ath` already tolerate minor gRPC reordering.
-- ===========================================================================

ALTER TABLE tokens ADD COLUMN IF NOT EXISTS creation_slot BIGINT;

ALTER TABLE tokens_info
    ADD COLUMN IF NOT EXISTS first_slot_buy_sol  BIGINT,   -- lamports
    ADD COLUMN IF NOT EXISTS first_slot_sell_sol BIGINT;   -- lamports
