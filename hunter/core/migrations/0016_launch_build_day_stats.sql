-- 0016: the launch-build door.
--
-- `tokens_info` learns the curve PEAK: the highest priced reserve (vsol) any curve
-- print left and when it happened - not a first crossing, and not `ath_price`
-- (which spans AMM prints too). The live cache latches it per curve print; the
-- workstation backfills history from `trades` once (never on the server).
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS curve_peak_reserve_sol DOUBLE PRECISION;
ALTER TABLE tokens_info ADD COLUMN IF NOT EXISTS curve_peak_at          TIMESTAMPTZ;

-- One row per (UTC day, creation build): how many tokens the build - the exact
-- ordered creation `ix_labels` - launched on the PREVIOUS day, and how many of those
-- became runners (curve peak >= RUNNER_PEAK_RESERVE_SOL, reached >=
-- RUNNER_MIN_PEAK_AGE_SECS after birth, and before the day began). Computed once
-- a day off `tokens` x `tokens_info`, never off `trades`; a day is written once and
-- never rewritten, so a restart never changes the door a token was born under.
CREATE TABLE IF NOT EXISTS launch_build_day_stats (
    day        DATE    NOT NULL,
    ix_labels  JSONB   NOT NULL,
    launches   INTEGER NOT NULL,
    runners    INTEGER NOT NULL,
    PRIMARY KEY (day, ix_labels)
);
