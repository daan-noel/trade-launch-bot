//! `m_flow_burst` — how much of a token's recent activity landed in the last instant.
//!
//! Two strict params, both trailing windows on the **same** clock:
//! * `window_size_sec` (`W`) — the REFERENCE span; the denominator.
//! * `burst_size_sec` (`b`) — the RECENT slice; the numerator. `b <= W`, enforced
//!   at save (`rule_params::validate_group`).
//!
//! One metric:
//! * `trade_share` — trades in `[now − b, now]` as a percent of trades in
//!   `[now − W, now]`.
//!
//! **Why this is its own group.** `m_flow_window`'s basis is ONE trailing window;
//! every metric in it is a quantity over that window. This basis is two *nested*
//! windows, and a ratio across them is not a quantity over either — folding it into
//! `m_flow_window` would put a second window param on every instance that only one
//! metric reads, the same silent no-op `arm_above_pct` is guarded against.
//!
//! **It owns no state.** Both readings are `m_flow_window`'s
//! [`trade_count`](WindowState::trade_count) on the two `WindowState` ring buffers the
//! track already keeps — [`CompiledRule`] registers both axes, so a rule that also
//! gates on `m_flow_window(3)` and `m_flow_window(60)` pays nothing extra here. Reusing
//! that one implementation is also what makes `trade_share(60s/3s)` and
//! `m_flow_window(3).trade_count / m_flow_window(60).trade_count` the same number by
//! construction rather than by agreement.
//!
//! [`CompiledRule`]: crate::arm::CompiledRule

use super::flow_window::WindowState;

/// The group's second strict param — the burst (numerator) window, in seconds.
///
/// Named once here because three layers spell it: the registry declares it,
/// `arm::build_reqs` reads it into [`Windows::secondary`], and
/// `rule_params::validate_group` enforces the nesting bound.
///
/// [`Windows::secondary`]: super::Windows::secondary
pub const BURST_PARAM: &str = "burst_size_sec";

/// The slot twin of [`BURST_PARAM`]. Mutually exclusive with it, and it must agree
/// with the group's own unit - a burst measured in slots inside a reference measured
/// in seconds is a ratio across two different axes.
pub const BURST_SLOT_PARAM: &str = "burst_size_slots";

/// Percent of the reference window's trades that landed in the burst window.
///
/// `NaN` on an empty reference window — no trades, no share to report, and a `0.0`
/// would let a `trade_share <= X` condition pass on a dead tape.
///
/// **Both windows are clipped by the token's age.** On a token younger than `b` every
/// trade is inside both and this reads `100`. That is a true reading of a young token,
/// not a sentinel: the share of a two-second life that happened in the last three
/// seconds really is all of it. A rule that means the metric as a *maturity* signal
/// must bound `m_snapshot.time` itself — this metric will not do it, and the same
/// clipping applies to the SQL a rule is fitted in, so backtest and engine agree.
pub fn trade_share(
    burst: &WindowState,
    reference: &WindowState,
    burst_now: i64,
    reference_now: i64,
) -> f64 {
    let denom = reference.trade_count(reference_now);
    if denom > 0.0 {
        burst.trade_count(burst_now) / denom * 100.0
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{MetricId, Side, Ts, WindowSpec};
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    /// The same instant as [`ts`], on a window's own millisecond cursor.
    fn p(secs: f64) -> i64 {
        ts(secs).timestamp_millis()
    }

    /// Feed the same tape to both aggregators — which is what the track does: one
    /// trade stream, one buffer per distinct window.
    fn tape(widths: [f64; 2], at: &[f64]) -> (WindowState, WindowState) {
        let mut a = WindowState::new(WindowSpec::secs(widths[0]));
        let mut b = WindowState::new(WindowSpec::secs(widths[1]));
        for (i, &t) in at.iter().enumerate() {
            a.on_trade(Side::Buy, 1.0, p(t), p(t), i as u64);
            b.on_trade(Side::Buy, 1.0, p(t), p(t), i as u64);
        }
        (a, b)
    }

    /// The reading the rule is fitted on: a tape whose trades cluster into the last
    /// instant scores high, one that spreads them evenly over the reference span
    /// scores low — on the SAME trade count and the same SOL, which is what
    /// `trade_count` and `gross_flow` cannot tell apart.
    #[test]
    fn a_burst_and_an_even_drip_carry_the_same_volume_and_read_differently() {
        // 10 trades, all in the last 3s of a 60s reference.
        let (burst_a, ref_a) = tape([3.0, 60.0], &[52.0, 53.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0, 59.5, 60.0]);
        // 10 trades, one every 6s across the same 60s.
        let (burst_b, ref_b) =
            tape([3.0, 60.0], &[6.0, 12.0, 18.0, 24.0, 30.0, 36.0, 42.0, 48.0, 54.0, 60.0]);

        let now = p(60.0);
        // Identical by every single-window flow reading.
        assert_eq!(ref_a.value(MetricId::TradeCount, now), ref_b.value(MetricId::TradeCount, now));
        assert_eq!(ref_a.value(MetricId::GrossFlow, now), ref_b.value(MetricId::GrossFlow, now));

        // 5 of 10 land in `[57, 60]` — 57.0 counts, the closed lower bound.
        assert_eq!(trade_share(&burst_a, &ref_a, now, now), 50.0);
        // Only the 60.0 print is inside 3s.
        assert_eq!(trade_share(&burst_b, &ref_b, now, now), 10.0);
    }

    /// `NaN`, not `0.0`, so `trade_share <= X` cannot pass on a tape with no trades.
    #[test]
    fn an_empty_reference_window_is_nan_not_zero() {
        let (burst, reference) = tape([3.0, 60.0], &[1.0]);
        // `now` is 200s later — the single trade is out of both windows.
        assert!(trade_share(&burst, &reference, p(201.0), p(201.0)).is_nan());
    }

    /// The documented young-token reading. A rule that wants maturity must say so
    /// with `m_snapshot.time`; this metric reports the truth about a short life.
    #[test]
    fn a_token_younger_than_the_burst_window_reads_one_hundred() {
        let (burst, reference) = tape([3.0, 60.0], &[0.1, 0.4, 0.9]);
        assert_eq!(trade_share(&burst, &reference, p(1.0), p(1.0)), 100.0);
    }

    /// Both axes are read at the SAME instant, and a tick that advances `now` past
    /// the burst window drains the numerator while the reference still holds the
    /// trades — the share decays on silence instead of freezing at its last print.
    #[test]
    fn the_share_decays_on_a_tick_that_outruns_the_burst_window() {
        let (burst, reference) = tape([3.0, 60.0], &[10.0, 20.0, 30.0]);
        // At the last print: the burst holds only the 30.0 trade, the reference three.
        assert!((trade_share(&burst, &reference, p(30.0), p(30.0)) - 100.0 / 3.0).abs() < 1e-9);
        // 15s of silence later the burst is empty; the reference still holds all three.
        assert_eq!(trade_share(&burst, &reference, p(45.0), p(45.0)), 0.0);
    }
}
