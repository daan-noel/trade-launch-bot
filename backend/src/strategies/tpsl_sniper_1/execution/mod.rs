//! Trade execution — *how* a matched position is opened and closed.
//!
//!   - `real` — drives on-chain buys/sells (snipe buy + sell-with-retries).
//!   - `paper` — mirrors the same lifecycle against the WS/DB trade feed without
//!     sending any transaction (added in a later sub-move).
//!
//! The live drivers in `service_tpsl` pick the path by `rule.trade_mode`.

pub mod paper;
pub mod real;

// Shared buy/sell retry + poll timing. Referenced by `real` execution and by the
// paper entry poll, so they live at the module root rather than on the service.
pub(crate) const BUY_MAX_ATTEMPTS: usize = 3;
pub(crate) const BUY_POLL_MAX_ATTEMPTS: usize = 12;
pub(crate) const BUY_POLL_INTERVAL_MS: u64 = 1_000;
pub(crate) const SELL_MAX_ATTEMPTS: usize = 6;
pub(crate) const SELL_POLL_MAX_ATTEMPTS: usize = 10;
pub(crate) const SELL_POLL_INTERVAL_MS: u64 = 500;
pub(crate) const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;
/// Wall-clock window the paper exit-fill poll watches the feed for a confirming
/// exit trade before giving up and marking the position ExitFailed.
pub(crate) const PAPER_EXIT_POLL_WINDOW_SECS: u64 = 10;
/// Delay between paper exit-fill poll ticks within that window.
pub(crate) const PAPER_EXIT_POLL_INTERVAL_MS: u64 = 500;
/// Floor on how often the sell-confirm loop may re-run the net-balance SUM
/// aggregate over the partitioned `trades` table. During an active dump the
/// feed bumps `seq` (and wakes the confirm loop) once per landed leg, so
/// notify-driven wakeups can fire many times per poll interval; coalescing the
/// aggregate to at most once per this window keeps that off the hot path. Kept
/// below `SELL_POLL_INTERVAL_MS` so the periodic full-window fallback poll is
/// never suppressed, and the loop force-runs one final aggregate at the
/// deadline (when `seq` advanced) so no clear is ever missed.
pub(crate) const SELL_BALANCE_QUERY_MIN_INTERVAL_MS: u64 = 250;
