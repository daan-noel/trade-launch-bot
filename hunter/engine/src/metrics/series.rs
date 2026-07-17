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
/// timestamp of event `i`; `rows[i][c]` is column `c`'s value at that event.
#[derive(Debug, Clone)]
pub struct MetricSeries {
    track: TokenTrack,
    columns: Vec<SeriesColumn>,
    /// Event timestamps, one per recorded row.
    pub at: Vec<Ts>,
    /// One value per column per event (`rows[event][column]`).
    pub rows: Vec<Vec<f64>>,
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
        Self { track, columns, at: Vec::new(), rows: Vec::new() }
    }

    /// Fold a trade and record a row at its timestamp.
    pub fn push_trade(&mut self, t: TradeLite) {
        let at = t.at;
        self.track.on_trade(t);
        self.record(at);
    }

    /// Advance to `now` (no trade) and record a row.
    pub fn push_tick(&mut self, now: Ts) {
        self.track.on_tick(now);
        self.record(now);
    }

    fn record(&mut self, now: Ts) {
        let row: Vec<f64> = self.columns.iter().map(|c| c.eval(&self.track, now)).collect();
        self.at.push(now);
        self.rows.push(row);
    }

    /// The recorded columns, in row order.
    pub fn columns(&self) -> &[SeriesColumn] {
        &self.columns
    }

    /// Values of one column across all recorded events (`None` if not recorded).
    pub fn column_values(&self, col: SeriesColumn) -> Option<Vec<f64>> {
        let idx = self.columns.iter().position(|c| *c == col)?;
        Some(self.rows.iter().map(|r| r[idx]).collect())
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
        s.rows.iter().map(|r| r.iter().map(|v| v.to_bits()).collect()).collect()
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
