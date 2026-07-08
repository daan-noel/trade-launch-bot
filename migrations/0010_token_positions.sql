-- Post-launch token management, Phase 1: the per-wallet holdings read model
-- (token-management-plan.md). One row per (mint, wallet) we hold or have held —
-- seeded from launch/bundle fills, reconciled against on-chain token balances.
--
-- NO FK on `mint_address`: a position is seeded the moment a launch is recorded,
-- which is BEFORE the create tx is ingested into `tokens` (mirrors how
-- `LaunchListRow` LEFT JOINs `tokens`). `wallet_id` DOES FK — the wallet always
-- pre-exists.
--
-- Amounts are exact base-unit integers (platform-core's `_base`/`_quote` rule):
--   balance_base   = tokens currently held (token base units)
--   cost_quote     = quote spent acquiring (cost basis, quote base units)
--   realized_quote = quote recovered from sells (quote base units)
-- Display/USD/PnL is derived by the caller from the token's price + decimals,
-- never stored here.
--
-- Keep the `role` CHECK byte-equal to `managed_wallets.role` (migration 0002 /
-- `WalletRole::as_str`) and the `status` CHECK byte-equal to
-- `platform_core::models::status::PositionStatus::as_str()` (its roundtrip test
-- is the Rust half of this SSOT).

CREATE TABLE token_positions (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    mint_address       TEXT        NOT NULL,
    wallet_id          UUID        NOT NULL REFERENCES managed_wallets(id),
    role               TEXT        NOT NULL CHECK (role IN ('dev','bundler','treasury','trading')),
    -- Canonical ATA the buys/sells route through — persisted so a bot sell/buy
    -- reuses the same account across restarts (multi-account-token lesson).
    token_account      TEXT,
    balance_base       BIGINT      NOT NULL DEFAULT 0,
    cost_quote         BIGINT      NOT NULL DEFAULT 0,
    realized_quote     BIGINT      NOT NULL DEFAULT 0,
    balance_checked_at TIMESTAMPTZ,
    status             TEXT        NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (mint_address, wallet_id)
);

-- The one read path is "all positions for a mint" (the token detail panel).
CREATE INDEX idx_token_positions_mint ON token_positions(mint_address);
