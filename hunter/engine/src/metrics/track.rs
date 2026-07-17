//! `TokenTrack` — the per-token metric state the engine folds trades and ticks
//! into. It composes the three group states:
//! * static [`SnapshotState`] + [`PricePathState`] — one copy, shared by every
//!   rule armed on the token;
//! * dynamic [`WindowState`]s — **deduped by `window_size_sec`**, so rules that
//!   share a window share one buffer.
//!
//! Two fold entry points, matching the engine's events: [`on_trade`] and
//! [`on_tick`]. Reads go through [`value`], which routes a `MetricId` (+ an
//! optional window for dynamic metrics) to the owning group. All reads take the
//! current `now` so every metric is evaluated at the same instant.
//!
//! [`on_trade`]: TokenTrack::on_trade
//! [`on_tick`]: TokenTrack::on_tick
//! [`value`]: TokenTrack::value

use std::collections::BTreeMap;

use super::price_path::PricePathState;
use super::snapshot::SnapshotState;
use super::time_window::{window_key, WindowState};
use super::{MetricId, TradeLite, Ts};

/// All metric state for one token.
#[derive(Debug, Clone)]
pub struct TokenTrack {
    created_at: Ts,
    snapshot: SnapshotState,
    price_path: PricePathState,
    /// Dynamic windows, keyed by [`window_key`] so equal sizes dedupe.
    windows: BTreeMap<u64, WindowState>,
}

impl TokenTrack {
    /// Fresh state for a token created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self {
            created_at,
            snapshot: SnapshotState::default(),
            price_path: PricePathState::new(created_at),
            windows: BTreeMap::new(),
        }
    }

    /// Register a trailing window (idempotent; deduped by `window_size_sec`).
    /// The engine calls this for every distinct `window_size_sec` its loaded
    /// rules reference before the token starts receiving trades.
    pub fn ensure_window(&mut self, window_secs: f64) {
        self.windows
            .entry(window_key(window_secs))
            .or_insert_with(|| WindowState::new(window_secs));
    }

    /// Fold one trade into every group.
    pub fn on_trade(&mut self, t: TradeLite) {
        self.snapshot.on_trade(t.reserve_sol);
        self.price_path.on_trade(t.price, t.at);
        for w in self.windows.values_mut() {
            w.on_trade(t.side, t.sol, t.at);
        }
    }

    /// Advance time to `now` without a trade — evicts stale window entries so a
    /// quiet token's flows decay (and the stall/time metrics read against `now`
    /// at the call site).
    pub fn on_tick(&mut self, now: Ts) {
        for w in self.windows.values_mut() {
            w.evict(now);
        }
    }

    /// The most recently observed canonical price (`NaN` before the first trade).
    /// The "last known price" a tick-driven TP/SL check reads (a tick carries no
    /// trade, so the position is marked against the last print).
    pub fn current_price(&self) -> f64 {
        self.price_path.last_price()
    }

    /// The most recently observed SOL reserves (`NaN` before the first trade) — the
    /// liquidity reading the dead-token verdict consumes.
    pub fn current_reserves(&self) -> f64 {
        self.value(MetricId::Liquidity, None, self.created_at)
    }

    /// Value of one metric at `now`. `window_secs` is required for dynamic
    /// (`m_time_window`) metrics and ignored for static ones. An unregistered
    /// window (or a missing one) yields `NaN` — which satisfies no condition.
    pub fn value(&self, id: MetricId, window_secs: Option<f64>, now: Ts) -> f64 {
        use MetricId::*;
        match id {
            Time | Liquidity => self.snapshot.value(id, self.created_at, now),
            Stall | Trail => self.price_path.value(id, now),
            GrossFlow | NetFlow | Buy | Sell => {
                match window_secs.and_then(|ws| self.windows.get(&window_key(ws))) {
                    Some(w) => w.value(id),
                    None => f64::NAN,
                }
            }
        }
    }

    /// Batch read a set of `(metric, optional window)` requests into a
    /// caller-owned buffer — no allocation on the hot path. `out` must be at
    /// least `reqs.len()` long; extra slots are left untouched.
    pub fn values(&self, reqs: &[(MetricId, Option<f64>)], now: Ts, out: &mut [f64]) {
        for (slot, &(id, ws)) in out.iter_mut().zip(reqs) {
            *slot = self.value(id, ws, now);
        }
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

    fn buy(sol: f64, price: f64, reserve: f64, secs: f64) -> TradeLite {
        TradeLite { side: Side::Buy, sol, price, reserve_sol: reserve, at: ts(secs) }
    }

    #[test]
    fn routes_each_metric_to_its_group() {
        let created = ts(0.0);
        let mut track = TokenTrack::new(created);
        track.ensure_window(10.0);
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));

        assert_eq!(track.value(MetricId::Time, None, ts(5.0)), 5.0);
        assert_eq!(track.value(MetricId::Liquidity, None, ts(5.0)), 15.0);
        assert_eq!(track.value(MetricId::Stall, None, ts(5.0)), 4.0); // moved at t=1
        assert_eq!(track.value(MetricId::Trail, None, ts(5.0)), 0.0); // at peak
        assert_eq!(track.value(MetricId::Buy, Some(10.0), ts(5.0)), 3.0);
        assert_eq!(track.value(MetricId::GrossFlow, Some(10.0), ts(5.0)), 3.0);
    }

    #[test]
    fn unregistered_or_missing_window_is_nan() {
        let mut track = TokenTrack::new(ts(0.0));
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        // No window ensured → NaN.
        assert!(track.value(MetricId::Buy, Some(10.0), ts(5.0)).is_nan());
        // Dynamic metric with no window arg → NaN.
        track.ensure_window(10.0);
        assert!(track.value(MetricId::Buy, None, ts(5.0)).is_nan());
    }

    #[test]
    fn equal_windows_dedupe_to_one_buffer() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.ensure_window(10.0);
        assert_eq!(track.windows.len(), 1);
        track.ensure_window(5.0);
        assert_eq!(track.windows.len(), 2);
    }

    #[test]
    fn tick_decays_quiet_flows() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), ts(5.0)), 4.0);
        // Tick past the window edge → flow decays to zero even with no trade.
        track.on_tick(ts(11.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), ts(11.0)), 0.0);
    }

    #[test]
    fn values_batch_fills_caller_buffer() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        let reqs = [
            (MetricId::Time, None),
            (MetricId::Liquidity, None),
            (MetricId::Buy, Some(10.0)),
        ];
        let mut out = [0.0_f64; 3];
        track.values(&reqs, ts(5.0), &mut out);
        assert_eq!(out, [5.0, 15.0, 3.0]);
    }
}
