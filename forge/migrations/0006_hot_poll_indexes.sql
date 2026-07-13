-- Efficiency audit 2026-07-13: index the two hot poll-endpoint sorts.
--
-- Both `/api/launches` (LaunchRepo::list_page) and `/api/wallet_pool`
-- (ManagedWalletRepo::list_all) end in `ORDER BY created_at DESC` and are polled
-- every few seconds by the operator dashboard, but neither `launches` nor
-- `managed_wallets` had a `created_at` index — every poll paid a sort. `tokens`
-- already carries the `(created_at DESC)` pattern; these two were missed.
--
-- `launches`: newest-first list scan. `id` as the tiebreaker keeps the sort
-- fully index-ordered (created_at is second-resolution).
CREATE INDEX IF NOT EXISTS idx_launches_created ON launches (created_at DESC, id);

-- `managed_wallets`: the pool admin view lists newest-first, optionally scoped to
-- one `role`. A composite `(role, created_at DESC)` serves both the role-filtered
-- and (via the leading column) the unfiltered scan.
CREATE INDEX IF NOT EXISTS idx_managed_wallets_role_created
    ON managed_wallets (role, created_at DESC);
