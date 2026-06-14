//! Trade execution — *how* a matched position is opened and closed.
//!
//!   - `real` — drives on-chain buys/sells (snipe buy + sell-with-retries).
//!   - `paper` — mirrors the same lifecycle against the WS/DB trade feed without
//!     sending any transaction (added in a later sub-move).
//!
//! The live drivers in `service_tpsl` pick the path by `rule.trade_mode`.

pub mod paper;
pub mod real;

use super::util::none_if_zero_u64;
use crate::models::Tpsl2Rule;

// Shared buy/sell retry + poll timing. Referenced by `real` execution and by the
// paper entry poll, so they live at the module root rather than on the service.
pub(crate) const BUY_MAX_ATTEMPTS: usize = 3;
pub(crate) const BUY_POLL_MAX_ATTEMPTS: usize = 12;
pub(crate) const BUY_POLL_INTERVAL_MS: u64 = 1_000;
pub(crate) const SELL_MAX_ATTEMPTS: usize = 6;
pub(crate) const SELL_POLL_MAX_ATTEMPTS: usize = 10;
pub(crate) const SELL_POLL_INTERVAL_MS: u64 = 500;
pub(crate) const PARTIAL_FILL_THRESHOLD: f64 = 0.0001;
/// Floor on how often the sell-confirm loop may re-run the net-balance SUM
/// aggregate over the partitioned `trades` table. During an active dump the
/// feed bumps `seq` (and wakes the confirm loop) once per landed leg, so
/// notify-driven wakeups can fire many times per poll interval; coalescing the
/// aggregate to at most once per this window keeps that off the hot path. Kept
/// below `SELL_POLL_INTERVAL_MS` so the periodic full-window fallback poll is
/// never suppressed, and the loop force-runs one final aggregate at the
/// deadline (when `seq` advanced) so no clear is ever missed.
pub(crate) const SELL_BALANCE_QUERY_MIN_INTERVAL_MS: u64 = 250;

// ── Scalp-entry arming window ────────────────────────────────────────────────
// The scalp gates take far longer to come true than a buy fill takes to index, so
// the *wait for the entry signal* gets its own window — sized to the rule, not a
// fixed number. Both the real arming wait (`await_scalp_entry_signal`) and the
// paper entry poll watch the live feed for the first trade where
// `entry::find_scalp_entry` holds; the backtest has no such cutoff (it walks the
// complete history), so the window only bounds how closely a live run matches sim.

/// Poll cadence for the arming wait.
pub(crate) const SCALP_ENTRY_WAIT_INTERVAL_MS: u64 = 1_000;

/// Slack added on top of a rule's own time-based gates when sizing the arming
/// window — room for a qualifying trade to appear *after* the age threshold and
/// for the volume/pullback gates to settle. The only knob to tune by hand; the
/// rest of the window tracks the configured gates automatically.
pub(crate) const SCALP_ARMING_BASE_SECS: u64 = 60;

/// How many poll ticks the live path watches for a rule's scalp entry signal before
/// giving up, **derived from the rule** so a larger `p_entry_min_age_secs` /
/// `p_entry_higher_low_secs` widens the window automatically — no constant to keep
/// in sync by hand. A rule with no time gates still gets `SCALP_ARMING_BASE_SECS`.
pub(crate) fn scalp_arming_attempts(rule: &Tpsl2Rule) -> usize {
    let min_age = none_if_zero_u64(rule.p_entry_min_age_secs).unwrap_or(0);
    let higher_low = none_if_zero_u64(rule.p_entry_higher_low_secs).unwrap_or(0);
    let window_ms = SCALP_ARMING_BASE_SECS
        .saturating_add(min_age)
        .saturating_add(higher_low)
        .saturating_mul(1_000);
    // One tick per interval; never fewer than one so the feed is checked at least once.
    (window_ms / SCALP_ENTRY_WAIT_INTERVAL_MS).max(1) as usize
}
