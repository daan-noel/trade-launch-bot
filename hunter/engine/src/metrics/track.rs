//! `TokenTrack` — all the metric state one coin carries, and the router every read
//! goes through.
//!
//! * One copy each of the coin-level states (`m_state`, lifetime price and flow, the
//!   wave), shared by every rule armed on the coin.
//! * Trailing windows deduped by span, one buffer family per subject (flow, price,
//!   crowd, ix shapes) so a rule pays only for the buffers its metrics read.
//! * One [`TagState`] per (fingerprint, tag) a loaded rule reads, and one
//!   [`TemplatePatterns`] per (fingerprint, tag) a slot or wave metric reads.
//!
//! Two fold entry points, matching the engine's events: [`on_trade`] and [`on_tick`].
//! Reads go through [`value`] with a [`MetricRef`] (metric + tag + span) and the
//! reading rule's fingerprint. Anything not registered reads `NaN`, which satisfies no
//! condition.
//!
//! [`on_trade`]: TokenTrack::on_trade
//! [`on_tick`]: TokenTrack::on_tick
//! [`value`]: TokenTrack::value

use std::collections::BTreeMap;

use crate::fingerprint::FingerprintId;

use super::build_window::BuildWindowState;
use super::burst_slot::{BurstSlotState, TemplatePatterns};
use super::burst_wave::BurstWaveState;
use super::crowd_after_age::{AgeAnchor, CrowdAfterAgeState};
use super::crowd_window::CrowdWindowState;
use super::flow_lifetime::FlowLifetimeState;
use super::flow_window::WindowState;
use super::holder_book::HolderBookState;
use super::price_lifetime::PriceLifetimeState;
use super::price_window::PriceWindowState;
use super::print_wallet::PrintWalletState;
use super::registry::{Family, Metric};
use super::state::StateMetrics;
use super::tags::config::TagPatterns;
use super::tags::state::TagState;
use super::tags::TagKey;
use super::{Cursor, MetricRef, Span, TradeLite, Ts, WindowKey, WindowSpec};

/// All metric state for one coin.
#[derive(Debug, Clone)]
pub struct TokenTrack {
    created_at: Ts,
    /// Highest slot observed — the cursor every slot window counts in. Monotonic; a
    /// tick advances it only when the producer supplies one.
    cur_slot: u64,
    /// Prints this coin has taken — the cursor every print window counts in. Bumped
    /// once per folded trade, BEFORE the fold, so the trade being folded sits at the
    /// cursor and `1p` is that trade alone. A tick never moves it: silence is not a
    /// print.
    n_prints: u64,
    state: StateMetrics,
    price_lifetime: PriceLifetimeState,
    flow_lifetime: FlowLifetimeState,
    /// Untagged flow windows (`m_flow.* [span]`), deduped by span.
    windows: BTreeMap<WindowKey, WindowState>,
    /// Price-extrema windows. Apart from `windows` so a rule reading only flow pays
    /// for no price deque, and vice versa.
    price_windows: BTreeMap<WindowKey, PriceWindowState>,
    /// Wallet windows (`m_crowd.unique_wallets` / `trades_per_wallet`). Their subject
    /// is the WALLET column, which nothing else in `windows` carries.
    crowd_windows: BTreeMap<WindowKey, CrowdWindowState>,
    /// Since-age buyer sets (`m_crowd.buyer_count [age60s]`), keyed by anchor.
    crowd_after_age: BTreeMap<AgeAnchor, CrowdAfterAgeState>,
    /// Ix-shape windows (`m_crowd.unique_ix_shapes`).
    build_windows: BTreeMap<WindowKey, BuildWindowState>,
    /// `m_print` — wallet -> last buy. `None` unless a loaded rule reads it.
    print_wallet: Option<PrintWalletState>,
    /// The per-wallet holder book behind `m_holdings.bag_share_pct`. `None` unless read.
    holder_book: Option<HolderBookState>,
    /// One state per (fingerprint, tag) a loaded rule reads at the trade level.
    tags: BTreeMap<(FingerprintId, TagKey), TagState>,
    /// One template view per (fingerprint, tag) a slot or wave metric reads.
    template_tags: BTreeMap<(FingerprintId, TagKey), TemplatePatterns>,
    /// This slot's buy prefix (`m_slot`, `m_crowd.unique_ix_templates`). `None` unless read.
    slot_state: Option<BurstSlotState>,
    /// The consecutive-slot buy wave (`m_wave`). Always folded.
    burst_wave: BurstWaveState,
    /// Creator wallet hash from `TokenCreated`, applied to every tag state.
    creator_wallet_hash: Option<u64>,
    /// Last observed *priced* SOL depth (`vsol`), for price impact only — not a metric,
    /// and deliberately NOT the `liquidity_sol` reading. See
    /// [`TradeLite::priced_reserve_sol`].
    priced_reserves: f64,
}

impl TokenTrack {
    /// Fresh state for a coin created at `created_at`.
    pub fn new(created_at: Ts) -> Self {
        Self {
            created_at,
            cur_slot: 0,
            n_prints: 0,
            state: StateMetrics::default(),
            price_lifetime: PriceLifetimeState::new(created_at),
            flow_lifetime: FlowLifetimeState::default(),
            windows: BTreeMap::new(),
            price_windows: BTreeMap::new(),
            crowd_windows: BTreeMap::new(),
            crowd_after_age: BTreeMap::new(),
            build_windows: BTreeMap::new(),
            print_wallet: None,
            holder_book: None,
            tags: BTreeMap::new(),
            template_tags: BTreeMap::new(),
            slot_state: None,
            burst_wave: BurstWaveState::default(),
            creator_wallet_hash: None,
            priced_reserves: f64::NAN,
        }
    }

    // ── Registration (idempotent; a reload re-registers) ─────────────────────

    /// An untagged flow window.
    pub fn ensure_window(&mut self, spec: WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| WindowState::new(spec));
    }

    pub fn ensure_price_window(&mut self, spec: WindowSpec) {
        self.price_windows.entry(spec.key()).or_insert_with(|| PriceWindowState::new(spec));
    }

    pub fn ensure_crowd_window(&mut self, spec: WindowSpec) {
        self.crowd_windows.entry(spec.key()).or_insert_with(|| CrowdWindowState::new(spec));
    }

    /// A since-age buyer set holding at most `cap` wallets; a re-registration only
    /// ever RAISES the cap.
    pub fn ensure_crowd_after_age(&mut self, anchor: AgeAnchor, cap: u32) {
        self.crowd_after_age
            .entry(anchor)
            .and_modify(|s| s.raise_cap(cap))
            .or_insert_with(|| CrowdAfterAgeState::new(anchor, cap));
    }

    pub fn ensure_build_window(&mut self, spec: WindowSpec) {
        self.build_windows.entry(spec.key()).or_insert_with(|| BuildWindowState::new(spec));
    }

    /// Open the `m_print` wallet map. A map opened mid-life knows only the buys folded
    /// after it, like any newly registered window.
    pub fn ensure_print_wallet(&mut self) {
        self.print_wallet.get_or_insert_with(PrintWalletState::default);
    }

    /// Open the holder book. Mid-life: it knows only the prints folded after it.
    pub fn ensure_holder_book(&mut self) {
        self.holder_book.get_or_insert_with(HolderBookState::default);
    }

    /// Open this slot's buy prefix (`m_slot`, `m_crowd.unique_ix_templates`).
    pub fn ensure_slot_state(&mut self) {
        self.slot_state.get_or_insert_with(BurstSlotState::default);
    }

    /// Register one trade-level tag of one fingerprint with the windows its rules read.
    ///
    /// Re-registering **adopts the new definition**: a reload happens because the
    /// operator edited it, and keeping the one compiled at first sight would leave every
    /// coin already tracked classifying against the old list for the rest of its life.
    /// Trades already folded keep the half they were folded under.
    pub fn ensure_tag(&mut self, fp: FingerprintId, key: TagKey, patterns: &TagPatterns, windows: &[WindowSpec]) {
        let creator = self.creator_wallet_hash;
        let state = self.tags.entry((fp, key)).or_insert_with(|| {
            let mut s = TagState::new(patterns.clone());
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

    /// Register one tag's template view for the slot and wave families.
    pub fn ensure_template_tag(&mut self, fp: FingerprintId, key: TagKey, patterns: &TemplatePatterns) {
        self.template_tags.insert((fp, key), patterns.clone());
    }

    /// Seed the creator wallet on every tag state.
    pub fn seed_creator(&mut self, hash: u64) {
        self.creator_wallet_hash = Some(hash);
        for t in self.tags.values_mut() {
            t.set_creator(hash);
        }
    }

    /// Seed the coin's create slot so that slot is not a fireable wave.
    pub fn seed_creation_slot(&mut self, slot: u64) {
        self.burst_wave.seed_creation_slot(slot);
    }

    // ── Folds ────────────────────────────────────────────────────────────────

    /// Fold one trade into every state.
    pub fn on_trade(&mut self, t: TradeLite) {
        // The slot prefix reads the trail and the previous slot's liquidity BEFORE
        // this print is folded.
        let pre_trail = self.price_lifetime.trail();
        let prev_liq = self.state.liquidity();
        if let Some(s) = self.slot_state.as_mut() {
            s.on_trade(&t, pre_trail, prev_liq);
        }
        self.burst_wave.on_trade(&t);
        self.state.on_trade(t.reserve_sol, t.on_curve);
        self.priced_reserves = t.priced_reserve_sol;
        // The slot cursor only moves forward: a regressed feed row must not rewind
        // every slot window on the coin.
        self.cur_slot = self.cur_slot.max(t.slot);
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
        for ca in self.crowd_after_age.values_mut() {
            ca.on_trade(&t, self.created_at, self.creator_wallet_hash);
        }
        for bw in self.build_windows.values_mut() {
            let spec = bw.spec();
            bw.on_trade(t.sol, t.build_hash, spec.pos(t.at, at), spec.now_pos(t.at, cur));
        }
        if let Some(pw) = self.print_wallet.as_mut() {
            pw.on_trade(&t);
        }
        if let Some(hb) = self.holder_book.as_mut() {
            hb.on_trade(&t);
        }
        for pw in self.price_windows.values_mut() {
            let spec = pw.spec();
            pw.on_trade(t.price, spec.pos(t.at, at), spec.now_pos(t.at, cur));
        }
        for tag in self.tags.values_mut() {
            tag.on_trade(&t, cur);
        }
    }

    /// Advance time to `now` without a trade: evict stale window entries so a quiet
    /// coin's windows decay.
    pub fn on_tick(&mut self, now: Ts, slot: Option<u64>) {
        // A slot axis has no clock of its own: it advances only when the producer says
        // a slot passed, and holds otherwise rather than guess from elapsed time.
        if let Some(sl) = slot {
            self.cur_slot = self.cur_slot.max(sl);
        }
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
        for ca in self.crowd_after_age.values_mut() {
            ca.on_tick();
        }
        for bw in self.build_windows.values_mut() {
            let now_pos = bw.spec().now_pos(now, cur);
            bw.evict(now_pos);
        }
        if let Some(pw) = self.print_wallet.as_mut() {
            pw.on_tick();
        }
        for tag in self.tags.values_mut() {
            tag.on_tick(now, cur);
        }
        if let Some(s) = self.slot_state.as_mut() {
            s.on_tick();
        }
        self.burst_wave.on_tick();
    }

    // ── Cursors and raw readings ─────────────────────────────────────────────

    fn cursor(&self) -> Cursor {
        Cursor { slot: self.cur_slot, print: self.n_prints }
    }

    /// The highest slot this coin has observed (`0` before the first trade).
    /// The coin's creation instant (what `m_state.age_sec` counts from).
    pub fn created_at(&self) -> Ts {
        self.created_at
    }

    pub fn cur_slot(&self) -> u64 {
        self.cur_slot
    }

    /// How many prints this coin has taken (`0` before the first trade).
    pub fn n_prints(&self) -> u64 {
        self.n_prints
    }

    /// The last pool price (`NaN` before the first trade): what a tick marks a
    /// position against.
    pub fn current_price(&self) -> f64 {
        self.price_lifetime.last_price()
    }

    /// The last real SOL reserve (`NaN` before the first trade) — what the deadness
    /// verdict reads.
    pub fn current_reserves(&self) -> f64 {
        self.state.liquidity()
    }

    /// The last *priced* SOL depth (`vsol`) — the basis price impact is charged
    /// against, NOT the `liquidity_sol` reading.
    pub fn current_priced_reserves(&self) -> f64 {
        self.priced_reserves
    }

    /// Whether this coin keeps state for `(fp, tag)`.
    pub fn has_tag(&self, fp: FingerprintId, key: TagKey) -> bool {
        self.tags.contains_key(&(fp, key))
    }

    // ── The read ─────────────────────────────────────────────────────────────

    /// One reading at `now`. `fp` is the reading rule's fingerprint (tags are scoped to
    /// it). Position metrics read `NaN` here — they come from a
    /// [`PositionCtx`](super::position::PositionCtx), and before entry (the only place
    /// the track is asked for one, the pre-entry veto) a position line must not hold.
    pub fn value(&self, r: MetricRef, fp: Option<FingerprintId>, now: Ts) -> f64 {
        let cur = self.cursor();
        let m = r.metric;
        let window_read = |spec: Option<WindowSpec>| spec.map(|s| (s, s.now_pos(now, cur)));
        match m.family() {
            Family::State => self.state.value(m, self.created_at, now),
            Family::Price => match window_read(r.span.window) {
                None => self.price_lifetime.value(m, now),
                Some((s, pos)) => self.price_windows.get(&s.key()).map_or(f64::NAN, |w| w.value(m, pos)),
            },
            Family::Flow => match r.tag {
                Some(t) => fp
                    .and_then(|fp| self.tags.get(&(fp, t.key)))
                    .map_or(f64::NAN, |st| st.value(m, t.negated, r.span.window, now, cur)),
                None if matches!(m, Metric::SliceTradeSharePct | Metric::SliceSolSharePct) => self.slice(m, r.span, now),
                None => match window_read(r.span.window) {
                    None => self.flow_lifetime.value(m),
                    Some((s, pos)) => self.windows.get(&s.key()).map_or(f64::NAN, |w| w.value(m, pos)),
                },
            },
            Family::Holdings => match (m, r.tag) {
                (Metric::ProfitSol, Some(t)) => fp
                    .and_then(|fp| self.tags.get(&(fp, t.key)))
                    .map_or(f64::NAN, |st| st.profit_sol(t.negated)),
                (Metric::BagSharePct, Some(t)) => {
                    self.holder_book.as_ref().map_or(f64::NAN, |b| b.bag_share_pct(t.name))
                }
                _ => f64::NAN,
            },
            Family::Crowd => match m {
                Metric::UniqueWallets | Metric::TradesPerWallet => window_read(r.span.window)
                    .and_then(|(s, pos)| self.crowd_windows.get(&s.key()).map(|w| w.value(m, pos)))
                    .unwrap_or(f64::NAN),
                Metric::UniqueIxShapes => window_read(r.span.window)
                    .and_then(|(s, pos)| self.build_windows.get(&s.key()).map(|w| w.value(m, pos)))
                    .unwrap_or(f64::NAN),
                Metric::BuyerCount | Metric::BuyerIsNew => r
                    .span
                    .since_age
                    .and_then(|a| self.crowd_after_age.get(&a))
                    .map_or(f64::NAN, |s| s.value(m)),
                Metric::UniqueIxTemplates => self.slot_read(r, fp),
                _ => f64::NAN,
            },
            Family::Print => self.print_wallet.as_ref().map_or(f64::NAN, |p| p.value(m)),
            Family::Slot => self.slot_read(r, fp),
            Family::Wave => match r.tag {
                None => self.burst_wave.value(m, None),
                Some(t) => match fp.and_then(|fp| self.template_tags.get(&(fp, t.key))) {
                    Some(p) => self.burst_wave.value(m, Some(p)),
                    None => f64::NAN,
                },
            },
            Family::Position => f64::NAN,
        }
    }

    /// A slot-prefix read: untagged, or through the tag's template view. A tagged read
    /// whose tag has no template view reads `NaN`, never the untagged number.
    fn slot_read(&self, r: MetricRef, fp: Option<FingerprintId>) -> f64 {
        let Some(s) = self.slot_state.as_ref() else { return f64::NAN };
        match r.tag {
            None => s.value(r.metric, None),
            Some(t) => match fp.and_then(|fp| self.template_tags.get(&(fp, t.key))) {
                Some(p) => s.value(r.metric, Some(p)),
                None => f64::NAN,
            },
        }
    }

    /// The two slice reads: the span's buffer is the reference, the slice's the
    /// numerator — both the coin's own flow windows, registered for each axis.
    fn slice(&self, m: Metric, span: Span, now: Ts) -> f64 {
        let cur = self.cursor();
        let (Some(rs), Some(bs)) = (span.window, span.slice) else { return f64::NAN };
        match (self.windows.get(&bs.key()), self.windows.get(&rs.key())) {
            (Some(b), Some(r)) => {
                let (bn, rn) = (bs.now_pos(now, cur), rs.now_pos(now, cur));
                if m == Metric::SliceTradeSharePct {
                    super::flow_slice::trade_share(b, r, bn, rn)
                } else {
                    super::flow_slice::sol_share(b, r, bn, rn)
                }
            }
            _ => f64::NAN,
        }
    }

    /// How many entries one tag window retains — see [`TagState::window_len`].
    #[cfg(test)]
    pub(crate) fn tag_window_len(&self, fp: FingerprintId, key: TagKey, spec: WindowSpec) -> Option<usize> {
        self.tags.get(&(fp, key)).and_then(|t| t.window_len(spec))
    }
}

#[cfg(test)]
#[path = "track_tests.rs"]
mod tests;
