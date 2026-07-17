//! `MetricSeries` — the sweep's precompute side. One replay pass over a token's
//! trades+ticks emits, for a fixed set of metric **columns**, the metric value
//! at every event. The per-combo scan (plan §2.6) then reads these series with
//! the same [`evaluator`](super::evaluator) fns the live engine uses — so the
//! optimization can never disagree with a full replay.
//!
//! It is deliberately a thin wrapper over [`TokenTrack`]: series values ARE
//! track values, sampled after each fold. That shared compute is what the
//! Phase-1.8 determinism test locks down (track ≡ series, byte-for-byte).

use super::track::TokenTrack;
use super::{MetricId, TradeLite, Ts};
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL};

/// One column of a [`MetricSeries`] — a static metric, or a dynamic metric at a
/// specific `window_size_sec`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesColumn {
    Static(MetricId),
    Window(MetricId, f64),
}

impl SeriesColumn {
    fn eval(self, track: &TokenTrack, now: Ts) -> f64 {
        match self {
            SeriesColumn::Static(id) => track.value(id, None, now),
            SeriesColumn::Window(id, ws) => track.value(id, Some(ws), now),
        }
    }
}

/// Per-event metric values for one token over a fixed column set. `at[i]` is the
/// timestamp of event `i`; the metric values live in one **flat** row-major buffer
/// ([`value_at`](Self::value_at)) with `n_cols` stride — one allocation for the
/// whole series (not one per row), so the scan reads cache-linearly and long
/// tokens don't pay per-row allocator overhead (plan §P1).
///
/// Beyond the metric columns, each row also carries the three rule-independent
/// facts the per-combo scan needs but that no metric expresses: the canonical
/// spot [`price`](Self::price) at the event (the fill price a `SubmitBuy`/`SubmitSell`
/// would take, and the TP/SL reference), the [`reserve_sol`](Self::reserve_sol),
/// and the precomputed [`dead`](Self::dead) verdict. `dead` is rule-independent, so
/// computing it once per event here — not per combo — is the precompute-then-scan
/// win (plan §2.6): the scan reads `Dead > SL > TP > Metrics` off these columns
/// with the exact values the live engine's [`reduce`](crate::reduce) sees.
#[derive(Debug, Clone)]
pub struct MetricSeries {
    track: TokenTrack,
    columns: Vec<SeriesColumn>,
    /// The last trade at or before `now` whose SOL cleared the meaningful-trade
    /// floor — the deadness clock (mirrors `EngineState`'s `last_meaningful_at`).
    last_meaningful_at: Ts,
    /// Column count = row stride into [`values`](Self::values).
    n_cols: usize,
    /// Flat row-major metric values: row `r`, column `c` → `values[r * n_cols + c]`.
    /// Read through [`value_at`](Self::value_at).
    values: Vec<f64>,
    /// Event timestamps, one per recorded row.
    pub at: Vec<Ts>,
    /// Canonical spot price at each event (`NaN` before the first trade).
    pub price: Vec<f64>,
    /// SOL reserves at each event (`NaN` before the first trade).
    pub reserve_sol: Vec<f64>,
    /// The dead-token verdict at each event — the same `is_dead_verdict` the live
    /// engine computes per token per event, precomputed once here.
    pub dead: Vec<bool>,
}

impl MetricSeries {
    /// Start a series for a token created at `created_at`, recording `columns`.
    /// Any dynamic column's window is registered up front so the whole history
    /// feeds it.
    pub fn new(created_at: Ts, columns: Vec<SeriesColumn>) -> Self {
        let mut track = TokenTrack::new(created_at);
        for c in &columns {
            if let SeriesColumn::Window(_, ws) = c {
                track.ensure_window(*ws);
            }
        }
        let n_cols = columns.len();
        Self {
            track,
            columns,
            last_meaningful_at: created_at,
            n_cols,
            values: Vec::new(),
            at: Vec::new(),
            price: Vec::new(),
            reserve_sol: Vec::new(),
            dead: Vec::new(),
        }
    }

    /// The deadness clock (newest meaningful-trade time) after every fold so far —
    /// the sparse-grid builder reads it to place the dead-flip tick (plan §P2).
    pub fn last_meaningful_at(&self) -> Ts {
        self.last_meaningful_at
    }

    /// Fold a trade and record a row at its timestamp.
    pub fn push_trade(&mut self, t: TradeLite) {
        let at = t.at;
        // Advance the deadness clock exactly as `reduce` does on a `Trade` event:
        // a trade that clears the meaningful floor and is not out of order refreshes
        // `last_meaningful_at`.
        if t.sol >= DEAD_MEANINGFUL_TRADE_SOL && at >= self.last_meaningful_at {
            self.last_meaningful_at = at;
        }
        self.track.on_trade(t);
        self.record(at);
    }

    /// Advance to `now` (no trade) and record a row.
    pub fn push_tick(&mut self, now: Ts) {
        self.track.on_tick(now);
        self.record(now);
    }

    fn record(&mut self, now: Ts) {
        // Append this row's columns into the flat buffer (stride `n_cols`); the
        // buffer grows by one row per event with no per-row allocation.
        self.values.reserve(self.n_cols);
        for c in &self.columns {
            self.values.push(c.eval(&self.track, now));
        }
        let reserves = self.track.current_reserves();
        let dead = is_dead_verdict(reserves.is_finite().then_some(reserves), self.last_meaningful_at, now);
        self.at.push(now);
        self.price.push(self.track.current_price());
        self.reserve_sol.push(reserves);
        self.dead.push(dead);
    }

    /// The recorded columns, in row order.
    pub fn columns(&self) -> &[SeriesColumn] {
        &self.columns
    }

    /// Number of recorded event rows.
    pub fn n_rows(&self) -> usize {
        self.at.len()
    }

    /// The metric value at `(row, col_idx)` from the flat buffer. `col_idx` is a
    /// column position resolved once via [`col_index`](Self::col_index) — the scan
    /// hoists it out of its per-row loop (plan §P1) instead of re-searching per read.
    #[inline]
    pub fn value_at(&self, row: usize, col_idx: usize) -> f64 {
        self.values[row * self.n_cols + col_idx]
    }

    /// Resolve a column's index in this series' fixed column set (`None` if the
    /// column wasn't recorded). Columns are constant for the whole series, so a
    /// scan resolves each requirement's index once per run, not per row.
    pub fn col_index(&self, col: SeriesColumn) -> Option<usize> {
        self.columns.iter().position(|c| *c == col)
    }

    /// Values of one column across all recorded events (`None` if not recorded).
    pub fn column_values(&self, col: SeriesColumn) -> Option<Vec<f64>> {
        let idx = self.col_index(col)?;
        Some((0..self.n_rows()).map(|r| self.value_at(r, idx)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Side;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn trade(side: Side, sol: f64, price: f64, reserve: f64, secs: f64) -> TradeLite {
        TradeLite { side, sol, price, reserve_sol: reserve, at: ts(secs) }
    }

    /// An event script: trades interleaved with bare ticks.
    enum Ev {
        Trade(TradeLite),
        Tick(Ts),
    }

    fn script() -> Vec<Ev> {
        vec![
            Ev::Trade(trade(Side::Buy, 3.0, 1.0, 15.0, 0.0)),
            Ev::Tick(ts(0.5)),
            Ev::Trade(trade(Side::Sell, 1.0, 1.2, 14.0, 1.0)),
            Ev::Tick(ts(1.5)),
            Ev::Tick(ts(2.0)),
            Ev::Trade(trade(Side::Buy, 2.0, 0.9, 16.0, 3.0)),
            Ev::Tick(ts(12.0)), // pushes early trades out of a 10 s window
        ]
    }

    fn columns() -> Vec<SeriesColumn> {
        vec![
            SeriesColumn::Static(MetricId::Time),
            SeriesColumn::Static(MetricId::Liquidity),
            SeriesColumn::Static(MetricId::Stall),
            SeriesColumn::Static(MetricId::Trail),
            SeriesColumn::Window(MetricId::GrossFlow, 10.0),
            SeriesColumn::Window(MetricId::NetFlow, 10.0),
            SeriesColumn::Window(MetricId::Buy, 10.0),
            SeriesColumn::Window(MetricId::Sell, 10.0),
        ]
    }

    /// Drive a bare `TokenTrack` over the same script, sampling the same columns
    /// after each fold — the reference the series must match bit-for-bit.
    fn track_reference(created: Ts, cols: &[SeriesColumn], evs: &[Ev]) -> Vec<Vec<u64>> {
        let mut track = TokenTrack::new(created);
        for c in cols {
            if let SeriesColumn::Window(_, ws) = c {
                track.ensure_window(*ws);
            }
        }
        let mut out = Vec::new();
        for ev in evs {
            let now = match ev {
                Ev::Trade(t) => {
                    let at = t.at;
                    track.on_trade(*t);
                    at
                }
                Ev::Tick(now) => {
                    track.on_tick(*now);
                    *now
                }
            };
            out.push(cols.iter().map(|c| c.eval(&track, now).to_bits()).collect());
        }
        out
    }

    fn series_bits(created: Ts, cols: Vec<SeriesColumn>, evs: &[Ev]) -> Vec<Vec<u64>> {
        let mut s = MetricSeries::new(created, cols);
        for ev in evs {
            match ev {
                Ev::Trade(t) => s.push_trade(*t),
                Ev::Tick(now) => s.push_tick(*now),
            }
        }
        (0..s.n_rows())
            .map(|r| (0..s.columns().len()).map(|c| s.value_at(r, c).to_bits()).collect())
            .collect()
    }

    #[test]
    fn series_equals_track_and_is_reproducible() {
        let created = ts(0.0);
        // Two independent runs of the identical script.
        let a = series_bits(created, columns(), &script());
        let b = series_bits(created, columns(), &script());
        // Byte-identical across runs (determinism).
        assert_eq!(a, b, "series is not reproducible");
        // Byte-identical to the bare-track reference (same compute, no drift).
        let reference = track_reference(created, &columns(), &script());
        assert_eq!(a, reference, "series diverged from TokenTrack");
        // Sanity: one row per event.
        assert_eq!(a.len(), script().len());
    }

    #[test]
    fn records_price_reserves_and_deadness_per_row() {
        // A token that trades then goes silent with drained liquidity should flip
        // `dead` once past the quiet window with low reserves.
        let created = ts(0.0);
        let mut s = MetricSeries::new(created, columns());
        // Live trade: finite price + healthy reserves ⇒ alive.
        s.push_trade(trade(Side::Buy, 3.0, 1.0, 15.0, 0.0));
        assert_eq!(s.price.last().copied(), Some(1.0));
        assert_eq!(s.reserve_sol.last().copied(), Some(15.0));
        assert!(!s.dead.last().copied().unwrap());
        // A tiny dust trade drops reserves under the dead floor but does NOT refresh
        // the meaningful-trade clock (sol < DEAD_MEANINGFUL_TRADE_SOL).
        s.push_trade(trade(Side::Sell, 0.01, 0.9, 1.0, 1.0));
        // Long after the last meaningful trade with reserves gone ⇒ dead.
        s.push_tick(ts(1.0 + crate::deadness::DEAD_QUIET_SECS as f64 + 5.0));
        assert!(s.dead.last().copied().unwrap(), "silent + drained token must read dead");
        // Price on a tick carries the last print (no trade at the tick).
        assert_eq!(s.price.last().copied(), Some(0.9));
    }

    #[test]
    fn column_values_extracts_one_series() {
        let mut s = MetricSeries::new(ts(0.0), columns());
        for ev in script() {
            match ev {
                Ev::Trade(t) => s.push_trade(t),
                Ev::Tick(now) => s.push_tick(now),
            }
        }
        let time = s.column_values(SeriesColumn::Static(MetricId::Time)).unwrap();
        assert_eq!(time.first().copied(), Some(0.0));
        assert_eq!(time.last().copied(), Some(12.0));
        assert!(s.column_values(SeriesColumn::Static(MetricId::Buy)).is_none());
    }
}
