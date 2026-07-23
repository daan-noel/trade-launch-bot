//! `TokenTrack` — the per-token metric state the engine folds trades and ticks
//! into. It composes the group states:
//! * static [`SnapshotState`] + [`PriceLifetimeState`] + [`FlowLifetimeState`] —
//!   one copy each, shared by every rule armed on the token;
//! * dynamic [`WindowState`]s — **deduped by `window_size_sec`**, so rules that
//!   share a window share one buffer;
//! * fingerprint-scoped [`FlowState`]s — one classifier + totals per fingerprint
//!   that has `m_flow_split` configured (volume-flow-split plan).
//!
//! Two fold entry points, matching the engine's events: [`on_trade`] and
//! [`on_tick`]. Reads go through [`value`], which routes a `MetricId` (+ an
//! optional window for dynamic metrics, + fingerprint for flow groups) to the
//! owning group. All reads take the current `now` so every metric is evaluated
//! at the same instant.
//!
//! [`on_trade`]: TokenTrack::on_trade
//! [`on_tick`]: TokenTrack::on_tick
//! [`value`]: TokenTrack::value

use std::collections::BTreeMap;

use crate::fingerprint::FingerprintId;

use super::flow_lifetime::FlowLifetimeState;
use super::flow_split::{FlowPatterns, FlowState};
use super::flow_window::{window_key, WindowState};
use super::price_lifetime::PriceLifetimeState;
use super::price_window::PriceWindowState;
use super::snapshot::SnapshotState;
use super::{MetricId, TradeLite, Ts};

/// All metric state for one token.
#[derive(Debug, Clone)]
pub struct TokenTrack {
    created_at: Ts,
    snapshot: SnapshotState,
    price_lifetime: PriceLifetimeState,
    flow_lifetime: FlowLifetimeState,
    /// Dynamic flow windows, keyed by [`window_key`] so equal sizes dedupe.
    windows: BTreeMap<u64, WindowState>,
    /// Dynamic price-extrema windows, keyed by [`window_key`]. Kept apart from
    /// `windows` so a rule using only `m_flow_window` pays for no price deque (and
    /// vice versa) — the two dynamic groups share the `window_size_sec` param name
    /// but not the buffers.
    price_windows: BTreeMap<u64, PriceWindowState>,
    /// Flow classifier state, keyed by fingerprint (pattern sets differ).
    flow: BTreeMap<FingerprintId, FlowState>,
    /// Creator wallet hash from `TokenCreated` — applied to every FlowState.
    creator_wallet_hash: Option<u64>,
}

impl TokenTrack {
    /// Fresh state for a token created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self {
            created_at,
            snapshot: SnapshotState::default(),
            price_lifetime: PriceLifetimeState::new(created_at),
            flow_lifetime: FlowLifetimeState::default(),
            windows: BTreeMap::new(),
            price_windows: BTreeMap::new(),
            flow: BTreeMap::new(),
            creator_wallet_hash: None,
        }
    }

    /// Register a trailing flow window (idempotent; deduped by `window_size_sec`).
    /// The engine calls this for every distinct `window_size_sec` its loaded
    /// rules reference before the token starts receiving trades.
    pub fn ensure_window(&mut self, window_secs: f64) {
        self.windows
            .entry(window_key(window_secs))
            .or_insert_with(|| WindowState::new(window_secs));
    }

    /// Register a trailing price-extrema window (idempotent; deduped by
    /// `window_size_sec`). The `m_price_window` counterpart of [`ensure_window`].
    pub fn ensure_price_window(&mut self, window_secs: f64) {
        self.price_windows
            .entry(window_key(window_secs))
            .or_insert_with(|| PriceWindowState::new(window_secs));
    }

    /// Register fingerprint-scoped flow state (idempotent). `windows` are the
    /// distinct `m_flow_split_window` sizes to track under this fingerprint.
    pub fn ensure_flow(
        &mut self,
        fp: FingerprintId,
        patterns: &FlowPatterns,
        windows: &[f64],
    ) {
        let state = self.flow.entry(fp).or_insert_with(|| {
            let mut s = FlowState::new(patterns.clone());
            if let Some(h) = self.creator_wallet_hash {
                s.set_creator(h);
            }
            s
        });
        for &w in windows {
            state.ensure_window(w);
        }
    }

    /// Seed the creator wallet (volume-side unconditionally) on every flow state.
    pub fn seed_creator(&mut self, hash: u64) {
        self.creator_wallet_hash = Some(hash);
        for flow in self.flow.values_mut() {
            flow.set_creator(hash);
        }
    }

    /// Fold one trade into every group.
    pub fn on_trade(&mut self, t: TradeLite) {
        self.snapshot.on_trade(t.reserve_sol);
        self.price_lifetime.on_trade(t.price, t.at);
        self.flow_lifetime.on_trade(t.side, t.sol);
        for w in self.windows.values_mut() {
            w.on_trade(t.side, t.sol, t.at);
        }
        for pw in self.price_windows.values_mut() {
            pw.on_trade(t.price, t.at);
        }
        for flow in self.flow.values_mut() {
            flow.on_trade(&t);
        }
    }

    /// Advance time to `now` without a trade — evicts stale window entries so a
    /// quiet token's flows decay (and the stall/time metrics read against `now`
    /// at the call site).
    pub fn on_tick(&mut self, now: Ts) {
        for w in self.windows.values_mut() {
            w.evict(now);
        }
        for pw in self.price_windows.values_mut() {
            pw.evict(now);
        }
        for flow in self.flow.values_mut() {
            flow.on_tick(now);
        }
    }

    /// The most recently observed canonical price (`NaN` before the first trade).
    /// The "last known price" a tick-driven TP/SL check reads (a tick carries no
    /// trade, so the position is marked against the last print).
    pub fn current_price(&self) -> f64 {
        self.price_lifetime.last_price()
    }

    /// The most recently observed SOL reserves (`NaN` before the first trade) — the
    /// liquidity reading the dead-token verdict consumes.
    pub fn current_reserves(&self) -> f64 {
        self.value(MetricId::Liquidity, None, None, self.created_at)
    }

    /// Value of one metric at `now`. `window_secs` is required for dynamic
    /// metrics and ignored for static ones. `fingerprint` is required for flow
    /// groups (absent / unregistered ⇒ `NaN`). An unregistered window yields
    /// `NaN` — which satisfies no condition.
    pub fn value(
        &self,
        id: MetricId,
        window_secs: Option<f64>,
        fingerprint: Option<FingerprintId>,
        now: Ts,
    ) -> f64 {
        use MetricId::*;
        match id {
            Time | Liquidity => self.snapshot.value(id, self.created_at, now),
            Stall | Trail | LifeRise => self.price_lifetime.value(id, now),
            LifeGrossFlow | LifeNetFlow | LifeBuy | LifeSell => self.flow_lifetime.value(id),
            WinTrail | WinRise => {
                match window_secs.and_then(|ws| self.price_windows.get(&window_key(ws))) {
                    Some(pw) => pw.value(id),
                    None => f64::NAN,
                }
            }
            GrossFlow | NetFlow | Buy | Sell => {
                match window_secs.and_then(|ws| self.windows.get(&window_key(ws))) {
                    Some(w) => w.value(id),
                    None => f64::NAN,
                }
            }
            VolBuy | VolSell | VolNet | VolGross | NonvolBuy | NonvolSell | NonvolNet
            | NonvolGross | VolShare | WinVolBuy | WinVolSell | WinVolNet | WinVolGross
            | WinNonvolBuy | WinNonvolSell | WinNonvolNet | WinNonvolGross | WinVolShare => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                match self.flow.get(&fp) {
                    Some(f) => f.value(id, window_secs),
                    None => f64::NAN,
                }
            }
            // Position-scoped metrics have no token state — they read from a
            // `PositionCtx` (see `metrics::position`), never the track. Before entry
            // (the only place `TokenTrack::value` reaches them, via the `can_enter`
            // exit-gate) they read NaN, so a position exit metric never blocks entry.
            Retrace | Bounce | Pnl | Held => f64::NAN,
        }
    }

    /// Batch read a set of `(metric, optional window, optional fingerprint)`
    /// requests into a caller-owned buffer — no allocation on the hot path.
    /// `out` must be at least `reqs.len()` long; extra slots are left untouched.
    pub fn values(
        &self,
        reqs: &[(MetricId, Option<f64>, Option<FingerprintId>)],
        now: Ts,
        out: &mut [f64],
    ) {
        for (slot, &(id, ws, fp)) in out.iter_mut().zip(reqs) {
            *slot = self.value(id, ws, fp, now);
        }
    }

    /// Whether this track has flow state for `fp` (configured fingerprint).
    pub fn has_flow(&self, fp: FingerprintId) -> bool {
        self.flow.contains_key(&fp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Side;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn buy(sol: f64, price: f64, reserve: f64, secs: f64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol,
            price,
            reserve_sol: reserve,
            at: ts(secs),
            ..Default::default()
        }
    }

    fn fp(n: u128) -> FingerprintId {
        FingerprintId(Uuid::from_u128(n))
    }

    #[test]
    fn routes_each_metric_to_its_group() {
        let created = ts(0.0);
        let mut track = TokenTrack::new(created);
        track.ensure_window(10.0);
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));

        assert_eq!(track.value(MetricId::Time, None, None, ts(5.0)), 5.0);
        assert_eq!(track.value(MetricId::Liquidity, None, None, ts(5.0)), 15.0);
        assert_eq!(track.value(MetricId::Stall, None, None, ts(5.0)), 4.0); // moved at t=1
        assert_eq!(track.value(MetricId::Trail, None, None, ts(5.0)), 0.0); // at peak
        assert_eq!(track.value(MetricId::LifeRise, None, None, ts(5.0)), 0.0); // at trough
        assert_eq!(track.value(MetricId::Buy, Some(10.0), None, ts(5.0)), 3.0);
        assert_eq!(track.value(MetricId::GrossFlow, Some(10.0), None, ts(5.0)), 3.0);
        // Lifetime totals ignore the window arg and do not need ensure_window.
        assert_eq!(track.value(MetricId::LifeBuy, None, None, ts(5.0)), 3.0);
        assert_eq!(track.value(MetricId::LifeGrossFlow, None, None, ts(5.0)), 3.0);
    }

    #[test]
    fn lifetime_flow_survives_window_decay() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), None, ts(5.0)), 4.0);
        assert_eq!(track.value(MetricId::LifeBuy, None, None, ts(5.0)), 4.0);
        // Tick past the window edge → window decays; lifetime keeps the total.
        track.on_tick(ts(11.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), None, ts(11.0)), 0.0);
        assert_eq!(track.value(MetricId::LifeBuy, None, None, ts(11.0)), 4.0);
        assert_eq!(track.value(MetricId::LifeGrossFlow, None, None, ts(11.0)), 4.0);
    }

    #[test]
    fn unregistered_or_missing_window_is_nan() {
        let mut track = TokenTrack::new(ts(0.0));
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        // No window ensured → NaN.
        assert!(track.value(MetricId::Buy, Some(10.0), None, ts(5.0)).is_nan());
        // Dynamic metric with no window arg → NaN.
        track.ensure_window(10.0);
        assert!(track.value(MetricId::Buy, None, None, ts(5.0)).is_nan());
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
    fn routes_price_window_and_dedupes_independently_of_flow_windows() {
        let created = ts(0.0);
        let mut track = TokenTrack::new(created);
        // A flow window and a price window at the SAME size must be distinct buffers.
        track.ensure_window(30.0);
        track.ensure_price_window(30.0);
        track.ensure_price_window(30.0); // idempotent
        assert_eq!(track.price_windows.len(), 1);
        assert_eq!(track.windows.len(), 1);

        // Unregistered price window / missing window arg → NaN.
        assert!(track.value(MetricId::WinTrail, Some(60.0), None, ts(1.0)).is_nan());
        assert!(track.value(MetricId::WinTrail, None, None, ts(1.0)).is_nan());
        // Before any trade → NaN.
        assert!(track.value(MetricId::WinTrail, Some(30.0), None, ts(1.0)).is_nan());

        // High at t=1 (price 2.0), dip at t=2 (price 1.5) → trail = 25%, rise = 0.
        track.on_trade(buy(1.0, 2.0, 15.0, 1.0));
        track.on_trade(buy(1.0, 1.5, 15.0, 2.0));
        assert!(
            (track.value(MetricId::WinTrail, Some(30.0), None, ts(2.0)) - 25.0).abs() < 1e-9
        );
        assert_eq!(track.value(MetricId::WinRise, Some(30.0), None, ts(2.0)), 0.0);
    }

    #[test]
    fn tick_decays_quiet_flows() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), None, ts(5.0)), 4.0);
        // Tick past the window edge → flow decays to zero even with no trade.
        track.on_tick(ts(11.0));
        assert_eq!(track.value(MetricId::Buy, Some(10.0), None, ts(11.0)), 0.0);
    }

    #[test]
    fn values_batch_fills_caller_buffer() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(10.0);
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        let reqs = [
            (MetricId::Time, None, None),
            (MetricId::Liquidity, None, None),
            (MetricId::Buy, Some(10.0), None),
        ];
        let mut out = [0.0_f64; 3];
        track.values(&reqs, ts(5.0), &mut out);
        assert_eq!(out, [5.0, 15.0, 3.0]);
    }

    #[test]
    fn flow_unconfigured_is_nan_configured_splits() {
        use crate::metrics::flow_split::ix_hash;
        use std::collections::BTreeSet;

        let mut track = TokenTrack::new(ts(0.0));
        let id = fp(1);
        // No ensure_flow → NaN.
        assert!(track
            .value(MetricId::VolBuy, None, Some(id), ts(1.0))
            .is_nan());

        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        track.ensure_flow(id, &patterns, &[10.0]);
        track.seed_creator(99);

        let mut t = buy(4.0, 1.0, 20.0, 0.0);
        t.ix_hash = Some(ix_hash(&["vol"]));
        t.wallet_hash = 1;
        track.on_trade(t);

        let mut t2 = buy(6.0, 1.0, 26.0, 1.0);
        t2.wallet_hash = 2;
        track.on_trade(t2);

        assert_eq!(track.value(MetricId::VolBuy, None, Some(id), ts(2.0)), 4.0);
        assert_eq!(track.value(MetricId::NonvolBuy, None, Some(id), ts(2.0)), 6.0);
        assert_eq!(
            track.value(MetricId::WinVolBuy, Some(10.0), Some(id), ts(2.0)),
            4.0
        );
        // Missing fingerprint arg → NaN even when state exists.
        assert!(crate::metrics::is_flow_metric(MetricId::VolBuy));
        assert!(track.value(MetricId::VolBuy, None, None, ts(2.0)).is_nan());
    }
}
