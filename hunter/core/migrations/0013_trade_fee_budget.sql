-- Trades carry the fee budget their sender chose, not just the build they used.
--
-- `ix_labels` already records that a transaction set a compute-unit limit, set a
-- price, and moved SOL with a transfer — and records nothing else about them. On
-- the live tape those labels sit on 96% / 95% / 84% of legs, so their PRESENCE
-- discriminates almost nothing; every bit of the signal is in the values, and the
-- values were being decoded and dropped. These three columns keep them.
--
-- WHAT THEY ARE, TOGETHER. A sender picks one thing — how much to spend to be
-- early — and the chain offers two rails to pay it on:
--
--   priority_lamports = ceil(cu_limit * cu_price / 1e6) + tip_lamports
--
-- That sum is the quantity; the columns are its parts. `cu_price` on its own is
-- NOT comparable between transactions: it is priced per compute unit, so the same
-- spend at half the limit shows up as double the price. Measured on the create
-- side, where these values are already captured, 15 distinct (cu_limit, cu_price)
-- pairs encode the single decision "spend 0.001 SOL" and cover more launches than
-- any one cu_price value does. Group by the sum, never by a part.
--
-- ATTRIBUTION — all three are per-TRANSACTION on a per-LEG table, denormalized
-- onto every leg exactly like `fee_lamports` (see 0001). A straight SUM
-- over-counts by the leg multiplier; collapse by signature first:
--     SELECT SUM(tip_lamports)
--     FROM (SELECT DISTINCT tx_signature, tip_lamports FROM trades WHERE ...) s
-- The tip makes this sharper than the compute fields do: one transaction selling
-- four wallets' bags emits four legs and pays ONE tip.
--
-- FORWARD-ONLY. `raw_txs` is opt-in with 3-day retention, so rows written before
-- this migration have no payload to re-decode and can never be filled. NULL means
-- "not captured" and must never be coalesced to 0 for display or averaged as 0.

ALTER TABLE trades
    -- `SetComputeUnitLimit` argument, in COMPUTE UNITS. NULL = the transaction set
    -- no limit and took the runtime default. Heavily modal (300k/400k/500k are
    -- hardcoded client presets) with a long simulation-derived tail, which makes it
    -- a property of the client software rather than of the moment — the half of the
    -- fee budget that behaves like identity.
    ADD COLUMN IF NOT EXISTS cu_limit     BIGINT,

    -- `SetComputeUnitPrice` argument, in MICRO-LAMPORTS PER COMPUTE UNIT. Not a
    -- lamport count, and not a number anyone chooses directly: 3,333,333 is what
    -- "0.001 SOL at a 300k limit" looks like from this side. NULL = no price set,
    -- i.e. the transaction pays no compute-rail priority fee at all.
    ADD COLUMN IF NOT EXISTS cu_price     BIGINT,

    -- LAMPORTS transferred to a known tip account (Jito block engine, Helius
    -- Sender). The other priority rail, and one that can never appear in
    -- `fee_lamports`: a tip is a transfer instruction, not a fee, so
    -- `TransactionStatusMeta.fee` excludes it by construction.
    --
    -- THREE STATES, and 0 is not NULL:
    --   NULL      the transaction carries no top-level system transfer.
    --   0         it carries one, but none reached a recognised tip account —
    --             either a router paying its own rake, or a tip rail the decoder's
    --             list does not know yet.
    --   > 0       tipped, this much.
    -- The 0 bucket is the coverage meter for that list. When it grows relative to
    -- the NULL and >0 buckets, the list is behind the market, not the market quiet:
    --     SELECT count(*) FILTER (WHERE tip_lamports IS NULL)  AS no_transfer,
    --            count(*) FILTER (WHERE tip_lamports = 0)      AS unrecognised,
    --            count(*) FILTER (WHERE tip_lamports > 0)      AS tipped
    --     FROM trades WHERE block_time > now() - interval '1 day';
    -- The list itself is `TIP_ACCOUNT_IDS` in
    -- `shared/ingest/pumpfun/src/protocol.rs`.
    --
    -- Only TOP-LEVEL transfers count. An inner (CPI) transfer is the venue moving
    -- its own protocol fee, which is not the sender buying priority.
    ADD COLUMN IF NOT EXISTS tip_lamports BIGINT;
