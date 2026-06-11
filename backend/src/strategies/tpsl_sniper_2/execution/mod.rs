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
