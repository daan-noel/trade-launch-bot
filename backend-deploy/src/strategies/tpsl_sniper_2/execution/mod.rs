//! Trade execution — *how* a matched position is opened and closed.
//!
//!   - `real` — drives on-chain buys/sells (snipe buy + sell-with-retries).
//!   - `paper` — mirrors the same lifecycle against the WS/DB trade feed without
//!     sending any transaction (added in a later sub-move).
//!
//! The live drivers in `service_tpsl` pick the path by `rule.trade_mode`.

pub mod paper;
pub mod real;

use std::time::Duration;

use super::util::none_if_zero_u64;
use backend_core::models::Tpsl2Rule;

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

// ── Scalp-entry watch window ─────────────────────────────────────────────────
// The scalp gates take far longer to come true than a buy fill takes to index, so
// the live *wait for the entry signal* needs a watch budget. That budget now
// derives entirely from `p_entry_max_age_secs` (the entry-window ceiling): a set
// ceiling gives a finite, self-limiting watch (`created_at + max_age`); `None`/`0`
// means watch until the token dies (`is_dead`), bounded only by the global
// concurrent-armer cap (see `Tpsl2RuntimeCache::begin_until_dead_armer`). The
// shared `find_scalp_entry` ceiling is the real guard — sim/paper/real all stop
// entering past `max_age` by construction — so the live deadline is just an
// early-exit that frees the entry slot promptly.

/// Poll cadence + bounded fallback tick for the scalp-entry watch.
pub(crate) const SCALP_ENTRY_WAIT_INTERVAL_MS: u64 = 1_000;

/// The live watch budget for a rule, derived from its entry-window ceiling:
/// `Some(max_age)` ⇒ watch at most that long after arming starts; `None` ⇒ no
/// ceiling, so the caller watches until the token dies (capped by the
/// concurrent-armer limit). Mirrors the `max_age` the shared `find_scalp_entry`
/// gate enforces, so the live deadline can never admit an entry the gate rejects.
pub(crate) fn scalp_watch_window(rule: &Tpsl2Rule) -> Option<Duration> {
    none_if_zero_u64(rule.p_entry_max_age_secs).map(Duration::from_secs)
}
