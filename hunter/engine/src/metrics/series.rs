//! `MetricSeries` — the sweep's precompute side. One replay pass over a token's
//! trades+ticks emits, for a fixed set of metric **columns**, the metric value
//! at every event. The per-combo scan (plan §2.6) then reads these series with
//! the same [`evaluator`](super::evaluator) fns the live engine uses — so the
//! optimization can never disagree with a full replay.
//!
//! It is deliberately a thin wrapper over [`TokenTrack`]: series values ARE
//! track values, sampled after each fold. That shared compute is what the
//! Phase-1.8 determinism test locks down (track ≡ series, byte-for-byte).

use super::flow_split::FlowPatterns;
use super::track::TokenTrack;
use super::{group_of, MetricGroupId, MetricId, TradeLite, Ts};
use crate::deadness::{is_dead_verdict, DEAD_MEANINGFUL_TRADE_SOL};
use crate::fingerprint::FingerprintId;

/// One column of a [`MetricSeries`] — a static metric, a dynamic metric at a
/// specific `window_size_sec`, or a fingerprint-scoped flow metric.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesColumn {
    Static(MetricId),
    /// A dynamic metric at its window(s) — carries the whole [`Windows`] rather than
    /// one `f64` so a two-window group (`m_flow_burst`) is a column like any other.
    /// Dropping the second axis here would not fail: it would read `NaN` for every
    /// row, which draws as a condition that never holds.
    ///
    /// [`Windows`]: super::Windows
    Window(MetricId, super::Windows),
    /// Flow metric (`m_flow_split` / `m_flow_split_window`) scoped to a fingerprint.
    Flow(MetricId, Option<super::WindowSpec>, FingerprintId),
}

impl SeriesColumn {
    /// A single-window dynamic column — the shape every group but `m_flow_burst` has.
    pub fn window(id: MetricId, spec: super::WindowSpec) -> Self {
        SeriesColumn::Window(id, super::Windows::one(spec))
    }

    fn eval(self, track: &TokenTrack, now: Ts) -> f64 {
        match self {
            SeriesColumn::Static(id) => track.value(id, super::Windows::NONE, None, now),
            SeriesColumn::Window(id, ws) => track.value(id, ws, None, now),
            SeriesColumn::Flow(id, ws, fp) => track.value(id, ws.into(), Some(fp), now),
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

/// Register a windowed column's backing deque on `track`, routed to the buffer its
/// metric actually reads: `m_price_window` (`WinTrail`/`WinRise`) needs the
/// price-extrema deque (`ensure_price_window`), every other windowed group the flow
/// deque (`ensure_window`). They share the `window_size_sec` param but not the state,
/// so registering only one — as the old blanket `ensure_window` did — left the price
/// windows unregistered and every `m_price_window` column reading `NaN`. This mirrors
/// the live engine's split registration in `state.rs`.
fn register_window(track: &mut TokenTrack, id: MetricId, ws: super::WindowSpec) {
    if group_of(id).id == MetricGroupId::PriceWindow {
        track.ensure_price_window(ws);
    } else {
        track.ensure_window(ws);
    }
}

impl MetricSeries {
    /// Start a series for a token created at `created_at`, recording `columns`.
    /// Any dynamic column's window is registered up front so the whole history
    /// feeds it.
    pub fn new(created_at: Ts, columns: Vec<SeriesColumn>) -> Self {
        let mut track = TokenTrack::new(created_at);
        for c in &columns {
            if let SeriesColumn::Window(id, ws) = c {
                // Both axes — registering only the primary leaves a burst column NaN.
                for w in [ws.primary, ws.secondary].into_iter().flatten() {
                    register_window(&mut track, *id, w);
                }
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

    /// Register a trailing **flow** window on the track before folding.
    ///
    /// [`new`](Self::new) already registers whatever a [`SeriesColumn::Window`]
    /// needs, so this is for a caller that registers off a *rule* rather than off
    /// its column set — `CompiledRule::flow_windows` covers `m_flow_split_window`
    /// too, whose columns are [`SeriesColumn::Flow`] and so are registered by
    /// [`ensure_flow`](Self::ensure_flow) instead. Registering the same width twice
    /// is a no-op, which is what lets a caller mirror the live track's setup
    /// verbatim rather than reasoning about which half owns which width.
    pub fn ensure_window(&mut self, spec: super::WindowSpec) {
        self.track.ensure_window(spec);
    }

    /// Register a rolling **price-extrema** window (`m_price_window`) — the twin of
    /// [`ensure_window`](Self::ensure_window) for the other deque family.
    pub fn ensure_price_window(&mut self, spec: super::WindowSpec) {
        self.track.ensure_price_window(spec);
    }

    /// Attach fingerprint-scoped flow state (and optional window sizes) before
    /// folding trades. Required for [`SeriesColumn::Flow`] columns to leave `NaN`.
    pub fn ensure_flow(
        &mut self,
        fp: FingerprintId,
        patterns: &FlowPatterns,
        windows: &[super::WindowSpec],
    ) {
        self.track.ensure_flow(fp, patterns, windows);
    }

    /// Seed the creator wallet hash (volume-side unconditionally).
    pub fn seed_creator(&mut self, hash: u64) {
        self.track.seed_creator(hash);
    }

    /// Seed the creation-transaction instruction count (`m_snapshot.ix_count`).
    ///
    /// Must be called before the first fold on every path that records a series, or
    /// `ix_count` reads `NaN` for the whole token and any `ix_count <= N` entry gate is
    /// silently unsatisfiable — the token is never entered, with no error anywhere.
    /// Seed the creation slot's buy total. Same before-the-first-fold contract as
    /// [`seed_ix_count`](Self::seed_ix_count) — it is a token-static fact, so the
    /// series records the same value on every row.
    pub fn seed_first_slot_buy(&mut self, sol: f64) {
        self.track.seed_first_slot_buy(sol);
    }

    pub fn seed_ix_count(&mut self, n: usize) {
        self.track.seed_ix_count(n);
    }

    /// Seed the creator's prior-launch count (`m_snapshot.prior_launches`).
    ///
    /// Same before-the-first-fold contract as [`seed_ix_count`](Self::seed_ix_count),
    /// and the same silent failure when skipped: the metric reads `NaN` for the whole
    /// token, so a `prior_launches == 0` gate never fires and nothing reports why.
    pub fn seed_prior_launches(&mut self, n: u32) {
        self.track.seed_prior_launches(n);
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
    use crate::metrics::Side;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
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
            SeriesColumn::Static(MetricId::Time),
            SeriesColumn::Static(MetricId::Liquidity),
            SeriesColumn::Static(MetricId::Stall),
            SeriesColumn::Static(MetricId::Trail),
            SeriesColumn::Static(MetricId::LifeGrossFlow),
            SeriesColumn::Static(MetricId::LifeBuy),
            SeriesColumn::window(MetricId::GrossFlow, WindowSpec::secs(10.0)),
            SeriesColumn::window(MetricId::NetFlow, WindowSpec::secs(10.0)),
            SeriesColumn::window(MetricId::Buy, WindowSpec::secs(10.0)),
            SeriesColumn::window(MetricId::Sell, WindowSpec::secs(10.0)),
        ]
    }

    /// Drive a bare `TokenTrack` over the same script, sampling the same columns
    /// after each fold — the reference the series must match bit-for-bit.
    fn track_reference(created: Ts, cols: &[SeriesColumn], evs: &[Ev]) -> Vec<Vec<u64>> {
        let mut track = TokenTrack::new(created);
        for c in cols {
            if let SeriesColumn::Window(id, ws) = c {
                // Both axes — registering only the primary leaves a burst column NaN.
                for w in [ws.primary, ws.secondary].into_iter().flatten() {
                    register_window(&mut track, *id, w);
                }
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
        let col = SeriesColumn::window(MetricId::WinTrail, WindowSpec::secs(30.0));
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
        let time = s.column_values(SeriesColumn::Static(MetricId::Time)).unwrap();
        assert_eq!(time.first().copied(), Some(0.0));
        assert_eq!(time.last().copied(), Some(12.0));
        assert!(s.column_values(SeriesColumn::Static(MetricId::Buy)).is_none());
    }
}
