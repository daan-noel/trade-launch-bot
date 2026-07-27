-- Slippage sentinel fix: reset both slippage settings, retire the legacy key.
--
-- Until now the settings write clamped every incoming value to
-- [SLIPPAGE_MIN_BPS=10, SLIPPAGE_MAX_BPS=5000] BEFORE persisting it. `0` — the
-- documented "no floor, accept any fill" sentinel — therefore stored as `10`,
-- the TIGHTEST possible non-zero floor. That inverted the value's meaning on the
-- bot's own buy AND sell paths: an exit meant to clear at any price during a
-- dump instead reverted on 0.1% movement.
--
-- A stored `10` may be a genuine 0.1% or a clamped `0`, and the two are NOT
-- distinguishable after the fact — so no repair is correct. Reset both keys to
-- absent, which restores the documented per-side defaults (buy = the built-in
-- DEFAULT_SLIPPAGE_BPS, sell = no floor / sell all). Whatever is typed next is
-- what runs: the clamp is gone and a typed value is now honored literally.
--
-- `trade.slippage_bps` (the legacy combined key) is deleted outright, not
-- migrated forward: it went through the same clamp, so its value is equally
-- untrustworthy, and keeping it would let a blank buy field fall through to a
-- stale legacy number instead of the default. The buy chain is now one key.
--
-- Applied to BOTH the live DB and the local lab mirror (core migrations run on
-- every boot of either bin).

DELETE FROM app_settings
WHERE key IN (
    'trade.slippage_bps',
    'trade.buy_slippage_bps',
    'trade.sell_slippage_bps'
);
