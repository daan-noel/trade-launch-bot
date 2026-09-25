//! The frozen quiet tail — decisions past a token's own series cut.
//!
//! The sweep records each token only up to `own_last_trade + DEAD_QUIET + TAIL_MARGIN`
//! (the sparse grid's RAM bound). `run_replay` ticks every held token on to the
//! corpus-wide tail `min(as_of, corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`. In the
//! gap a simulate can still close a position that the sweep would report `Open`: after
//! the last trade the price is frozen, but the clocks keep running — `m_state.age_sec`,
//! `m_price.stall_sec`, `m_position.held_sec` / `stage_sec`, and the stage deadlines.
//!
//! So the tail is resolved without extending the grid: while no print arrives, every
//! life-span read is a function of the instant alone, and the series' own fold
//! ([`MetricSeries::track`]) answers it exactly at any later instant. A held step can
//! only change where a clock crosses a condition's threshold or a deadline, so the
//! resolve evaluates the rule's own [`CompiledRule::held_line`] at those ticks only —
//! and one tick after any move or partial sell, when the new stage is first read.
//!
//! Out of scope, left `Open` as before: a rule whose held side reads a trailing window
//! (windows decay in the tail, which this does not fold), and a series whose last row
//! is a print (the tail reads the fold at a tick). `Dead` cannot newly fire here: the
//! series cut already covers `last_meaningful + DEAD_QUIET`, and a still-open position
//! has healthy reserves.

use chrono::Duration;

use hunter_engine::arm::{CompiledRule, CondReq, MetricReq};
use hunter_engine::deadness::{DEAD_QUIET_SECS, TAIL_MARGIN_SECS};
use hunter_engine::metrics::series::MetricSeries;
use hunter_engine::metrics::state::StateMetrics;
use hunter_engine::metrics::{Metric, Ts};
use hunter_engine::rule_params::DeadlineBasis;
use hunter_engine::TICK_MS;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;
use crate::sweep::strategy::TokenOutcome;

use super::scan::{self, apply_step, BoundCombo, Fill, Held, Stepped};
use super::strategy::Pricing;

/// The tick as fractional seconds — the rate every tail clock advances at.
const TICK_SECS: f64 = TICK_MS as f64 / 1000.0;

/// The corpus-wide tail `run_replay` bounds its tick loop by for a token set scanned
/// at `as_of`: `min(as_of, corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`. `None` for
/// a trade-less set.
pub(crate) fn frozen_tail_horizon(as_of: Ts, tokens: &[CorpusToken]) -> Option<Ts> {
    // The corpus's newest trade — single-sourced with the freshness stamp the run row
    // stores (`Corpus::last_trade_at`).
    let last = crate::sweep::corpus::Corpus::last_trade_at_of(tokens)?;
    Some(as_of.min(last + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS)))
}

/// Resolve a position still held at the series' last row through the frozen tail
/// `(last_row, tail_horizon]`: the outcome of the first sell-all the rule makes there,
/// or `None` (still open, `held` carrying any partial legs and moves the tail made).
pub(crate) fn resolve(
    trades: &[CorpusTrade],
    series: &MetricSeries,
    b: &BoundCombo,
    fill: &Fill,
    held: &mut Held,
    pricing: &Pricing,
    tail_horizon: Option<Ts>,
) -> Option<TokenOutcome> {
    let horizon = tail_horizon?;
    let last = series.n_rows().checked_sub(1)?;
    if series.slot[last].is_some() || reads_a_window(&b.rule) {
        return None;
    }
    let last_at = series.at[last];
    // `run_replay` ticks while `next_tick < tail_end`: the last tick is the largest `m`
    // with `last_at + m·TICK < horizon`.
    let horizon_ms = horizon.signed_duration_since(last_at).num_milliseconds();
    if horizon_ms < TICK_MS {
        return None;
    }
    let m_max = (horizon_ms - 1) / TICK_MS;
    let at = |m: i64| last_at + Duration::milliseconds(m * TICK_MS);
    let track = series.track();
    let mut m = 0i64;
    // The last row may have moved the stage: its first read is the next tick.
    let mut force_next = true;
    loop {
        let next = next_tick(&b.rule, track, held, series, at(m), m, m_max, force_next)?;
        let now = at(next);
        let step = b.rule.held_line(track, &held.pos, held.stage, now);
        let changed = !matches!(step, hunter_engine::arm::HeldStep::None);
        // A tail leg fills at the last trade, stamped at the decision.
        let leg = || (scan::partial_leg(trades, series, last, pricing, fill.reserve).0, now);
        if let Stepped::Close(tag) = apply_step(b, held, step, now, leg) {
            return Some(scan::close(trades, series, tag, fill, held, last, now, pricing));
        }
        force_next = changed;
        m = next;
    }
}

/// Whether any held-side read (every line and every signal) is windowed.
fn reads_a_window(rule: &CompiledRule) -> bool {
    held_reqs(rule).any(|r| r.r.span.is_windowed())
}

/// Every metric condition the held side can read: its lines and every signal.
fn held_reqs(rule: &CompiledRule) -> impl Iterator<Item = &MetricReq> {
    rule.held_lines()
        .flat_map(|l| l.conds.iter())
        .filter_map(|c| match c {
            CondReq::Metric(m) => Some(m),
            CondReq::Signal { .. } => None,
        })
        .chain(rule.signals.iter().flat_map(|s| s.groups.iter().flatten()))
}

/// Whether a read keeps moving while no print arrives: a clock.
fn is_clock(req: &MetricReq) -> bool {
    matches!(req.r.metric, Metric::AgeSec | Metric::StallSec | Metric::HeldSec | Metric::StageSec)
}

/// The next tick after `m` (at most `m_max`) where the held step can change: each
/// clock condition's thresholds and the current stage's deadline, bracketed by a tick
/// either side (the estimate is exact to far below a tick; the evaluation is exact).
#[allow(clippy::too_many_arguments)]
fn next_tick(
    rule: &CompiledRule,
    track: &hunter_engine::metrics::track::TokenTrack,
    held: &Held,
    series: &MetricSeries,
    now: Ts,
    m: i64,
    m_max: i64,
    force_next: bool,
) -> Option<i64> {
    let mut best: Option<i64> = (force_next && m < m_max).then_some(m + 1);
    let mut consider = |base: f64, theta: f64| {
        if !base.is_finite() || !theta.is_finite() {
            return;
        }
        let m0 = m + ((theta - base) / TICK_SECS).floor() as i64;
        for k in (m0 - 1)..=(m0 + 2) {
            if k > m && k <= m_max && best.is_none_or(|b| k < b) {
                best = Some(k);
            }
        }
    };
    for req in held_reqs(rule).filter(|r| is_clock(r)) {
        let base = req.read(track, Some(&held.pos), now);
        let half = req.tolerance / 2.0;
        for c in req.conds.iter().flatten() {
            for theta in [c.value, c.value - half, c.value + half] {
                consider(base, theta);
            }
        }
    }
    if let Some((basis, secs)) = rule.stages.get(usize::from(held.stage)).and_then(|s| s.ends) {
        let elapsed = match basis {
            DeadlineBasis::Age => StateMetrics::time(series.created_at(), now),
            DeadlineBasis::Held => held.pos.held(now),
            DeadlineBasis::Stage => held.pos.stage_sec(now),
        };
        consider(elapsed, secs);
    }
    best
}
