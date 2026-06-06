-- Per-token sync watermark. Lets "Fetch new" resume from the last sync even for
-- tokens that produced zero saved trades (where the trades-derived boundary is
-- NULL and would otherwise force a full re-fetch), and surfaces sync freshness
-- in the UI.
--
--   last_synced_at        — wall-clock time of the last successful sync (display).
--   last_synced_curve_sig — newest bonding-curve signature seen, used as the
--                           `until` boundary for the next incremental sync.
--   last_synced_amm_sig   — same, for post-migration PumpSwap (AMM) signatures.
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS last_synced_at TIMESTAMPTZ;
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS last_synced_curve_sig TEXT;
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS last_synced_amm_sig TEXT;
