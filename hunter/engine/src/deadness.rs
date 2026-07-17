//! Dead-token verdict — the SSOT for "is this token dead?", moved here from
//! `trading_core` (which re-exports every item so `token_cache` / the analysis
//! death-close keep their old paths). Pure: a function of two reserve/time
//! readings and the metric constants, no clock or DB.
//!
//! A token is dead at `now` when BOTH signals hold:
//!   1. **Liquidity gone** — the newest real SOL reserves are below
//!      [`DEAD_MAX_LIQUIDITY_SOL`]. `None` (no reserve snapshot yet) ⇒ alive.
//!   2. **Silent** — no *meaningful* trade (≥ [`DEAD_MEANINGFUL_TRADE_SOL`]) for
//!      at least [`DEAD_QUIET_SECS`]; the caller passes the newest meaningful
//!      trade time (falling back to token creation when none has arrived, so a
//!      never-traded token's quiet clock runs from birth).
//!
//! The quiet requirement means a token that briefly dips in reserves but keeps
//! trading is never flagged; the verdict flips true exactly once and stays there.
//! Live ([`token_cache`]) and the analysis death-close feed the same inputs
//! through this one function so they can never disagree on *which* tokens are
//! dead.

use crate::metrics::Ts;

/// Signal 1 — liquidity is gone once the latest real SOL reserves drop below this.
pub const DEAD_MAX_LIQUIDITY_SOL: f64 = 30.0;
/// Signal 2 — no meaningful trade for this many seconds ⇒ silent.
pub const DEAD_QUIET_SECS: i64 = 300; // 5 minutes
/// SOL threshold below which a trade is dust and does NOT reset the quiet timer.
pub const DEAD_MEANINGFUL_TRADE_SOL: f64 = 0.1;

/// The pure dead-token decision. `reserves` is the newest real SOL reserves
/// (`None` = no snapshot yet ⇒ alive); `last_meaningful` is the newest
/// meaningful-trade time (callers fall back to `created_at` when none arrived).
pub fn is_dead_verdict(reserves: Option<f64>, last_meaningful: Ts, now: Ts) -> bool {
    // Signal 1: liquidity depleted. None → no reserve snapshot yet → alive.
    let reserves_depleted = matches!(reserves, Some(sol) if sol < DEAD_MAX_LIQUIDITY_SOL);
    if !reserves_depleted {
        return false;
    }
    // Signal 2: silent for DEAD_QUIET_SECS (dust ignored by the caller's clock).
    now.signed_duration_since(last_meaningful).num_seconds() >= DEAD_QUIET_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn at(secs: i64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::seconds(secs)
    }

    #[test]
    fn none_reserves_is_alive() {
        assert!(!is_dead_verdict(None, at(0), at(100_000)));
    }

    #[test]
    fn healthy_reserves_never_dead() {
        assert!(!is_dead_verdict(Some(DEAD_MAX_LIQUIDITY_SOL), at(0), at(100_000)));
        assert!(!is_dead_verdict(Some(50.0), at(0), at(100_000)));
    }

    #[test]
    fn depleted_and_quiet_is_dead() {
        // Reserves gone + silent past the quiet window.
        assert!(is_dead_verdict(Some(5.0), at(10), at(10 + DEAD_QUIET_SECS)));
        // One second short of the window → still alive.
        assert!(!is_dead_verdict(Some(5.0), at(10), at(10 + DEAD_QUIET_SECS - 1)));
    }
}
