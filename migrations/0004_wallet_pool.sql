-- Fresh-wallet pool — Phase 1 (see docs/wallet-pool-plan.md). Replaces the bare
-- `is_active` flag with an explicit, race-safe lifecycle:
--   generated -> funded -> reserved -> used -> retired
--
-- generated = keypair minted + envelope-encrypted, no SOL yet.
-- funded    = balance poller observed an on-chain balance >= the funded floor.
-- reserved  = atomically claimed for one launch (`reserved_by_launch_id` + TTL).
-- used      = terminal; consumed by a completed launch/bundle leg, never reselected.
-- retired   = swept/burned (Phase 4 dust sweep) or manually decommissioned.
--
-- `funding_source` is a free-text audit note (who/where the manual SOL transfer
-- came from) — partial fulfillment of ADR D5's "managed_wallets must eventually
-- record funding source + hop graph"; the hop graph itself stays deferred
-- (Phase 5+, automated multi-hop funding).
--
-- `balance_lamports`/`balance_checked_at` are the wallet's own native SOL balance
-- for pool bookkeeping — NOT a trade `amount_quote`/`amount_base` (ADR D1's
-- quote-asset-relative naming doesn't apply here: every wallet's gas/balance is
-- denominated in the chain's native lamport unit regardless of which market it
-- trades, so there is no quote asset to reference).

ALTER TABLE managed_wallets
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'generated'
        CHECK (status IN ('generated','funded','reserved','used','retired')),
    ADD COLUMN IF NOT EXISTS funding_source TEXT,
    ADD COLUMN IF NOT EXISTS reserved_by_launch_id UUID REFERENCES launches(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS reserved_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS balance_lamports BIGINT,
    ADD COLUMN IF NOT EXISTS balance_checked_at TIMESTAMPTZ;

-- Backfill from the flag being replaced: wallets already in active use (the seeded
-- dev-01/bundler-01 pair) are claimable today, not fresh-generated.
UPDATE managed_wallets SET status = 'funded'  WHERE is_active;
UPDATE managed_wallets SET status = 'retired' WHERE NOT is_active;

DROP INDEX IF EXISTS idx_managed_wallets_active;
ALTER TABLE managed_wallets DROP COLUMN IF EXISTS is_active;

-- Bounds the balance poller's scan to the (small) generated set.
CREATE INDEX IF NOT EXISTS idx_managed_wallets_generated
    ON managed_wallets(created_at) WHERE status = 'generated';

-- Bounds the reservation TTL sweep to the (small) in-flight reserved set.
CREATE INDEX IF NOT EXISTS idx_managed_wallets_reserved
    ON managed_wallets(reserved_at) WHERE status = 'reserved';

-- Bounds the atomic claim query (`role = $1 AND status = 'funded'`).
CREATE INDEX IF NOT EXISTS idx_managed_wallets_role_funded
    ON managed_wallets(role) WHERE status = 'funded';
