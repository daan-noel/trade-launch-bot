//! The two slice metrics of `m_flow`: a span with a shorter slice nested in it
//! (`[30s, slice 2s]`), and the share of the span's activity the slice holds.
//!
//! * `slice_trade_share_pct` — prints in the slice as a percent of prints in the span.
//! * `slice_sol_share_pct` — the same ratio on gross SOL. Ten prints of 0.1 SOL and one
//!   print of 10 are the same trade share and far apart here, and on a PRINT span the
//!   trade share is `slice / span` on every coin while this one still varies.
//!
//! **This module owns no state.** Both readings come off the two token-level flow
//! windows the track already keeps — [`trade_count`](WindowState::trade_count) and
//! gross SOL — so `[30s, slice 2s]` and a separate `[2s]` read share buffers.

use super::flow_window::WindowState;

/// Percent of the reference window's trades that landed in the slice window.
///
/// `NaN` on an empty reference window — no trades, no share to report, and a `0.0`
/// would let a `trade_share <= X` condition pass on a dead tape.
///
/// **Both windows are clipped by the token's age.** On a token younger than the slice every
/// trade is inside both and this reads `100`. That is a true reading of a young token,
/// not a sentinel: the share of a two-second life that happened in the last three
/// seconds really is all of it. A rule that means the metric as a *maturity* signal
/// must bound `m_state.time` itself — this metric will not do it, and the same
/// clipping applies to the SQL a rule is fitted in, so backtest and engine agree.
pub fn trade_share(
    slice: &WindowState,
    reference: &WindowState,
    slice_now: i64,
    reference_now: i64,
) -> f64 {
    let denom = reference.trade_count(reference_now);
    if denom > 0.0 {
        slice.trade_count(slice_now) / denom * 100.0
    } else {
        f64::NAN
    }
}

/// Percent of the reference window's gross SOL that moved in the slice window.
///
/// Same shape and same contracts as [`trade_share`] — `NaN` on an empty reference
/// window, `100` on a token younger than the slice — measured on SOL instead of on
/// a count. Kept beside it, and reading the same [`WindowState`] pair, so the two can
/// never disagree about what the two spans are.
///
/// This is the reading a PRINT basis leaves standing. A print slice is a fixed count
/// of transactions inside a fixed count of transactions, so `trade_share` there is
/// `b / W` on every token and carries no information; the SOL that moved in those
/// prints is what still separates one tape from another.
pub fn sol_share(
    slice: &WindowState,
    reference: &WindowState,
    slice_now: i64,
    reference_now: i64,
) -> f64 {
    let denom = reference.value(super::Metric::GrossSol, reference_now);
    if denom > 0.0 {
        slice.value(super::Metric::GrossSol, slice_now) / denom * 100.0
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{Metric, Side, Ts, WindowSpec};
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
        for &t in at {
            a.on_trade(Side::Buy, 1.0, p(t), p(t));
            b.on_trade(Side::Buy, 1.0, p(t), p(t));
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
        let (slice_a, ref_a) = tape([3.0, 60.0], &[52.0, 53.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0, 59.5, 60.0]);
        // 10 trades, one every 6s across the same 60s.
        let (slice_b, ref_b) =
            tape([3.0, 60.0], &[6.0, 12.0, 18.0, 24.0, 30.0, 36.0, 42.0, 48.0, 54.0, 60.0]);

        let now = p(60.0);
        // Identical by every single-window flow reading.
        assert_eq!(ref_a.value(Metric::TradeCount, now), ref_b.value(Metric::TradeCount, now));
        assert_eq!(ref_a.value(Metric::GrossSol, now), ref_b.value(Metric::GrossSol, now));

        // 5 of 10 land in `[57, 60]` — 57.0 counts, the closed lower bound.
        assert_eq!(trade_share(&slice_a, &ref_a, now, now), 50.0);
        // Only the 60.0 print is inside 3s.
        assert_eq!(trade_share(&slice_b, &ref_b, now, now), 10.0);
    }

    /// The reading `trade_share` cannot give: same trade counts, different SOL.
    ///
    /// Both tapes put one of three prints in the slice, so `trade_share` is 33.3 on
    /// each. The SOL in that print is 1 of 12 on one and 10 of 12 on the other, and
    /// only `sol_share` separates them — which is why the pair is two metrics and
    /// not one.
    #[test]
    fn the_same_trade_share_can_be_a_tenth_or_most_of_the_money() {
        let build = |sols: [f64; 3]| {
            let mut slice = WindowState::new(WindowSpec::secs(3.0));
            let mut reference = WindowState::new(WindowSpec::secs(60.0));
            for (&sol, at) in sols.iter().zip([10.0, 20.0, 30.0]) {
                slice.on_trade(Side::Buy, sol, p(at), p(30.0));
                reference.on_trade(Side::Buy, sol, p(at), p(30.0));
            }
            (slice, reference)
        };
        let now = p(30.0);

        // The last print carries 1 of 12 SOL.
        let (s_small, r_small) = build([1.0, 10.0, 1.0]);
        // The last print carries 10 of 12.
        let (s_big, r_big) = build([1.0, 1.0, 10.0]);

        let share = |a: &WindowState, b: &WindowState| trade_share(a, b, now, now);
        assert!((share(&s_small, &r_small) - 100.0 / 3.0).abs() < 1e-9);
        assert!((share(&s_big, &r_big) - 100.0 / 3.0).abs() < 1e-9, "identical by count");

        assert!((sol_share(&s_small, &r_small, now, now) - 100.0 / 12.0).abs() < 1e-9);
        assert!((sol_share(&s_big, &r_big, now, now) - 1000.0 / 12.0).abs() < 1e-9);
    }

    /// `sol_share` counts SOL, not direction: a sell moves money too, so it is
    /// `gross_flow` on both ends and never `net_flow`. A signed numerator would make
    /// the share negative, which is not a share of anything.
    #[test]
    fn a_sell_moves_money_so_it_counts_toward_the_share() {
        let mut slice = WindowState::new(WindowSpec::secs(3.0));
        let mut reference = WindowState::new(WindowSpec::secs(60.0));
        for (side, sol, at) in [(Side::Buy, 5.0, 10.0), (Side::Sell, 5.0, 30.0)] {
            slice.on_trade(side, sol, p(at), p(30.0));
            reference.on_trade(side, sol, p(at), p(30.0));
        }
        let now = p(30.0);
        // 5 of 10 SOL gross moved in the slice, even though net flow over the
        // reference is exactly zero.
        assert_eq!(sol_share(&slice, &reference, now, now), 50.0);
    }

    /// Same empty-window contract as [`trade_share`]: `NaN`, never `0.0`, so
    /// `sol_share <= X` cannot pass on a tape where nothing moved.
    #[test]
    fn an_empty_reference_window_has_no_sol_share() {
        let (slice, reference) = tape([3.0, 60.0], &[1.0]);
        assert!(sol_share(&slice, &reference, p(201.0), p(201.0)).is_nan());
    }

    /// `NaN`, not `0.0`, so `trade_share <= X` cannot pass on a tape with no trades.
    #[test]
    fn an_empty_reference_window_is_nan_not_zero() {
        let (slice, reference) = tape([3.0, 60.0], &[1.0]);
        // `now` is 200s later — the single trade is out of both windows.
        assert!(trade_share(&slice, &reference, p(201.0), p(201.0)).is_nan());
    }

    /// The documented young-token reading. A rule that wants maturity must say so
    /// with `m_state.time`; this metric reports the truth about a short life.
    #[test]
    fn a_token_younger_than_the_slice_window_reads_one_hundred() {
        let (slice, reference) = tape([3.0, 60.0], &[0.1, 0.4, 0.9]);
        assert_eq!(trade_share(&slice, &reference, p(1.0), p(1.0)), 100.0);
    }

    /// Both axes are read at the SAME instant, and a tick that advances `now` past
    /// the slice window drains the numerator while the reference still holds the
    /// trades — the share decays on silence instead of freezing at its last print.
    #[test]
    fn the_share_decays_on_a_tick_that_outruns_the_slice_window() {
        let (slice, reference) = tape([3.0, 60.0], &[10.0, 20.0, 30.0]);
        // At the last print: the slice holds only the 30.0 trade, the reference three.
        assert!((trade_share(&slice, &reference, p(30.0), p(30.0)) - 100.0 / 3.0).abs() < 1e-9);
        // 15s of silence later the slice is empty; the reference still holds all three.
        assert_eq!(trade_share(&slice, &reference, p(45.0), p(45.0)), 0.0);
    }
}
