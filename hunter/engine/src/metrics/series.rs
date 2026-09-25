//! `MetricSeries` — the sweep's precompute side. One replay pass over a token's
//! trades+ticks emits, for a fixed set of metric **columns**, the metric value
//! at every event. The per-combo scan (plan §2.6) then reads these series with
//! the same [`evaluator`](super::evaluator) fns the live engine uses — so the
//! optimization can never disagree with a full replay.
//!
//! It is deliberately a thin wrapper over [`TokenTrack`]: series values ARE
//! track values, sampled after each fold. That shared compute is what the
//! Phase-1.8 determinism test locks down (track ≡ series, byte-for-byte).

use super::buffers::Buffers;
use super::burst_slot::TemplatePatterns;
use super::tags::config::TagPatterns;
use super::tags::TagKey;
use super::track::TokenTrack;
use super::{MetricRef, TradeLite, Ts};
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL};
use crate::fingerprint::FingerprintId;

/// One column of a [`MetricSeries`]: a read ([`MetricRef`]) and, for a fingerprint tag,
/// the fingerprint it is scoped to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeriesColumn {
    pub r: MetricRef,
    pub fp: Option<FingerprintId>,
}

impl SeriesColumn {
    /// A coin-level read.
    pub fn of(r: MetricRef) -> Self {
        Self { r, fp: None }
    }

    /// A read scoped to a fingerprint's tag.
    pub fn tagged(r: MetricRef, fp: FingerprintId) -> Self {
        Self { r, fp: Some(fp) }
    }

    fn eval(self, track: &TokenTrack, now: Ts) -> f64 {
        track.value(self.r, self.fp, now)
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
    /// Slot of the trade on each row: `Some(slot)` on a trade row, `None` on a tick
    /// (or when fed via [`push_trade`] with no slot). Lets the sweep drill-in map a
    /// fill row to the real trade it fills against — the first trade at/after the
    /// fill row (see the drill-in's `fill_trade_slot`) → its `tx_signature`. Not
    /// carried across ticks, so a fill never resolves back to a stale earlier trade.
    pub slot: Vec<Option<u64>>,
    /// Canonical spot price at each event (`NaN` before the first trade).
    pub price: Vec<f64>,
    /// **Real** SOL reserves at each event (`NaN` before the first trade) — the
    /// `liquidity` reading and the deadness input.
    pub reserve_sol: Vec<f64>,
    /// **Priced** SOL depth (`vsol`) at each event, for price impact only. Charging
    /// impact against `reserve_sol` overcharges by `vsol / (vsol - 30)` on the curve;
    /// see [`TradeLite::priced_reserve_sol`](super::TradeLite::priced_reserve_sol).
    pub priced_reserve_sol: Vec<f64>,
    /// The dead-token verdict at each event — the same `is_dead_verdict` the live
    /// engine computes per token per event, precomputed once here.
    pub dead: Vec<bool>,
    /// Suppress **recording** (not folding) before this instant. See
    /// [`set_record_from`](Self::set_record_from). `None` records from the first event.
    record_from: Option<Ts>,
}

/// The buyer-set cap a series opens a since-age column at. A series has no rule to
/// derive the cap from, and it draws the count rather than judging a threshold, so it
/// takes a fixed cap wide enough for any chart: past it the count reads the cap.
pub const SERIES_ANCHOR_CAP: u32 = 64;

impl MetricSeries {
    /// Start a series for a token created at `created_at`, recording `columns`.
    /// Any dynamic column's window is registered up front so the whole history
    /// feeds it.
    pub fn new(created_at: Ts, columns: Vec<SeriesColumn>) -> Self {
        let mut track = TokenTrack::new(created_at);
        // Every coin-level buffer the columns read, from the same walk the engine
        // registers from. Tag columns also need their definition: `ensure_tag`.
        Buffers::of(columns.iter().map(|c| c.r), SERIES_ANCHOR_CAP).ensure_on(&mut track);
        let n_cols = columns.len();
        Self {
            track,
            columns,
            last_meaningful_at: created_at,
            n_cols,
            values: Vec::new(),
            at: Vec::new(),
            slot: Vec::new(),
            price: Vec::new(),
            reserve_sol: Vec::new(),
            priced_reserve_sol: Vec::new(),
            dead: Vec::new(),
            record_from: None,
        }
    }

    /// Start **recording** rows at `from` while still folding every event from
    /// creation.
    ///
    /// The two are not the same knob and must not be collapsed into one. Lifetime
    /// metrics (`m_price_lifetime`, `time`) are defined from token creation, so a fold
    /// that *starts* later reports different numbers — a silent wrong answer. A fold
    /// that starts at creation and merely withholds early rows reports the same numbers
    /// over a narrower span, which is an honest one.
    ///
    /// The reason to want it: a row budget buys a fixed *span* of grid, so where that
    /// span sits decides whether it covers the position anyone opened the modal to look
    /// at. Suppressed rows cost no budget, so the whole budget lands on the window the
    /// caller asked for.
    pub fn set_record_from(&mut self, from: Ts) {
        self.record_from = Some(from);
    }

    /// Register the buffers a rule reads, when a caller registers off a rule rather
    /// than off its column set (the sweep's drill-in mirrors the live track this way).
    /// Registering twice is a no-op.
    pub fn ensure_buffers(&mut self, b: &Buffers) {
        b.ensure_on(&mut self.track);
    }

    /// Open one fingerprint tag's trade-level state (and windows) before folding. A
    /// tagged column reads `NaN` without it.
    pub fn ensure_tag(&mut self, fp: FingerprintId, key: TagKey, patterns: &TagPatterns, windows: &[super::WindowSpec]) {
        self.track.ensure_tag(fp, key, patterns, windows);
    }

    /// Register one tag's template view (slot / wave columns).
    pub fn ensure_template_tag(&mut self, fp: FingerprintId, key: TagKey, patterns: &TemplatePatterns) {
        self.track.ensure_template_tag(fp, key, patterns);
    }

    /// Seed the creator wallet hash on every tag state.
    pub fn seed_creator(&mut self, hash: u64) {
        self.track.seed_creator(hash);
    }

    /// The deadness clock (newest meaningful-trade time) after every fold so far —
    /// the sparse-grid builder reads it to place the dead-flip tick (plan §P2).
    pub fn last_meaningful_at(&self) -> Ts {
        self.last_meaningful_at
    }

    /// Fold a trade and record a row at its timestamp (no slot — the metric-series
    /// API path, which doesn't resolve fills back to trades).
    pub fn push_trade(&mut self, t: TradeLite) {
        self.push_trade_at(t, None);
    }

    /// Fold a trade recorded at its real corpus `slot` — the sweep drill-in path,
    /// so the fill row can be resolved back to the trade's `tx_signature`.
    pub fn push_trade_at(&mut self, t: TradeLite, slot: Option<u64>) {
        let at = t.at;
        // Advance the deadness clock exactly as `reduce` does on a `Trade` event:
        // a trade that clears the meaningful floor and is not out of order refreshes
        // `last_meaningful_at`.
        if t.sol >= DEAD_MEANINGFUL_TRADE_SOL && at >= self.last_meaningful_at {
            self.last_meaningful_at = at;
        }
        self.track.on_trade(t);
        self.record(at, slot);
    }

    /// Advance to `now` (no trade) and record a row.
    pub fn push_tick(&mut self, now: Ts) {
        // A replayed series folds trades in order, so the track's own slot cursor
        // is already at the last trade - a synthetic tick must not move it.
        self.track.on_tick(now, None);
        self.record(now, None);
    }

    fn record(&mut self, now: Ts, slot: Option<u64>) {
        // Withheld rows are folded but not stored — the track has already advanced by
        // the time we get here, so skipping the append changes coverage, never a value.
        if self.record_from.is_some_and(|from| now < from) {
            return;
        }
        // Append this row's columns into the flat buffer (stride `n_cols`); the
        // buffer grows by one row per event with no per-row allocation.
        self.values.reserve(self.n_cols);
        for c in &self.columns {
            self.values.push(c.eval(&self.track, now));
        }
        let reserves = self.track.current_reserves();
        let dead = is_dead_verdict(reserves.is_finite().then_some(reserves), self.last_meaningful_at, now);
        self.at.push(now);
        self.slot.push(slot);
        self.price.push(self.track.current_price());
        self.reserve_sol.push(reserves);
        self.priced_reserve_sol.push(self.track.current_priced_reserves());
        self.dead.push(dead);
    }

    /// The fold's state after the last recorded row — the coin as a later instant with
    /// no new print reads it (the sweep's frozen-tail resolve).
    pub fn track(&self) -> &TokenTrack {
        &self.track
    }

    /// The token's creation instant.
    pub fn created_at(&self) -> Ts {
        self.track.created_at()
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
    use crate::metrics::WindowSpec;
    use super::*;
    use crate::metrics::{Metric, Side};
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn life(m: Metric) -> SeriesColumn {
        SeriesColumn::of(MetricRef::life(m))
    }

    fn win(m: Metric, w: WindowSpec) -> SeriesColumn {
        SeriesColumn::of(MetricRef::life(m).with_span(crate::metrics::Span::window(w)))
    }

    fn trade(side: Side, sol: f64, price: f64, reserve: f64, secs: f64) -> TradeLite {
        TradeLite { side, sol, price, reserve_sol: reserve, at: ts(secs), ..Default::default() }
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
            life(Metric::AgeSec),
            life(Metric::LiquiditySol),
            life(Metric::StallSec),
            life(Metric::TrailPct),
            life(Metric::GrossSol),
            life(Metric::BuySol),
            win(Metric::GrossSol, WindowSpec::secs(10.0)),
            win(Metric::NetSol, WindowSpec::secs(10.0)),
            win(Metric::BuySol, WindowSpec::secs(10.0)),
            win(Metric::SellSol, WindowSpec::secs(10.0)),
        ]
    }

    /// Drive a bare `TokenTrack` over the same script, sampling the same columns
    /// after each fold — the reference the series must match bit-for-bit.
    fn track_reference(created: Ts, cols: &[SeriesColumn], evs: &[Ev]) -> Vec<Vec<u64>> {
        let mut track = TokenTrack::new(created);
        for c in cols {
            if let Some(w) = c.r.span.window {
                track.ensure_window(w);
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
                    track.on_tick(*now, None);
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
    fn price_window_column_is_registered_and_finite() {
        // Regression: `m_price_window` (WinTrail/WinRise) reads a SEPARATE price-extrema
        // deque from the flow windows. The series must route its registration to
        // `ensure_price_window`; the old blanket `ensure_window` left it unregistered so
        // every price-window column read `NaN` (empty panes / dead sweep entry gate).
        let created = ts(0.0);
        let col = win(Metric::TrailPct, WindowSpec::secs(30.0));
        let mut s = MetricSeries::new(created, vec![col]);
        s.push_trade(trade(Side::Buy, 3.0, 2.0, 15.0, 0.0)); // rolling high = 2.0
        s.push_trade(trade(Side::Sell, 1.0, 1.5, 14.0, 1.0)); // dip to 1.5 → 25% below high
        let trail = s.column_values(col).unwrap();
        assert!(trail.iter().all(|v| v.is_finite()), "price-window column must not be NaN");
        assert!((trail.last().unwrap() - 25.0).abs() < 1e-9, "trail = % below the rolling high");
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
        let time = s.column_values(life(Metric::AgeSec)).unwrap();
        assert_eq!(time.first().copied(), Some(0.0));
        assert_eq!(time.last().copied(), Some(12.0));
        assert!(s.column_values(win(Metric::BuySol, WindowSpec::secs(99.0))).is_none());
    }
}
