-- Wallet funding orchestration (docs/wallet-funding-plan.md). Adds the `funding`
-- lifecycle state — a treasury→wallet SOL send in flight — between `generated`
-- and `funded`:
--   generated -> funding -> funded -> reserved -> used -> retired
--
-- A wallet is atomically claimed out of `generated` into `funding` before the
-- send (FOR UPDATE SKIP LOCKED) so a concurrent funder / post-restart run can't
-- double-fund it (real SOL loss). The balance poller promotes `funding` ->
-- `funded` when the SOL lands (same balance-driven detection as `generated` ->
-- `funded`); a send failure reverts `funding` -> `generated` (retryable).
--
-- Keep the CHECK IN-list byte-equal to `platform_core::models::status`
-- `WalletStatus::as_str()` (guarded by its roundtrip test) — this is the SQL
-- half of that SSOT.

-- The 0004 CHECK was created inline with ADD COLUMN, so PG auto-named it
-- `managed_wallets_status_check`. Drop + re-add with `funding` included.
ALTER TABLE managed_wallets DROP CONSTRAINT IF EXISTS managed_wallets_status_check;
ALTER TABLE managed_wallets
    ADD CONSTRAINT managed_wallets_status_check
    CHECK (status IN ('generated','funding','funded','reserved','used','retired'));

-- The balance poller must observe `funding` wallets (not just `generated`) to
-- promote them once SOL lands. Widen the poller's partial index to cover both
-- pollable states. Replaces the `generated`-only `idx_managed_wallets_generated`.
DROP INDEX IF EXISTS idx_managed_wallets_generated;
CREATE INDEX IF NOT EXISTS idx_managed_wallets_pollable
    ON managed_wallets(created_at) WHERE status IN ('generated','funding');
