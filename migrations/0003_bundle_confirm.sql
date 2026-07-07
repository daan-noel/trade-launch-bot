-- Bundle-landing confirmation. Executor already produces a Jito bundle id + the
-- per-leg tx signatures at submit time; this persists them so a separate watcher
-- can confirm landing against the already-ingested `trades` feed (never a fresh
-- RPC poll — same principle as meme-trading's feed-based sell-confirm).
--
-- Status lifecycle: planned -> submitting -> submitted -> landed | dropped | partial | failed.
-- (landed = every leg signature found in `trades`; dropped = none within the
-- confirm timeout; partial = some but not all — an atomicity anomaly worth
-- surfacing, since a true Jito bundle is all-or-nothing.)
ALTER TABLE bundles
    ADD COLUMN IF NOT EXISTS jito_bundle_id  TEXT,
    ADD COLUMN IF NOT EXISTS leg_signatures  TEXT[]      NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS submitted_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS confirmed_at    TIMESTAMPTZ;

-- Bounds the confirm watcher's poll to the tiny in-flight set.
CREATE INDEX IF NOT EXISTS idx_bundles_status_submitted
    ON bundles(status, submitted_at)
    WHERE status = 'submitted';
