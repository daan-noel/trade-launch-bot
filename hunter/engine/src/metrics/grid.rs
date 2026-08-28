//! Sparse tick grid — the ONE driver that folds a token's trades into a
//! [`MetricSeries`] the way the engine actually decides: trades **interleaved with
//! synthetic `Tick`s** on the `created + k·TICK_MS` grid.
//!
//! Every consumer of a `MetricSeries` must drive it through here. A trade-only
//! fold is not a coarser view of the same series — it is a *different* series, and
//! silently so: the time-decaying metrics (`m_flow_window` / `m_flow_ix_window`
//! decay, `m_price_window` rolling extrema, `m_state.stall`/`.time`, and the
//! dead verdict) only advance inside [`TokenTrack::on_tick`], so with no ticks they
//! are sampled solely at trade instants — exactly where a fresh trade has just been
//! folded back in. A window that dips below a threshold between two trades is then
//! invisible, and a scan over that series reports a *later* fire (or none) than the
//! engine takes. That was a real, shipped bug: the metric-series chart endpoint
//! folded trades only and drew an exit marker 70 s after the one simulate booked.
//!
//! [`TokenTrack::on_tick`]: super::track::TokenTrack::on_tick
//!
//! Ticks are emitted **sparsely** ([`SparseGrid`]): every trade, plus grid ticks
//! only up to each gap's [`gap_horizon`](SparseGrid::gap_horizon) — the last instant
//! at which any metric the caller reads could still change. Past it every tick is
//! provably identical to its predecessor and is skipped, so a long-lived token
//! records rows ∝ its trades rather than ∝ its wall-clock lifespan. Every emitted
//! tick still lands on the same `created + k·TICK` grid a dense fold would use, so
//! the recorded values are bit-identical to the dense series'.

use chrono::Duration;

use super::series::MetricSeries;
use super::{Ts, TradeLite};
use crate::deadness::{DEAD_QUIET_SECS, TAIL_MARGIN_SECS};
use crate::TICK_MS;

/// ~2 days of ticks — beyond the sparse-grid horizon clamp. Hard-caps one gap so a
/// corrupt timestamp / horizon cannot push billions of rows and abort with a
/// multi-hundred-TB allocation.
const MAX_TICKS_PER_GAP: usize = 2 * 24 * 3600 * 2;

/// How far a caller's reads can still change inside a quiet gap.
///
/// Between two trades nothing that depends on a *trade* moves: price, reserves,
/// lifetime flows and the lifetime trail are constant; only window flows (until a
/// trade ages out of the largest window), the monotone `time`/`stall` clocks, and
/// the one-shot dead verdict can change. So within a gap we only need dense ticks
/// up to the last instant any of those could still flip a condition the caller
/// evaluates; past it every tick is provably identical to its predecessor and is
/// omitted.
///
/// A caller that does not know which conditions will be evaluated (e.g. a chart
/// endpoint serving arbitrary rules) must pass horizons wide enough to cover them —
/// see [`SparseGrid::for_windows`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SparseGrid {
    /// Largest registered trailing window (secs) across BOTH window families —
    /// `m_flow_window`/`m_flow_ix_window` and `m_price_window`; `0` if the caller
    /// reads neither. A trade keeps changing windowed metrics until `trade + this`
    /// (flows decay to 0; a rolling price high/low drops as the print ages out),
    /// after which every windowed read is settled for that gap.
    pub max_window_secs: f64,
    /// Max `time` condition value + its `=`-tolerance (secs since creation); `0` if
    /// `time` isn't read. `time` is monotone, so past this every `time` condition is
    /// settled forever.
    pub time_horizon_secs: f64,
    /// Max `stall` condition value + its `=`-tolerance (secs). `stall` grows
    /// monotonically within a gap, so past `last_trade + this` every `stall`
    /// condition is settled for that gap.
    pub stall_horizon_secs: f64,
}

impl SparseGrid {
    /// Hard ceiling on sparse-grid horizons (secs). A fat-fingered `time`/`stall`/
    /// window axis (e.g. 1e12) must not turn [`gap_horizon`](Self::gap_horizon) into
    /// `DateTime::MAX` and emit centuries of ticks (the alloc that printed ~419 TB).
    pub const MAX_HORIZON_SECS: f64 = 7.0 * 24.0 * 3600.0;

    /// A grid for a caller that knows only its trailing windows — the `time`/`stall`
    /// horizons are left at `0`, which is correct **only** when no `time`/`stall`
    /// condition is evaluated over the series. Pass those horizons explicitly
    /// otherwise; a horizon that is too small silently drops the ticks a crossing
    /// would have landed on.
    pub fn for_windows(windows: &[f64]) -> Self {
        Self {
            max_window_secs: windows.iter().cloned().fold(0.0_f64, f64::max),
            time_horizon_secs: 0.0,
            stall_horizon_secs: 0.0,
        }
    }

    /// Widen every horizon to at least `other`'s — the superset relation a shared
    /// precompute needs (extra ticks are decision-neutral, missing ones are not).
    pub fn widen(self, other: Self) -> Self {
        Self {
            max_window_secs: self.max_window_secs.max(other.max_window_secs),
            time_horizon_secs: self.time_horizon_secs.max(other.time_horizon_secs),
            stall_horizon_secs: self.stall_horizon_secs.max(other.stall_horizon_secs),
        }
    }

    /// Clamp a caller-supplied horizon into `[0, MAX_HORIZON_SECS]`.
    pub fn clamp_secs(s: f64) -> f64 {
        if !s.is_finite() || s <= 0.0 {
            0.0
        } else {
            s.min(Self::MAX_HORIZON_SECS)
        }
    }

    /// The last grid instant in a gap at which any read could still change, given the
    /// trade state entering the gap: `last_trade_at` (the newest folded trade, ≥ every
    /// metric clock's origin) and `last_meaningful_at` (the deadness clock). Dense
    /// ticks are emitted up to here; ticks past it are all provably static and skipped.
    pub fn gap_horizon(&self, created: Ts, last_trade_at: Ts, last_meaningful_at: Ts) -> Ts {
        let secs = |s: f64| {
            let ms = (Self::clamp_secs(s) * 1000.0).ceil();
            // chrono::Duration::milliseconds panics / overflows on absurd inputs —
            // clamp to i64::MAX/2 ms (~3e8 years still, but finite adds).
            let ms_i = if ms.is_finite() {
                ms.min((i64::MAX / 2) as f64) as i64
            } else {
                0
            };
            Duration::milliseconds(ms_i.max(0))
        };
        let mut h = last_trade_at;
        // Flow decay: a trade influences the largest window until `trade + window`.
        h = h.max(last_trade_at + secs(self.max_window_secs));
        // `time` is measured from creation.
        h = h.max(created + secs(self.time_horizon_secs));
        // `stall` is measured from the last all-time high (≤ last_trade_at); using
        // last_trade_at is a safe upper bound (it only over-emits, never drops).
        h = h.max(last_trade_at + secs(self.stall_horizon_secs));
        // Dead flips once, at the first tick past the quiet window.
        h = h.max(last_meaningful_at + Duration::seconds(DEAD_QUIET_SECS));
        h
    }
}

/// Outcome of a [`fold_sparse`] pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SparseFold {
    /// Rows recorded (trades + emitted ticks).
    pub rows: usize,
    /// Set when a `max_rows` budget stopped the fold before the tail — the series
    /// covers only `[created, covered_until]`. Callers that bound the fold MUST
    /// surface this; a silently short series reads exactly like a token that stopped
    /// trading.
    pub truncated: bool,
    /// The last instant the series covers (the newest recorded row, or `created`
    /// when nothing was recorded).
    pub covered_until: Ts,
    /// The first instant the series covers (the oldest recorded row, or `created`
    /// when nothing was recorded). Equals the first event unless the caller set
    /// [`MetricSeries::set_record_from`] — coverage is a span, and a caller that
    /// moves its start must be able to say where it now begins.
    pub covered_from: Ts,
}

/// Fold `trades` into `series` on the sparse tick grid, then tick the tail up to
/// `min(as_of, last_trade + DEAD_QUIET + TAIL_MARGIN)` so a quiet token books its
/// dead verdict.
///
/// `trades` yields `(trade, slot)` in canonical (block-time-monotonic) order; `slot`
/// is `Some` only for callers that resolve a fill row back to its source trade.
/// `max_rows` bounds the recorded rows — `None` for an unbounded fold (the sweep,
/// which admits by an up-front [`estimate_sparse_rows`] instead); `Some(n)` stops
/// early and reports [`SparseFold::truncated`].
///
/// Trades are folded in the caller's order, which for a canonical corpus load
/// (slot → tx_index → leg) is block-time-monotonic — the order a full `run_replay`
/// also folds them in, so the two never diverge.
///
/// Where the recorded rows *begin* is the series' own knob
/// ([`MetricSeries::set_record_from`]), not an argument here: this fold always runs
/// from `created`, because lifetime metrics are defined from there. A withheld row
/// costs no budget, so `max_rows` then buys its span around the caller's window
/// instead of around the token's first trade.
pub fn fold_sparse<I>(
    series: &mut MetricSeries,
    created: Ts,
    trades: I,
    grid: &SparseGrid,
    as_of: Ts,
    max_rows: Option<usize>,
) -> SparseFold
where
    I: IntoIterator<Item = (TradeLite, Option<u64>)>,
{
    let tick = Duration::milliseconds(TICK_MS);
    let budget = max_rows.unwrap_or(usize::MAX);
    let mut next_tick = created + tick;
    let mut last_trade_at = created;
    let mut truncated = false;

    let mut any = false;
    for (t, slot) in trades {
        any = true;
        let at = t.at;
        // Gap before this trade: dense ticks only up to the gap horizon, then jump
        // straight to the trade (skipping the provably-static remainder).
        let horizon = grid.gap_horizon(created, last_trade_at, series.last_meaningful_at());
        if emit_gap_ticks(series, &mut next_tick, at, horizon, tick, created, budget) {
            truncated = true;
            break;
        }
        if series.n_rows() >= budget {
            truncated = true;
            break;
        }
        series.push_trade_at(t, slot);
        if at > last_trade_at {
            last_trade_at = at;
        }
    }

    if any && !truncated {
        // Tail: keep ticking so a quiet token books its dead verdict, but no further
        // than the window past which every token is provably dead + pruned.
        let cap = last_trade_at + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS);
        let tail_end = as_of.min(cap);
        let horizon = grid.gap_horizon(created, last_trade_at, series.last_meaningful_at());
        if emit_gap_ticks(series, &mut next_tick, tail_end, horizon, tick, created, budget) {
            truncated = true;
        }
    }

    SparseFold {
        rows: series.n_rows(),
        truncated,
        covered_until: series.at.last().copied().unwrap_or(created),
        covered_from: series.at.first().copied().unwrap_or(created),
    }
}

/// Emit grid ticks in `[next_tick, stop)` but no further than `horizon`; then
/// fast-forward `next_tick` onto the grid to the first tick ≥ `stop` **without**
/// emitting the settled ticks in between (that arithmetic jump is what makes the
/// grid sparse over long quiet gaps). Post-state matches a dense loop's
/// (`next_tick` = smallest grid tick ≥ `stop`) so the caller's next gap is unaffected.
///
/// Returns `true` when the `budget` row cap stopped it (the caller's series is now
/// truncated). Also hard-capped at [`MAX_TICKS_PER_GAP`] per gap.
fn emit_gap_ticks(
    series: &mut MetricSeries,
    next_tick: &mut Ts,
    stop: Ts,
    horizon: Ts,
    tick: Duration,
    created: Ts,
    budget: usize,
) -> bool {
    let mut emitted = 0usize;
    while *next_tick < stop && *next_tick <= horizon {
        if series.n_rows() >= budget {
            return true;
        }
        if emitted >= MAX_TICKS_PER_GAP {
            // Jump to stop without further emits — decisions past the clamp are
            // identical for settled monotone clocks under the horizon cap.
            break;
        }
        series.push_tick(*next_tick);
        *next_tick += tick;
        emitted += 1;
    }
    if *next_tick < stop {
        // Stopped at the horizon (or tick cap), not at `stop` — jump to the next
        // on-grid tick ≥ stop.
        let delta_ms = stop.signed_duration_since(created).num_milliseconds();
        let k = (delta_ms + TICK_MS - 1) / TICK_MS; // ceil ⇒ smallest grid tick ≥ stop
        *next_tick = created + Duration::milliseconds(k * TICK_MS);
    }
    false
}

/// Worst-case row count of a token's sparse series, without building it — the
/// admission estimate. Upper-bounds each gap's dense span by the grid horizon so it
/// never under-counts (which would defeat the guard it feeds).
pub fn estimate_sparse_rows<I>(created: Ts, trade_times: I, grid: &SparseGrid, as_of: Ts) -> usize
where
    I: IntoIterator<Item = Ts>,
{
    let tick_ms = TICK_MS.max(1);
    let horizon_ms = ((SparseGrid::clamp_secs(grid.max_window_secs)
        .max(SparseGrid::clamp_secs(grid.time_horizon_secs))
        .max(SparseGrid::clamp_secs(grid.stall_horizon_secs))
        + DEAD_QUIET_SECS as f64)
        * 1000.0)
        .ceil() as i64;
    let mut rows = 0usize;
    let mut prev = created;
    let mut any = false;
    // One row per trade, plus at most `horizon_ms / tick` dense ticks per gap.
    for at in trade_times {
        any = true;
        let gap_ms = at.signed_duration_since(prev).num_milliseconds().max(0);
        let ticks = (gap_ms.min(horizon_ms).max(0) / tick_ms) as usize;
        rows = rows.saturating_add(1).saturating_add(ticks);
        if at > prev {
            prev = at;
        }
    }
    if !any {
        return 0;
    }
    // Tail gap (bounded by DEAD_QUIET + TAIL_MARGIN as well as the caller's `as_of`).
    let tail_cap = prev + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS);
    let tail_ms = as_of.min(tail_cap).signed_duration_since(prev).num_milliseconds().max(0);
    rows.saturating_add((tail_ms.min(horizon_ms).max(0) / tick_ms) as usize)
}

#[cfg(test)]
mod tests {
    use crate::metrics::WindowSpec;
    use super::*;
    use crate::metrics::series::SeriesColumn;
    use crate::metrics::{MetricId, Side};
    use chrono::{TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn buy(sol: f64, secs: f64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol,
            price: 1.0,
            reserve_sol: 40.0,
            at: ts(secs),
            ..Default::default()
        }
    }

    /// The regression this module exists for: a trailing window that dips below a
    /// threshold **between** two trades is invisible to a trade-only fold and
    /// visible to the sparse grid — at the same instant a dense fold would see it.
    #[test]
    fn ticks_expose_a_between_trades_window_dip_that_a_trade_only_fold_hides() {
        let created = ts(0.0);
        let col = SeriesColumn::window(MetricId::Buy, WindowSpec::secs(60.0));
        // 6 SOL of buys land at t=0, then nothing until t=61 — where a fresh 6 SOL
        // buy lands. `buy@60` is 6 at t=0, decays to 0 just after t=60, and is back
        // to 6 at t=61. A `buy < 5` exit must fire in the gap.
        let trades = [(buy(6.0, 0.0), None), (buy(6.0, 61.0), None)];

        // Trade-only fold (the bug): only two rows, both reading 6 ⇒ never < 5.
        let mut naive = MetricSeries::new(created, vec![col]);
        for (t, _) in trades {
            naive.push_trade(t);
        }
        let naive_vals = naive.column_values(col).unwrap();
        assert_eq!(naive_vals.len(), 2);
        assert!(
            naive_vals.iter().all(|v| *v >= 5.0),
            "trade-only fold must be the blind case this test documents",
        );

        // Sparse grid: the decay is sampled, and the first sub-5 row lands on the
        // first grid tick past the window edge (t = 60.0 + TICK).
        let mut series = MetricSeries::new(created, vec![col]);
        let grid = SparseGrid::for_windows(&[60.0]);
        fold_sparse(&mut series, created, trades, &grid, ts(61.0), None);
        let vals = series.column_values(col).unwrap();
        let first_below = (0..series.n_rows())
            .find(|&i| vals[i] < 5.0)
            .expect("the sparse grid must sample the window decay");
        let at = series.at[first_below];
        assert!(
            at > ts(60.0) && at <= ts(60.0) + Duration::milliseconds(TICK_MS),
            "crossing must land on the first grid tick past the window edge, got {at}",
        );
        assert_eq!(vals[first_below], 0.0, "the whole 6 SOL ages out at once");
    }

    /// Emitted ticks sit on the `created + k·TICK` grid, so a sparse series' rows are
    /// bit-identical to the dense series' rows at the same instants.
    #[test]
    fn emitted_ticks_land_on_the_shared_grid() {
        let created = ts(0.0);
        let col = SeriesColumn::window(MetricId::Buy, WindowSpec::secs(5.0));
        let mut series = MetricSeries::new(created, vec![col]);
        let grid = SparseGrid::for_windows(&[5.0]);
        fold_sparse(&mut series, created, [(buy(1.0, 0.4), None)], &grid, ts(2.0), None);
        for at in &series.at {
            let off = at.signed_duration_since(created).num_milliseconds();
            // Trade rows sit at their own timestamp; tick rows are on the grid.
            assert!(
                off % TICK_MS == 0 || off == 400,
                "row at +{off}ms is neither a trade nor on the {TICK_MS}ms grid",
            );
        }
    }

    /// A quiet gap costs ticks only up to the horizon, not the whole gap.
    #[test]
    fn a_long_quiet_gap_stays_sparse() {
        let created = ts(0.0);
        let col = SeriesColumn::window(MetricId::Buy, WindowSpec::secs(10.0));
        let mut series = MetricSeries::new(created, vec![col]);
        let grid = SparseGrid::for_windows(&[10.0]);
        // Two trades an hour apart. Dense would be 3600/0.2 = 18_000 rows; sparse
        // ticks only to `last_trade + max(window, DEAD_QUIET)` = +300 s.
        fold_sparse(
            &mut series,
            created,
            [(buy(1.0, 0.0), None), (buy(1.0, 3600.0), None)],
            &grid,
            ts(3600.0),
            None,
        );
        let dense = 3600 * 1000 / TICK_MS as usize;
        assert!(series.n_rows() < dense / 4, "gap was not sparse: {} rows", series.n_rows());
        assert!(series.n_rows() > 300 * 1000 / TICK_MS as usize / 2, "horizon was not covered");
    }

    /// The estimate never under-counts the fold it admits.
    #[test]
    fn estimate_upper_bounds_the_real_row_count() {
        let created = ts(0.0);
        let col = SeriesColumn::window(MetricId::Buy, WindowSpec::secs(10.0));
        let times = [0.0, 1.0, 7.5, 400.0, 900.0];
        let grid = SparseGrid::for_windows(&[10.0]);
        let mut series = MetricSeries::new(created, vec![col]);
        let trades: Vec<_> = times.iter().map(|s| (buy(1.0, *s), None)).collect();
        let fold = fold_sparse(&mut series, created, trades, &grid, ts(2000.0), None);
        let est = estimate_sparse_rows(
            created,
            times.iter().map(|s| ts(*s)),
            &grid,
            ts(2000.0),
        );
        assert!(est >= fold.rows, "estimate {est} under-counted actual {}", fold.rows);
    }

    /// A row budget stops the fold and says so — never a silently short series.
    #[test]
    fn a_row_budget_truncates_and_reports() {
        let created = ts(0.0);
        let col = SeriesColumn::window(MetricId::Buy, WindowSpec::secs(60.0));
        let mut series = MetricSeries::new(created, vec![col]);
        let grid = SparseGrid::for_windows(&[60.0]);
        let fold = fold_sparse(
            &mut series,
            created,
            [(buy(1.0, 0.0), None), (buy(1.0, 600.0), None)],
            &grid,
            ts(600.0),
            Some(50),
        );
        assert!(fold.truncated, "a hit budget must report truncated");
        assert_eq!(fold.rows, 50);
        assert_eq!(series.n_rows(), 50);
        assert_eq!(fold.covered_until, *series.at.last().unwrap());
    }

    /// No trades ⇒ no rows, no tail, no truncation.
    #[test]
    fn an_empty_trade_stream_records_nothing() {
        let created = ts(0.0);
        let mut series = MetricSeries::new(created, vec![SeriesColumn::Static(MetricId::Time)]);
        let grid = SparseGrid::default();
        let fold = fold_sparse(&mut series, created, [], &grid, ts(1000.0), None);
        assert_eq!((fold.rows, fold.truncated, fold.covered_until), (0, false, created));
        assert_eq!(estimate_sparse_rows(created, [], &grid, ts(1000.0)), 0);
    }
}
