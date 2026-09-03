//! `TokenTrack` — the per-token metric state the engine folds trades and ticks
//! into. It composes the group states:
//! * static [`StateMetrics`] + [`PriceLifetimeState`] + [`FlowLifetimeState`] —
//!   one copy each, shared by every rule armed on the token;
//! * dynamic [`WindowState`]s — **deduped by `window_size_sec`**, so rules that
//!   share a window share one buffer;
//! * fingerprint-scoped [`FlowState`]s — one classifier + totals per fingerprint
//!   that has `m_flow_ix` configured (volume-flow-split plan).
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

use super::burst_slot::{BurstPatterns, BurstSlotState};
use super::burst_wave::BurstWaveState;
use super::copy::{CopyPatterns, CopyState};
use super::crowd_window::CrowdWindowState;
use super::flow_lifetime::FlowLifetimeState;
use super::dump_ix::{DumpPatterns, DumpState};
use super::flow_ix::{FlowPatterns, FlowState};
use super::flow_window::WindowState;
use super::price_lifetime::PriceLifetimeState;
use super::price_window::PriceWindowState;
use super::state::StateMetrics;
use super::{Cursor, MetricId, TradeLite, Ts};

/// All metric state for one token.
#[derive(Debug, Clone)]
pub struct TokenTrack {
    created_at: Ts,
    /// Highest slot observed on this token - the cursor every slot-unit window
    /// counts in. Monotonic; a tick advances it only when the producer supplies one.
    cur_slot: u64,
    /// How many prints this token has taken - the cursor every print-unit window
    /// counts in. Bumped once per folded trade, BEFORE the fold, so the trade being
    /// folded sits at the cursor and a `size: 1, lag: 0` window is that trade alone.
    /// A tick never moves it: silence is not a print, which is exactly why a print
    /// window reads the same tape whether the token is busy or dead.
    n_prints: u64,
    state: StateMetrics,
    price_lifetime: PriceLifetimeState,
    flow_lifetime: FlowLifetimeState,
    /// Dynamic flow windows, keyed by [`window_key`] so equal sizes dedupe.
    windows: BTreeMap<super::WindowKey, WindowState>,
    /// Dynamic price-extrema windows, keyed by [`window_key`]. Kept apart from
    /// `windows` so a rule using only `m_flow_window` pays for no price deque (and
    /// vice versa) — the dynamic groups share the window size param names but not the
    /// buffers.
    price_windows: BTreeMap<super::WindowKey, PriceWindowState>,
    /// Dynamic wallet windows (`m_crowd_window`), keyed by [`window_key`]. Apart from
    /// `windows` for the same reason `price_windows` is: this group's obligation is the
    /// WALLET column, and a rule that reads no crowd metric must not pay a second deque
    /// push and a hash-map entry on every trade of every window to carry it.
    crowd_windows: BTreeMap<super::WindowKey, CrowdWindowState>,
    /// Flow classifier state, keyed by fingerprint (pattern sets differ).
    flow: BTreeMap<FingerprintId, FlowState>,
    /// `m_dump_ix` state, keyed by fingerprint. Apart from `flow` for the reason
    /// `crowd_windows` is apart from `windows`: it reads a DIFFERENT list and stores
    /// nothing at all for a trade that does not match one, so a rule reading no dump
    /// metric must not pay a deque push per trade to carry it.
    dump: BTreeMap<FingerprintId, DumpState>,
    /// `m_burst_slot` working lists, keyed by fingerprint. The slot prefix itself
    /// is one buffer on the token ([`burst`](Self::burst)); only the 0/1 working
    /// match differs per list.
    burst_patterns: BTreeMap<FingerprintId, BurstPatterns>,
    burst: BurstSlotState,
    /// `m_copy` / `m_copy_window` state, keyed by fingerprint. Apart from `flow` for
    /// the reason `dump` is: it reads a DIFFERENT list (wallets, not builds) and
    /// stores nothing at all for a print the target did not sign, so a rule reading
    /// no copy metric must not pay a deque push per trade to carry it.
    copy: BTreeMap<FingerprintId, CopyState>,
    /// Consecutive-slot buy wave. Token-level, always folded — not fingerprint-scoped.
    burst_wave: BurstWaveState,
    /// Creator wallet hash from `TokenCreated` — applied to every FlowState.
    creator_wallet_hash: Option<u64>,
    /// Last observed *priced* SOL depth (`vsol`), for price impact only. Not a
    /// metric: no rule reads it, and it is deliberately NOT the `liquidity` reading.
    /// See [`TradeLite::priced_reserve_sol`].
    priced_reserves: f64,
}

impl TokenTrack {
    /// Fresh state for a token created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self {
            created_at,
            state: StateMetrics::default(),
            price_lifetime: PriceLifetimeState::new(created_at),
            flow_lifetime: FlowLifetimeState::default(),
            windows: BTreeMap::new(),
            price_windows: BTreeMap::new(),
            crowd_windows: BTreeMap::new(),
            cur_slot: 0,
            n_prints: 0,
            priced_reserves: f64::NAN,
            flow: BTreeMap::new(),
            dump: BTreeMap::new(),
            burst_patterns: BTreeMap::new(),
            burst: BurstSlotState::default(),
            burst_wave: BurstWaveState::default(),
            copy: BTreeMap::new(),
            creator_wallet_hash: None,
        }
    }

    /// Register a trailing flow window (idempotent; deduped by `window_size_sec`).
    /// The engine calls this for every distinct `window_size_sec` its loaded
    /// rules reference before the token starts receiving trades.
    pub fn ensure_window(&mut self, spec: super::WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| WindowState::new(spec));
    }

    /// Register a trailing price-extrema window (idempotent; deduped by the whole
    /// span). The `m_price_window` counterpart of [`ensure_window`].
    pub fn ensure_price_window(&mut self, spec: super::WindowSpec) {
        self.price_windows.entry(spec.key()).or_insert_with(|| PriceWindowState::new(spec));
    }

    /// Register a trailing wallet window (idempotent; deduped by the whole span). The
    /// `m_crowd_window` counterpart of [`ensure_window`] — a separate call because it
    /// opens a separate buffer, so a rule reading no crowd metric never pays for one.
    pub fn ensure_crowd_window(&mut self, spec: super::WindowSpec) {
        self.crowd_windows.entry(spec.key()).or_insert_with(|| CrowdWindowState::new(spec));
    }

    /// Register fingerprint-scoped flow state (idempotent). `windows` are the
    /// distinct `m_flow_ix_window` sizes to track under this fingerprint.
    /// Re-registering an existing fingerprint **adopts the new pattern set**. A
    /// reload happens because the operator edited `ix_patterns`, and keeping
    /// the compiled-at-first-sight set would leave every token the engine is already
    /// tracking classifying against the pre-edit list for the rest of its life — an
    /// edit that visibly takes effect on new tokens and silently does nothing on the
    /// ones on screen. Trades already folded keep the classification they were folded
    /// under (the totals are running sums; nothing retains the trades to redo), so an
    /// edit moves the *future* of a live token, never its past.
    pub fn ensure_flow(
        &mut self,
        fp: FingerprintId,
        patterns: &FlowPatterns,
        windows: &[super::WindowSpec],
    ) {
        let creator = self.creator_wallet_hash;
        let state = self.flow.entry(fp).or_insert_with(|| {
            let mut s = FlowState::new(patterns.clone());
            if let Some(h) = creator {
                s.set_creator(h);
            }
            s
        });
        state.set_patterns(patterns);
        for &w in windows {
            state.ensure_window(w);
        }
    }

    /// Register fingerprint-scoped dump state (idempotent). Same adopt-on-reload
    /// contract as [`ensure_flow`](Self::ensure_flow): an edited build list moves a
    /// live token's future, never its past. No creator seed — this group has no
    /// wallet rule to seed.
    pub fn ensure_dump(
        &mut self,
        fp: FingerprintId,
        patterns: &DumpPatterns,
        windows: &[super::WindowSpec],
    ) {
        let state = self.dump.entry(fp).or_insert_with(|| DumpState::new(patterns.clone()));
        state.set_patterns(patterns);
        for &w in windows {
            state.ensure_window(w);
        }
    }

    /// Register fingerprint-scoped burst patterns (idempotent). Same adopt-on-reload
    /// contract as [`ensure_dump`](Self::ensure_dump): an edited working list moves
    /// a live token's future, never its past. The slot prefix is token-level and
    /// does not reset on reload.
    pub fn ensure_burst(&mut self, fp: FingerprintId, patterns: &BurstPatterns) {
        self.burst_patterns.insert(fp, patterns.clone());
    }

    /// Register fingerprint-scoped copy state (idempotent). Same adopt-on-reload
    /// contract as [`ensure_dump`](Self::ensure_dump): swapping the target wallet
    /// moves a live token's future, never its past. No creator seed — this group's
    /// only wallet rule is the list itself.
    pub fn ensure_copy(
        &mut self,
        fp: FingerprintId,
        patterns: &CopyPatterns,
        windows: &[super::WindowSpec],
    ) {
        let state = self.copy.entry(fp).or_insert_with(|| CopyState::new(patterns.clone()));
        state.set_patterns(patterns);
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

    /// Seed the mint's create slot so that slot is not a fireable wave.
    pub fn seed_creation_slot(&mut self, slot: u64) {
        self.burst_wave.seed_creation_slot(slot);
    }

    /// Fold one trade into every group.
    pub fn on_trade(&mut self, t: TradeLite) {
        // Burst reads trail and previous-slot liquidity BEFORE this print is folded.
        let pre_trail = self.price_lifetime.trail();
        let prev_liq = self.state.liquidity();
        if !self.burst_patterns.is_empty() {
            self.burst.on_trade(&t, pre_trail, prev_liq);
        }
        self.burst_wave.on_trade(&t);
        self.state.on_trade(t.reserve_sol);
        self.priced_reserves = t.priced_reserve_sol;
        // The slot cursor only ever moves forward. Canonical order is
        // slot -> tx_index -> leg, so a regressed feed row must not rewind every
        // slot window on the token.
        self.cur_slot = self.cur_slot.max(t.slot);
        // The print cursor is a fold counter, not a feed field: it advances once per
        // trade in the order trades arrive, which is the canonical order above. Bumped
        // BEFORE the fold so this trade sits AT the cursor.
        self.n_prints += 1;
        self.price_lifetime.on_trade(t.price, t.at);
        self.flow_lifetime.on_trade(t.side, t.sol);
        let cur = self.cursor();
        let at = cur.at_trade(&t);
        for w in self.windows.values_mut() {
            let spec = w.spec();
            w.on_trade(t.side, t.sol, spec.pos(t.at, at), spec.now_pos(t.at, cur));
        }
        for cw in self.crowd_windows.values_mut() {
            let spec = cw.spec();
            cw.on_trade(t.sol, t.wallet_hash, spec.pos(t.at, at), spec.now_pos(t.at, cur));
        }
        for pw in self.price_windows.values_mut() {
            let spec = pw.spec();
            pw.on_trade(t.price, spec.pos(t.at, at), spec.now_pos(t.at, cur));
        }
        for flow in self.flow.values_mut() {
            flow.on_trade(&t, cur);
        }
        for dump in self.dump.values_mut() {
            dump.on_trade(&t, cur);
        }
        for copy in self.copy.values_mut() {
            copy.on_trade(&t, cur);
        }
    }

    /// Advance time to `now` without a trade — evicts stale window entries so a
    /// quiet token's flows decay (and the stall/time metrics read against `now`
    /// at the call site).
    pub fn on_tick(&mut self, now: Ts, slot: Option<u64>) {
        // A slot axis has no clock of its own: it advances only when something tells
        // it a slot passed. When the producer cannot supply one, slot windows HOLD
        // their last reading rather than guess from elapsed time - slot durations
        // vary, and an estimated cursor is a silently wrong number.
        if let Some(sl) = slot {
            self.cur_slot = self.cur_slot.max(sl);
        }
        // `n_prints` is deliberately untouched: a tick is not a print, so a print
        // window evicts nothing here and HOLDS its whole content through any silence.
        let cur = self.cursor();
        for w in self.windows.values_mut() {
            let now_pos = w.spec().now_pos(now, cur);
            w.evict(now_pos);
        }
        for pw in self.price_windows.values_mut() {
            let now_pos = pw.spec().now_pos(now, cur);
            pw.evict(now_pos);
        }
        for cw in self.crowd_windows.values_mut() {
            let now_pos = cw.spec().now_pos(now, cur);
            cw.evict(now_pos);
        }
        for flow in self.flow.values_mut() {
            flow.on_tick(now, cur);
        }
        for dump in self.dump.values_mut() {
            dump.on_tick(now, cur);
        }
        for copy in self.copy.values_mut() {
            copy.on_tick(now, cur);
        }
        if !self.burst_patterns.is_empty() {
            self.burst.on_tick();
        }
        self.burst_wave.on_tick();
    }

    /// How many matched sells one fingerprint's dump window retains — see
    /// [`DumpState::window_len`].
    #[cfg(test)]
    pub(crate) fn dump_window_len(
        &self,
        fp: FingerprintId,
        spec: super::WindowSpec,
    ) -> Option<usize> {
        self.dump.get(&fp).and_then(|d| d.window_len(spec))
    }

    /// Where the token stands on every discrete window axis right now.
    fn cursor(&self) -> Cursor {
        Cursor { slot: self.cur_slot, print: self.n_prints }
    }

    /// The highest slot this token has observed. `0` before the first trade, or when
    /// no adapter supplies slots.
    pub fn cur_slot(&self) -> u64 {
        self.cur_slot
    }

    /// How many prints this token has taken. `0` before the first trade.
    pub fn n_prints(&self) -> u64 {
        self.n_prints
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
        self.value(MetricId::Liquidity, crate::metrics::Windows::NONE, None, self.created_at)
    }

    /// The most recently observed **priced** SOL depth (`vsol`; `NaN` before the first
    /// trade or when the adapter did not carry it) — the basis price impact is charged
    /// against, and NOT the `liquidity` reading. See [`TradeLite::priced_reserve_sol`].
    pub fn current_priced_reserves(&self) -> f64 {
        self.priced_reserves
    }

    /// Value of one metric at `now`. `window_secs` is required for dynamic
    /// metrics and ignored for static ones. `fingerprint` is required for flow
    /// groups (absent / unregistered ⇒ `NaN`). An unregistered window yields
    /// `NaN` — which satisfies no condition.
    pub fn value(
        &self,
        id: MetricId,
        windows: crate::metrics::Windows,
        fingerprint: Option<FingerprintId>,
        now: Ts,
    ) -> f64 {
        use MetricId::*;
        let window = windows.primary;
        let cur = self.cursor();
        match id {
            Time | Liquidity => self.state.value(id, self.created_at, now),
            Stall | Trail | LifeRise => self.price_lifetime.value(id, now),
            LifeGrossFlow | LifeNetFlow | LifeBuy | LifeSell | LifeTradeCount => {
                self.flow_lifetime.value(id)
            }
            WinTrail | WinRise => {
                match window.and_then(|sp| self.price_windows.get(&sp.key()).map(|pw| (sp, pw))) {
                    Some((sp, pw)) => pw.value(id, sp.now_pos(now, cur)),
                    None => f64::NAN,
                }
            }
            GrossFlow | NetFlow | Buy | Sell | TradeCount | BuyCount | SellCount | BuyShare => {
                match window.and_then(|sp| self.windows.get(&sp.key()).map(|w| (sp, w))) {
                    Some((sp, w)) => w.value(id, sp.now_pos(now, cur)),
                    None => f64::NAN,
                }
            }
            // `m_crowd_window` reads its OWN deque — the wallet column is its subject,
            // not `m_flow_window`'s payload.
            UniqueWallets | TradesPerWallet => {
                match window.and_then(|sp| self.crowd_windows.get(&sp.key()).map(|w| (sp, w))) {
                    Some((sp, w)) => w.value(id, sp.now_pos(now, cur)),
                    None => f64::NAN,
                }
            }
            // The TWO-window reads: `primary` is the reference span, `secondary` the
            // slice nested inside it. Both buffers are `m_flow_window`'s own —
            // `CompiledRule` registers each axis, so this allocates nothing. Either
            // axis unregistered ⇒ NaN, the same "no reading" a missing single window
            // gives, and NaN satisfies no condition.
            //
            // `build_reqs` sets `secondary` only for the metrics [`is_two_window`]
            // selects, so a single-window read can never arrive here with a slice
            // attached, nor one of these without one.
            //
            // [`is_two_window`]: super::is_two_window
            SliceTradeShare | SliceSolShare => {
                let reference = windows.primary.and_then(|w| self.windows.get(&w.key()));
                let slice = windows.secondary.and_then(|b| self.windows.get(&b.key()));
                match (slice, reference, windows.primary, windows.secondary) {
                    (Some(b), Some(r), Some(rs), Some(bs)) => {
                        let (bn, rn) = (bs.now_pos(now, cur), rs.now_pos(now, cur));
                        if id == SliceTradeShare {
                            super::flow_slice::trade_share(b, r, bn, rn)
                        } else {
                            super::flow_slice::sol_share(b, r, bn, rn)
                        }
                    }
                    _ => f64::NAN,
                }
            }
            TaggedBuy | TaggedSell | TaggedNet | TaggedGross | UntaggedBuy | UntaggedSell | UntaggedNet
            | UntaggedGross | TaggedShare | TaggedBuyCount | TaggedSellCount | WinTaggedBuy
            | WinTaggedSell | WinTaggedNet | WinTaggedGross | WinUntaggedBuy | WinUntaggedSell
            | WinUntaggedNet | WinUntaggedGross | WinTaggedShare | WinTaggedBuyCount
            | WinTaggedSellCount => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                match self.flow.get(&fp) {
                    Some(f) => f.value(id, window, now, cur),
                    None => f64::NAN,
                }
            }
            CopyBuySol | CopyBuyCount | CopySellSol | CopySellCount | WinCopyBuySol
            | WinCopyBuyCount | WinCopySellSol | WinCopySellCount => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                match self.copy.get(&fp) {
                    Some(c) => c.value(id, window, now, cur),
                    None => f64::NAN,
                }
            }
            DumpSell | DumpSellCount | WinDumpSell | WinDumpSellCount => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                match self.dump.get(&fp) {
                    Some(d) => d.value(id, window, now, cur),
                    None => f64::NAN,
                }
            }
            ThisMember | ThisWorking | SameBuyCount | SameBuySol | SameWalletCount
            | MemberTemplateCount | WorkingBuyCount | WorkingBuySol | WorkingWalletCount | WorkingTemplateCount
            | WorkingTemplatesSeen
            | WorkingBuyShare | HasNew | HasUnknown | Packed
            | PreSlotLiquidity | PrePrintTrail => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                self.burst.value(id, self.burst_patterns.get(&fp))
            }
            WaveThisMember | WaveWalletCount | WaveBuySol | WaveGapSlots | WaveAllNew
            | WaveHasUnknown | WaveThisTip | WaveHole | WaveTipSeen => {
                self.burst_wave.value(id, None)
            }
            WaveWorkingBuyCount | WaveThisWorking => {
                let Some(fp) = fingerprint else {
                    return f64::NAN;
                };
                self.burst_wave.value(id, self.burst_patterns.get(&fp))
            }
            // Position-scoped metrics have no token state — they read from a
            // `PositionCtx` (see `metrics::position`), never the track. Before entry
            // (the only place `TokenTrack::value` reaches them, via the `can_enter`
            // exit-gate) they read NaN, so a position exit metric never blocks entry.
            Retrace | Bounce | Pnl | Held | Armed => f64::NAN,
        }
    }

    /// Batch read a set of `(metric, optional window, optional fingerprint)`
    /// requests into a caller-owned buffer — no allocation on the hot path.
    /// `out` must be at least `reqs.len()` long; extra slots are left untouched.
    pub fn values(
        &self,
        reqs: &[(MetricId, Option<super::WindowSpec>, Option<FingerprintId>)],
        now: Ts,
        out: &mut [f64],
    ) {
        for (slot, &(id, ws, fp)) in out.iter_mut().zip(reqs) {
            *slot = self.value(id, ws.into(), fp, now);
        }
    }

    /// Whether this track has flow state for `fp` (configured fingerprint).
    pub fn has_flow(&self, fp: FingerprintId) -> bool {
        self.flow.contains_key(&fp)
    }
}

#[cfg(test)]
mod tests {
    use crate::metrics::WindowSpec;
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

    /// A quiet token's dump window must DECAY, exactly as its `m_flow_ix` sibling
    /// does. `DumpState::on_tick` existed but `TokenTrack::on_tick` never called it,
    /// so the buffer was only ever trimmed by the next matching sell — on a token
    /// nobody dumps on again, that is never.
    #[test]
    fn a_dump_window_decays_on_a_tick_like_its_flow_sibling() {
        use crate::metrics::flow_ix::ix_hash;
        use std::collections::BTreeSet;
        let id = fp(1);
        let build = ix_hash(&["Pump.Fun: Sell", "Token Program: CloseAccount"]);
        let w = crate::metrics::WindowSpec::secs(10.0);
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_dump(id, &DumpPatterns::new(BTreeSet::from([build])), &[w]);

        track.on_trade(TradeLite {
            side: crate::metrics::Side::Sell,
            sol: 2.0,
            price: 1.0,
            at: ts(1.0),
            ix_hash: Some(build),
            ..Default::default()
        });
        let read = |t: &TokenTrack, at: f64| {
            t.value(MetricId::WinDumpSell, crate::metrics::Windows::secs(10.0), Some(id), ts(at))
        };
        assert_eq!(read(&track, 2.0), 2.0, "inside the window");
        assert_eq!(track.dump_window_len(id, w), Some(1));

        // Ticking past the span must leave nothing behind. The READ was always
        // right (both window ends are corrected on read), so the retained deque is
        // the only thing that shows whether the tick reached this group at all.
        track.on_tick(ts(20.0), None);
        assert_eq!(read(&track, 20.0), 0.0, "the window released");
        assert_eq!(track.dump_window_len(id, w), Some(0), "and the tick evicted it");
    }

    /// A pattern edit must reach a token the engine is ALREADY tracking. Before this,
    /// `ensure_flow` kept the set compiled at first sight, so an operator adding a
    /// missing volume pattern saw it work on new tokens and do nothing on the open
    /// ones — the exact shape that makes an exit look unexplainable.
    #[test]
    fn reload_adopts_an_edited_pattern_set_on_a_live_token() {
        use crate::metrics::flow_ix::ix_hash;
        use std::collections::BTreeSet;
        let fp = FingerprintId(Uuid::nil());
        let bot = ix_hash(&["bot", "buy"]);
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_flow(fp, &FlowPatterns::default(), &[]);

        // Unconfigured: the bot buy reads as organic.
        track.on_trade(TradeLite {
            slot: 0,
            marker_bits: 0,
            side: Side::Buy,
            sol: 1.0,
            price: 1.0,
            reserve_sol: 10.0,
            priced_reserve_sol: 10.0,
            at: ts(1.0),
            ix_hash: Some(bot),
            wallet_hash: 11,
            leg_index: 0,
            ..Default::default()
        });
        assert_eq!(track.value(MetricId::UntaggedBuy, crate::metrics::Windows::NONE, Some(fp), ts(1.0)), 1.0);

        // Operator adds the pattern; the reload re-registers the same fingerprint.
        track.ensure_flow(fp, &FlowPatterns::new(BTreeSet::from([bot])), &[]);
        track.on_trade(TradeLite {
            slot: 0,
            marker_bits: 0,
            side: Side::Buy,
            sol: 2.0,
            price: 1.0,
            reserve_sol: 10.0,
            priced_reserve_sol: 10.0,
            at: ts(2.0),
            ix_hash: Some(bot),
            wallet_hash: 12,
            leg_index: 0,
            ..Default::default()
        });
        // The new trade classifies volume-side; the already-folded one keeps its past.
        assert_eq!(track.value(MetricId::TaggedBuy, crate::metrics::Windows::NONE, Some(fp), ts(2.0)), 2.0);
        assert_eq!(track.value(MetricId::UntaggedBuy, crate::metrics::Windows::NONE, Some(fp), ts(2.0)), 1.0);
    }

    #[test]
    fn routes_each_metric_to_its_group() {
        let created = ts(0.0);
        let mut track = TokenTrack::new(created);
        track.ensure_window(WindowSpec::secs(10.0));
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));

        assert_eq!(track.value(MetricId::Time, crate::metrics::Windows::NONE, None, ts(5.0)), 5.0);
        assert_eq!(track.value(MetricId::Liquidity, crate::metrics::Windows::NONE, None, ts(5.0)), 15.0);
        assert_eq!(track.value(MetricId::Stall, crate::metrics::Windows::NONE, None, ts(5.0)), 4.0); // moved at t=1
        assert_eq!(track.value(MetricId::Trail, crate::metrics::Windows::NONE, None, ts(5.0)), 0.0); // at peak
        assert_eq!(track.value(MetricId::LifeRise, crate::metrics::Windows::NONE, None, ts(5.0)), 0.0); // at trough
        assert_eq!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(5.0)), 3.0);
        assert_eq!(track.value(MetricId::GrossFlow, crate::metrics::Windows::secs(10.0), None, ts(5.0)), 3.0);
        // Lifetime totals ignore the window arg and do not need ensure_window.
        assert_eq!(track.value(MetricId::LifeBuy, crate::metrics::Windows::NONE, None, ts(5.0)), 3.0);
        assert_eq!(track.value(MetricId::LifeGrossFlow, crate::metrics::Windows::NONE, None, ts(5.0)), 3.0);
    }

    #[test]
    fn lifetime_flow_survives_window_decay() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::secs(10.0));
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
        assert_eq!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(5.0)), 4.0);
        assert_eq!(track.value(MetricId::LifeBuy, crate::metrics::Windows::NONE, None, ts(5.0)), 4.0);
        // Tick past the window edge → window decays; lifetime keeps the total.
        track.on_tick(ts(11.0), None);
        assert_eq!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(11.0)), 0.0);
        assert_eq!(track.value(MetricId::LifeBuy, crate::metrics::Windows::NONE, None, ts(11.0)), 4.0);
        assert_eq!(track.value(MetricId::LifeGrossFlow, crate::metrics::Windows::NONE, None, ts(11.0)), 4.0);
    }

    #[test]
    fn unregistered_or_missing_window_is_nan() {
        let mut track = TokenTrack::new(ts(0.0));
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        // No window ensured → NaN.
        assert!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(5.0)).is_nan());
        // Dynamic metric with no window arg → NaN.
        track.ensure_window(WindowSpec::secs(10.0));
        assert!(track.value(MetricId::Buy, crate::metrics::Windows::NONE, None, ts(5.0)).is_nan());
    }

    #[test]
    fn equal_windows_dedupe_to_one_buffer() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::secs(10.0));
        track.ensure_window(WindowSpec::secs(10.0));
        assert_eq!(track.windows.len(), 1);
        track.ensure_window(WindowSpec::secs(5.0));
        assert_eq!(track.windows.len(), 2);
    }

    #[test]
    fn routes_price_window_and_dedupes_independently_of_flow_windows() {
        let created = ts(0.0);
        let mut track = TokenTrack::new(created);
        // A flow window and a price window at the SAME size must be distinct buffers.
        track.ensure_window(WindowSpec::secs(30.0));
        track.ensure_price_window(WindowSpec::secs(30.0));
        track.ensure_price_window(WindowSpec::secs(30.0)); // idempotent
        assert_eq!(track.price_windows.len(), 1);
        assert_eq!(track.windows.len(), 1);

        // Unregistered price window / missing window arg → NaN.
        assert!(track.value(MetricId::WinTrail, crate::metrics::Windows::secs(60.0), None, ts(1.0)).is_nan());
        assert!(track.value(MetricId::WinTrail, crate::metrics::Windows::NONE, None, ts(1.0)).is_nan());
        // Before any trade → NaN.
        assert!(track.value(MetricId::WinTrail, crate::metrics::Windows::secs(30.0), None, ts(1.0)).is_nan());

        // High at t=1 (price 2.0), dip at t=2 (price 1.5) → trail = 25%, rise = 0.
        track.on_trade(buy(1.0, 2.0, 15.0, 1.0));
        track.on_trade(buy(1.0, 1.5, 15.0, 2.0));
        assert!(
            (track.value(MetricId::WinTrail, crate::metrics::Windows::secs(30.0), None, ts(2.0)) - 25.0).abs() < 1e-9
        );
        assert_eq!(track.value(MetricId::WinRise, crate::metrics::Windows::secs(30.0), None, ts(2.0)), 0.0);
    }

    #[test]
    fn tick_decays_quiet_flows() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::secs(10.0));
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));
        assert_eq!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(5.0)), 4.0);
        // Tick past the window edge → flow decays to zero even with no trade.
        track.on_tick(ts(11.0), None);
        assert_eq!(track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(11.0)), 0.0);
    }

    /// The span "10 SOL in ONE trade" — the reading no clock can produce.
    ///
    /// Three one-SOL prints and one three-SOL print are the same `gross_flow` over any
    /// seconds or slots window; over `prints(1, 0)` they are 1 and 3. That difference
    /// IS the basis, so it is asserted directly rather than through a rule.
    #[test]
    fn a_single_print_window_reads_one_transaction() {
        let mut spread = TokenTrack::new(ts(0.0));
        let mut one = TokenTrack::new(ts(0.0));
        for t in [&mut spread, &mut one] {
            t.ensure_window(WindowSpec::prints(1.0, 0.0));
            t.ensure_window(WindowSpec::secs(10.0));
        }
        for (i, sol) in [1.0, 1.0, 1.0].into_iter().enumerate() {
            spread.on_trade(buy(sol, 1.0, 20.0, i as f64));
        }
        one.on_trade(buy(3.0, 1.0, 20.0, 2.0));

        let prints = crate::metrics::Windows::one(WindowSpec::prints(1.0, 0.0));
        let secs = crate::metrics::Windows::secs(10.0);
        let now = ts(2.0);
        // Indistinguishable on the wall clock...
        assert_eq!(spread.value(MetricId::GrossFlow, secs, None, now), 3.0);
        assert_eq!(one.value(MetricId::GrossFlow, secs, None, now), 3.0);
        // ...and told apart by the print axis, which is the whole point.
        assert_eq!(spread.value(MetricId::GrossFlow, prints, None, now), 1.0);
        assert_eq!(one.value(MetricId::GrossFlow, prints, None, now), 3.0);
        assert_eq!(one.value(MetricId::TradeCount, prints, None, now), 1.0);
    }

    /// A print window counts prints, so its span is the same N trades however long
    /// they took, and a lag of 1 excludes the trade being folded — the causal shape a
    /// gate on "the tape BEFORE this transaction" needs.
    #[test]
    fn a_print_window_spans_trades_not_time_and_lags_by_trades() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::prints(3.0, 0.0));
        track.ensure_window(WindowSpec::prints(3.0, 1.0));
        let last3 = crate::metrics::Windows::one(WindowSpec::prints(3.0, 0.0));
        let prior3 = crate::metrics::Windows::one(WindowSpec::prints(3.0, 1.0));

        // Four prints, deliberately spread over an hour: the span is trades, so the
        // gaps between them are not part of the reading.
        for (sol, secs) in [(1.0, 0.0), (2.0, 900.0), (4.0, 1800.0), (8.0, 3600.0)] {
            track.on_trade(buy(sol, 1.0, 20.0, secs));
        }
        let now = ts(3600.0);
        assert_eq!(track.value(MetricId::GrossFlow, last3, None, now), 14.0, "prints 2..4");
        assert_eq!(track.value(MetricId::GrossFlow, prior3, None, now), 7.0, "prints 1..3");
        assert_eq!(track.value(MetricId::TradeCount, last3, None, now), 3.0);
    }

    /// Silence is not a print. A seconds window decays to nothing while the token is
    /// quiet; a print window HOLDS every trade it has, which is what makes a print
    /// gate read the same on a busy token and a dead one.
    #[test]
    fn ticks_never_decay_a_print_window() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::prints(5.0, 0.0));
        track.ensure_window(WindowSpec::secs(10.0));
        track.on_trade(buy(4.0, 1.0, 20.0, 0.0));

        let prints = crate::metrics::Windows::one(WindowSpec::prints(5.0, 0.0));
        track.on_tick(ts(3600.0), Some(9_999));
        assert_eq!(
            track.value(MetricId::Buy, crate::metrics::Windows::secs(10.0), None, ts(3600.0)),
            0.0,
            "the wall clock ran out"
        );
        assert_eq!(
            track.value(MetricId::Buy, prints, None, ts(3600.0)),
            4.0,
            "an hour of silence produced no prints, so the print window is unchanged"
        );
        assert_eq!(track.n_prints(), 1, "a tick is not a print");
    }

    /// Before the first trade a print window is EMPTY, not a phantom one — `0`
    /// prints means `size: 1, lag: 0` reads nothing rather than the bounds of a trade
    /// that has not happened.
    #[test]
    fn a_print_window_is_empty_before_the_first_trade() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::prints(1.0, 0.0));
        let prints = crate::metrics::Windows::one(WindowSpec::prints(1.0, 0.0));
        assert_eq!(track.n_prints(), 0);
        assert_eq!(track.value(MetricId::GrossFlow, prints, None, ts(1.0)), 0.0);
        assert_eq!(track.value(MetricId::TradeCount, prints, None, ts(1.0)), 0.0);
    }

    /// A print window and a slot window of the same size are DIFFERENT buffers, the
    /// same way seconds and slots are: three prints inside one slot is three on the
    /// print axis and one bucket on the slot axis, and merging them would make one of
    /// two authored conditions disappear.
    #[test]
    fn print_and_slot_windows_of_one_size_do_not_dedupe() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::prints(1.0, 0.0));
        track.ensure_window(WindowSpec::slots(1.0, 0.0));
        assert_ne!(
            WindowSpec::prints(1.0, 0.0).key(),
            WindowSpec::slots(1.0, 0.0).key(),
            "one size, two bases, two buffers"
        );
        for sol in [1.0, 2.0, 5.0] {
            track.on_trade(TradeLite { slot: 7, ..buy(sol, 1.0, 20.0, 0.0) });
        }
        let now = ts(0.0);
        let prints = crate::metrics::Windows::one(WindowSpec::prints(1.0, 0.0));
        let slots = crate::metrics::Windows::one(WindowSpec::slots(1.0, 0.0));
        assert_eq!(track.value(MetricId::GrossFlow, prints, None, now), 5.0, "the last print");
        assert_eq!(track.value(MetricId::GrossFlow, slots, None, now), 8.0, "the whole slot");
    }

    #[test]
    fn values_batch_fills_caller_buffer() {
        let mut track = TokenTrack::new(ts(0.0));
        track.ensure_window(WindowSpec::secs(10.0));
        track.on_trade(buy(3.0, 2.0, 15.0, 1.0));
        let reqs = [
            (MetricId::Time, None, None),
            (MetricId::Liquidity, None, None),
            (MetricId::Buy, Some(WindowSpec::secs(10.0)), None),
        ];
        let mut out = [0.0_f64; 3];
        track.values(&reqs, ts(5.0), &mut out);
        assert_eq!(out, [5.0, 15.0, 3.0]);
    }

    #[test]
    fn flow_unconfigured_is_nan_configured_splits() {
        use crate::metrics::flow_ix::ix_hash;
        use std::collections::BTreeSet;

        let mut track = TokenTrack::new(ts(0.0));
        let id = fp(1);
        // No ensure_flow → NaN.
        assert!(track
            .value(MetricId::TaggedBuy, crate::metrics::Windows::NONE, Some(id), ts(1.0))
            .is_nan());

        let patterns = FlowPatterns::new(BTreeSet::from([ix_hash(&["vol"])]));
        track.ensure_flow(id, &patterns, &[WindowSpec::secs(10.0)]);
        track.seed_creator(99);

        let mut t = buy(4.0, 1.0, 20.0, 0.0);
        t.ix_hash = Some(ix_hash(&["vol"]));
        t.wallet_hash = 1;
        track.on_trade(t);

        let mut t2 = buy(6.0, 1.0, 26.0, 1.0);
        t2.wallet_hash = 2;
        track.on_trade(t2);

        assert_eq!(track.value(MetricId::TaggedBuy, crate::metrics::Windows::NONE, Some(id), ts(2.0)), 4.0);
        assert_eq!(track.value(MetricId::UntaggedBuy, crate::metrics::Windows::NONE, Some(id), ts(2.0)), 6.0);
        assert_eq!(
            track.value(MetricId::WinTaggedBuy, crate::metrics::Windows::secs(10.0), Some(id), ts(2.0)),
            4.0
        );
        // Missing fingerprint arg → NaN even when state exists.
        assert!(crate::metrics::is_flow_metric(MetricId::TaggedBuy));
        assert!(track.value(MetricId::TaggedBuy, crate::metrics::Windows::NONE, None, ts(2.0)).is_nan());
    }

    /// A build on BOTH `m_flow_ix.ix_patterns` and `m_dump_ix.ix_patterns` is counted
    /// by both groups, and that is the intended reading, not a double count: the flow
    /// split says the sell is part of the family's trade history, the dump group says
    /// it carries the dev's dump shape, and the two states are independent. Pinned
    /// here because the guard that used to forbid the overlap made the only way to
    /// configure a dump build a deletion from the flow list - which moves those sells
    /// to untagged and changes what every `tagged_sell` rule measures.
    #[test]
    fn one_sell_on_both_lists_counts_in_both_groups() {
        use crate::metrics::flow_ix::ix_hash;
        use std::collections::BTreeSet;

        let shared = ix_hash(&["Pump.Fun: Sell", "Token Program: CloseAccount"]);
        let mut track = TokenTrack::new(ts(0.0));
        let id = fp(1);
        track.ensure_flow(id, &FlowPatterns::new(BTreeSet::from([shared])), &[]);
        track.ensure_dump(id, &DumpPatterns::new(BTreeSet::from([shared])), &[]);
        track.seed_creator(99);

        let mut t = buy(3.0, 1.0, 20.0, 0.0);
        t.side = Side::Sell;
        t.ix_hash = Some(shared);
        t.wallet_hash = 7;
        track.on_trade(t);

        let none = crate::metrics::Windows::NONE;
        let v = |id_m| track.value(id_m, none, Some(id), ts(1.0));
        assert_eq!(v(MetricId::TaggedSell), 3.0, "the flow split still tags the sell");
        assert_eq!(v(MetricId::DumpSell), 3.0, "and the dump group still counts it");
        assert_eq!(v(MetricId::UntaggedSell), 0.0, "neither group steals it from the other");
        assert_eq!(v(MetricId::TaggedSellCount), 1.0);
        assert_eq!(v(MetricId::DumpSellCount), 1.0);
    }

    /// **Cross-implementation parity.** Three real token tapes from the lake, replayed
    /// through `TokenTrack`, asserted against the values an independent SQL
    /// implementation computed for the same instant. The rule these metrics carry was
    /// fitted in that SQL, so any semantic drift between the two - a window edge, a
    /// poisoned-trade rule, a ratio's empty case - silently moves the threshold that
    /// ships away from the one that was validated. Fixture: the three rule-firing
    /// tokens with the fewest trades, so it stays readable.
    #[test]
    fn engine_metrics_match_the_sql_the_rule_was_fitted_in() {
        struct Case {
            mint: &'static str,
            /// `(offset_secs_from_first_trade, is_buy, sol, wallet)`
            trades: &'static [(f64, bool, f64, u64)],
            now: f64,
            buy3: f64,
            buy5: f64,
            gross60: f64,
            ntx60: f64,
            uw10: f64,
            tpw10: f64,
            lifegross: f64,
            lifentx: f64,
            buyshare10: f64,
            /// `m_flow_window{window_size_sec: 60, slice_size_sec: 3}.trade_share`
            share60_3: f64,
            /// Same reference window, a wider slice — the pair that proves the
            /// second axis is read and not ignored.
            share60_10: f64,
        }
        let cases = [
            Case {
                mint: "EmZkRz1q",
                trades: &[
                    (0.000000, true, 4.000000000, 1),
                    (0.000628, true, 2.300000000, 2),
                    (0.001010, true, 2.700000000, 3),
                    (7.921934, false, 5.173469387, 1),
                    (8.425656, false, 2.005896761, 2),
                    (8.455656, false, 1.820633850, 3),
                    (8.606678, true, 0.296296296, 4),
                    (190.617137, false, 0.296296295, 4),
                    (332.726772, true, 3.111477703, 5),
                    (332.730405, true, 2.110947953, 6),
                    (332.731504, true, 1.707943763, 7),
                    (332.739706, true, 3.394372016, 8),
                    (332.751188, true, 4.341925230, 1),
                    (334.436165, false, 4.127288448, 8),
                    (334.440091, false, 4.567766132, 5),
                    (334.440176, false, 1.622362176, 7),
                    (334.588120, false, 2.010570212, 6),
                    (334.824645, false, 2.338679695, 1),
                ],
                now: 334.824645,
                buy3: 14.666666665,
                buy5: 14.666666665,
                gross60: 29.333333328,
                ntx60: 10.0,
                uw10: 5.0,
                tpw10: 2.000000000,
                lifegross: 47.925925917,
                lifentx: 18.0,
                buyshare10: 50.000000003,
                share60_3: 100.0,
                share60_10: 100.0,
            },
            Case {
                mint: "2CewB2b1",
                trades: &[
                    (0.000000, true, 5.000000000, 1),
                    (0.000789, true, 3.965000000, 2),
                    (0.331632, true, 0.545901233, 3),
                    (1.400722, false, 0.545901232, 3),
                    (1.486422, false, 7.349716113, 1),
                    (1.498124, false, 3.511053484, 4),
                    (1.505395, false, 2.272230402, 2),
                    (1.559892, true, 1.975308641, 5),
                    (1.672612, true, 0.109507785, 6),
                    (3.790649, false, 0.109507784, 6),
                    (76.600342, false, 1.975308640, 5),
                    (140.667753, true, 1.950007476, 7),
                    (140.668349, true, 2.264512947, 8),
                    (140.668658, true, 2.669261795, 9),
                    (140.668997, true, 2.804014410, 10),
                    (140.713764, true, 2.059379313, 11),
                    (140.715517, true, 1.925031387, 12),
                    (140.715881, true, 2.420431215, 13),
                    (140.716362, true, 1.980219096, 14),
                    (140.716526, true, 2.019626664, 15),
                    (140.717024, true, 2.272601898, 16),
                ],
                now: 140.717024,
                buy3: 22.365086201,
                buy5: 22.365086201,
                gross60: 22.365086201,
                ntx60: 10.0,
                uw10: 10.0,
                tpw10: 1.000000000,
                lifegross: 49.724521515,
                lifentx: 21.0,
                buyshare10: 100.000000000,
                share60_3: 100.0,
                share60_10: 100.0,
            },
            Case {
                mint: "NMYKtKLS",
                trades: &[
                    (0.000000, true, 5.000000000, 1),
                    (0.000692, true, 2.000000000, 2),
                    (0.001253, true, 2.000000000, 3),
                    (0.001507, true, 2.000000000, 4),
                    (2.126228, false, 1.166551006, 1),
                    (2.214598, false, 1.102004999, 1),
                    (2.806099, false, 1.707148144, 1),
                    (3.197055, false, 2.721507005, 1),
                    (3.532430, true, 0.391111111, 5),
                    (4.137951, true, 2.007635548, 6),
                    (4.138246, true, 2.064413120, 7),
                    (4.138885, true, 1.794618000, 8),
                    (20.533181, false, 0.533549651, 5),
                    (81.752705, true, 1.655506109, 4),
                    (81.754106, true, 1.467052279, 3),
                    (81.754148, true, 1.766330501, 2),
                    (82.434242, true, 1.529342198, 3),
                    (82.434570, true, 1.725509662, 2),
                    (82.435232, true, 1.634037030, 4),
                    (83.206192, true, 1.598867187, 2),
                    (83.206245, true, 1.581689765, 3),
                    (84.700243, false, 3.809554246, 4),
                    (84.700304, false, 3.912501603, 3),
                ],
                now: 84.700304,
                buy3: 12.958334731,
                buy5: 12.958334731,
                gross60: 20.680390580,
                ntx60: 10.0,
                uw10: 3.0,
                tpw10: 3.333333333,
                lifegross: 45.168929164,
                lifentx: 23.0,
                buyshare10: 62.660009640,
                share60_3: 100.0,
                share60_10: 100.0,
            },
            // A 6ix-cohort tape read mid-life, where the two slice axes DIVERGE:
            // 4 of the last 20 trades landed in the last 3s, 17 in the last 10s. The
            // three cases above all read 100 on both (their whole 60s reference is
            // one cluster), so without this one the second axis could be dropped and
            // the harness would still pass.
            Case {
                mint: "22wow9yw",
                trades: &[
                    (0.000000, true, 3.000000000, 1),
                    (0.000042, true, 1.810000000, 2),
                    (0.000590, true, 1.190000000, 3),
                    (1.539222, false, 0.129135113, 1),
                    (1.696796, false, 0.128211987, 1),
                    (1.858192, false, 0.345055645, 1),
                    (1.909494, true, 0.353495243, 4),
                    (2.044103, false, 0.127358740, 1),
                    (3.021644, false, 0.102623499, 1),
                    (3.135742, false, 0.348984088, 4),
                    (3.202521, false, 0.271507091, 1),
                    (4.593791, false, 0.038128166, 3),
                    (4.652663, false, 0.103563739, 3),
                    (4.855668, false, 0.102950261, 3),
                    (7.454992, false, 0.085213335, 1),
                    (7.767080, false, 0.230284305, 1),
                    (10.956288, false, 0.073401251, 1),
                    (11.075448, false, 0.198599623, 1),
                    (11.256284, true, 0.009876542, 5),
                    (11.263913, false, 0.196424375, 1),
                ],
                now: 11.263913,
                buy3: 0.009876542,
                buy5: 0.009876542,
                gross60: 8.844813003,
                ntx60: 20.0,
                uw10: 4.0,
                tpw10: 4.250000000,
                lifegross: 8.844813003,
                lifentx: 20.0,
                buyshare10: 12.773134284,
                share60_3: 20.000000000,
                share60_10: 85.000000000,
            },
        ];
        for c in &cases {
            let mut track = TokenTrack::new(ts(0.0));
            for w in [3.0, 5.0, 10.0, 60.0] {
                track.ensure_window(WindowSpec::secs(w));
                // The crowd metrics read their own deque, so the parity harness has to
                // register it - exactly as a rule gating on them does.
                track.ensure_crowd_window(WindowSpec::secs(w));
            }
            for &(at, is_buy, sol, wallet) in c.trades {
                track.on_trade(TradeLite {
                    slot: 0,
                    marker_bits: 0,
                    side: if is_buy { Side::Buy } else { Side::Sell },
                    sol,
                    price: 1.0,
                    reserve_sol: 100.0,
                    priced_reserve_sol: 100.0,
                    at: ts(at),
                    ix_hash: None,
                    wallet_hash: wallet,
                    leg_index: 0,
                    ..Default::default()
                });
            }
            let now = ts(c.now);
            let got = |id, w: Option<f64>| track.value(id, w.map(WindowSpec::secs).into(), None, now);
            // Each assertion carries that metric's own `eq_tolerance`: parity means
            // "indistinguishable to a condition", not bit equality.
            let close = |a: f64, b: f64, tol: f64, what: &str| {
                assert!((a - b).abs() <= tol, "{} {}: engine {a} vs sql {b} (tol {tol})", c.mint, what);
            };
            close(got(MetricId::Buy, Some(3.0)), c.buy3, 0.1, "m_flow_window(3).buy");
            close(got(MetricId::Buy, Some(5.0)), c.buy5, 0.1, "m_flow_window(5).buy");
            close(got(MetricId::GrossFlow, Some(60.0)), c.gross60, 0.1, "gross_flow(60)");
            close(got(MetricId::TradeCount, Some(60.0)), c.ntx60, 0.5, "trade_count(60)");
            close(got(MetricId::UniqueWallets, Some(10.0)), c.uw10, 0.5, "unique_wallets(10)");
            close(got(MetricId::TradesPerWallet, Some(10.0)), c.tpw10, 0.05, "trades_per_wallet(10)");
            close(got(MetricId::BuyShare, Some(10.0)), c.buyshare10, 0.5, "buy_share(10)");
            close(got(MetricId::LifeGrossFlow, None), c.lifegross, 0.1, "m_flow_lifetime.gross_flow");
            close(got(MetricId::LifeTradeCount, None), c.lifentx, 0.5, "m_flow_lifetime.trade_count");
            // The two-window read, through the same public entry point a rule uses.
            let share = |reference: f64, slice: f64| {
                track.value(
                    MetricId::SliceTradeShare,
                    crate::metrics::Windows::two(
                        WindowSpec::secs(reference),
                        WindowSpec::secs(slice),
                    ),
                    None,
                    now,
                )
            };
            close(share(60.0, 3.0), c.share60_3, 0.5, "m_flow_window{60,3}.trade_share");
            close(share(60.0, 10.0), c.share60_10, 0.5, "m_flow_window{60,10}.trade_share");
            // An unregistered axis reads NaN, never a silently narrower window.
            assert!(share(60.0, 7.0).is_nan(), "{}: unregistered slice axis must be NaN", c.mint);
        }
    }

}
