//! Unified trade execution — *how* a matched position is opened and closed,
//! strategy-agnostic over [`StrategyPosition`] / [`StrategyRepo`] /
//! [`StrategyRuntimeCache`]. Replaces the per-strategy `tpsl_sniper_{1,2}/execution`
//! clones; the only strategy-specific entry orchestration (tpsl2's scalp-arming +
//! until-dead armers) is dispatched in via the service's `EntryOrchestration`.
//!
//!   - `real` — on-chain buys/sells (snipe buy + sell-with-retries + recovery).
//!   - `paper` — the same lifecycle against the WS/DB trade feed, no transactions.
//!
//! [`StrategyPosition`]: trading_core::models::StrategyPosition
//! [`StrategyRepo`]: trading_core::storage::repositories::strategy_repo::StrategyRepo
//! [`StrategyRuntimeCache`]: trading_core::strategies::runtime_cache::StrategyRuntimeCache

// Built ahead of the runner/service rewire (T4). Until the unified runner replaces
// `tpsl_sniper_{1,2}`, these are unreferenced — the `allow` is removed in T4/T6.
#![allow(dead_code)]

pub mod paper;
pub mod real;
pub mod scalp;

// Shared buy/sell retry + poll timing — referenced by `real` execution and the
// paper entry/exit polls, so they live at the module root rather than on a service.
pub(crate) const BUY_MAX_ATTEMPTS: usize = 3;
pub(crate) const BUY_POLL_MAX_ATTEMPTS: usize = 12;
pub(crate) const BUY_POLL_INTERVAL_MS: u64 = 1_000;
pub(crate) const SELL_MAX_ATTEMPTS: usize = 6;
pub(crate) const SELL_POLL_MAX_ATTEMPTS: usize = 10;
pub(crate) const SELL_POLL_INTERVAL_MS: u64 = 500;
/// Dust threshold (raw token base units) below which a remaining balance counts as
/// cleared. `trades.token_amount` is raw-unit-valued in both the model and the DB
/// (no decimal scaling — `raw_to_f64` is identity), so this is a near-zero check
/// independent of token decimals.
pub(crate) const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;
/// Wall-clock window the paper exit-fill poll watches the feed for a confirming
/// exit trade before giving up and marking the position ExitFailed.
pub(crate) const PAPER_EXIT_POLL_WINDOW_SECS: u64 = 10;
/// Delay between paper exit-fill poll ticks within that window.
pub(crate) const PAPER_EXIT_POLL_INTERVAL_MS: u64 = 500;
/// Floor on how often the sell-confirm loop may re-run the net-balance SUM
/// aggregate over the partitioned `trades` table (coalesces notify-driven wakeups
/// during a dump). Kept below `SELL_POLL_INTERVAL_MS` so the periodic fallback poll
/// is never suppressed; the loop force-runs one final aggregate at the deadline.
pub(crate) const SELL_BALANCE_QUERY_MIN_INTERVAL_MS: u64 = 250;
/// Poll cadence + bounded fallback tick for the tpsl2 scalp-entry watch (see
/// [`scalp::await_scalp_entry_signal`]). The watch is event-driven off the mint
/// trade lane; this is the fallback tick when no notify arrives.
pub(crate) const SCALP_ENTRY_WAIT_INTERVAL_MS: u64 = 1_000;
