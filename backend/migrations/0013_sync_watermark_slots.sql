-- Per-token sync watermark: store the SLOT of the newest signature seen, not
-- just the signature, so an incremental ("Fetch New") sync can resume by slot.
--
-- Needed by the LaserStream replay fast path: replay subscribes `from_slot`, so
-- Fetch New needs a slot boundary. The existing `last_synced_*_sig` columns stay
-- as the RPC-path `until` boundary; these add the matching slot per venue.
--
-- Nullable + no backfill: tokens synced before this migration have NULL slots
-- and simply fall back to the signature/RPC path until their next sync stamps a
-- slot. BIGINT matches Solana slot range.
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS last_synced_curve_slot BIGINT;
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS last_synced_amm_slot   BIGINT;
