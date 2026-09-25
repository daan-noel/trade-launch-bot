//! One tag on one coin: which trades carry it, and the running totals of both halves.
//!
//! Reads serve `m_flow.* @tag` / `@!tag` (lifetime or a trailing window) and
//! `m_holdings.profit_sol @tag` / `@!tag`. Every total is kept for BOTH halves, so a
//! quantity reads the same way whichever half a condition names.
//!
//! **Excluded trades** (a creation-slot buyer under `exclude_creation_slot`) move no
//! total at all: they count on neither side.

use std::collections::{BTreeMap, VecDeque};

use super::config::TagPatterns;
use crate::hash::HashedSet;
use crate::metrics::fee::FeeKeys;
use crate::metrics::flow_window::push_sorted;
use crate::metrics::registry::Metric;
use crate::metrics::{Cursor, Side, TradeLite, Ts, WindowKey, WindowSpec};

/// SOL, prints and transactions for one side of one half.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SideTotals {
    pub buy_sol: f64,
    pub sell_sol: f64,
    pub buy_n: u32,
    pub sell_n: u32,
    pub buy_tx: u32,
    pub sell_tx: u32,
}

impl SideTotals {
    fn add(&mut self, side: Side, sol: f64, first_leg: bool) {
        match side {
            Side::Buy => {
                self.buy_sol += sol;
                self.buy_n += 1;
                self.buy_tx += u32::from(first_leg);
            }
            Side::Sell => {
                self.sell_sol += sol;
                self.sell_n += 1;
                self.sell_tx += u32::from(first_leg);
            }
        }
    }

    /// The exact inverse of [`add`](Self::add) for a buffered entry.
    ///
    /// The side is the sign BIT, not `signed >= 0.0`: a zero-SOL sell is stored as
    /// `-0.0`, which compares `>= 0.0` and would take the buy arm. The SOL sums never
    /// notice (subtracting zero changes nothing), a COUNT does.
    fn sub_signed(&mut self, signed: f64, first_leg: bool) {
        if !signed.is_sign_negative() {
            self.buy_sol -= signed;
            self.buy_n = self.buy_n.saturating_sub(1);
            self.buy_tx = self.buy_tx.saturating_sub(u32::from(first_leg));
        } else {
            self.sell_sol += signed; // signed < 0
            self.sell_n = self.sell_n.saturating_sub(1);
            self.sell_tx = self.sell_tx.saturating_sub(u32::from(first_leg));
        }
    }
}

/// Both halves of the split.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SplitTotals {
    /// Trades carrying the tag.
    pub tagged: SideTotals,
    /// Every other trade the tag does not exclude.
    pub rest: SideTotals,
}

impl SplitTotals {
    fn half(&mut self, tagged: bool) -> &mut SideTotals {
        if tagged {
            &mut self.tagged
        } else {
            &mut self.rest
        }
    }

    /// One `m_flow` quantity for the half `negated` names.
    pub fn value(&self, metric: Metric, negated: bool) -> f64 {
        let s = if negated { self.rest } else { self.tagged };
        match metric {
            Metric::BuySol => s.buy_sol,
            Metric::SellSol => s.sell_sol,
            Metric::NetSol => s.buy_sol - s.sell_sol,
            Metric::GrossSol => s.buy_sol + s.sell_sol,
            Metric::BuyCount => f64::from(s.buy_n),
            Metric::SellCount => f64::from(s.sell_n),
            Metric::TradeCount => f64::from(s.buy_n + s.sell_n),
            Metric::BuyTxCount => f64::from(s.buy_tx),
            Metric::SellTxCount => f64::from(s.sell_tx),
            Metric::TagSharePct => {
                let vg = self.tagged.buy_sol + self.tagged.sell_sol;
                let ng = self.rest.buy_sol + self.rest.sell_sol;
                let total = vg + ng;
                if total > 0.0 {
                    100.0 * if negated { ng } else { vg } / total
                } else {
                    f64::NAN
                }
            }
            _ => f64::NAN,
        }
    }
}

/// One entry of a window buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    /// Buy positive, sell negative (`-0.0` for a zero-SOL sell).
    signed: f64,
    tagged: bool,
    first_leg: bool,
}

/// One trailing window: a position-sorted deque plus running totals over all of it, so
/// a read corrects only the two out-of-window ends instead of re-scanning.
#[derive(Debug, Clone, PartialEq)]
struct TagWindow {
    spec: WindowSpec,
    buf: VecDeque<(i64, Entry)>,
    totals: SplitTotals,
}

impl TagWindow {
    fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), totals: SplitTotals::default() }
    }

    fn push(&mut self, side: Side, sol: f64, tagged: bool, first_leg: bool, pos: i64, now_pos: i64) {
        let signed = match side {
            Side::Buy => sol,
            Side::Sell => -sol,
        };
        push_sorted(&mut self.buf, pos, Entry { signed, tagged, first_leg });
        self.totals.half(tagged).add(side, sol, first_leg);
        self.evict(now_pos);
    }

    fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, e)) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            self.totals.half(e.tagged).sub_signed(e.signed, e.first_leg);
        }
    }

    /// Totals over `[lo, hi]` at `now_pos`: the running totals minus the not-yet-evicted
    /// front and anything past `hi` (a lagged window's head, or a regressed block time).
    fn totals_at(&self, now_pos: i64) -> SplitTotals {
        let (lo, hi) = self.spec.bounds(now_pos);
        let mut out = self.totals;
        for &(pos, e) in self.buf.iter() {
            if pos >= lo {
                break;
            }
            out.half(e.tagged).sub_signed(e.signed, e.first_leg);
        }
        for &(pos, e) in self.buf.iter().rev() {
            if pos <= hi {
                break;
            }
            out.half(e.tagged).sub_signed(e.signed, e.first_leg);
        }
        out
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buf.len()
    }
}

/// Which half a folded trade lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Half {
    Tagged,
    Rest,
    Excluded,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ClusterGroup {
    ix_hash: Option<u64>,
    side: Side,
    fee: FeeKeys,
    first_sol: f64,
    close: u32,
}

/// Per-(coin, fingerprint, tag) state.
#[derive(Debug, Clone, PartialEq)]
pub struct TagState {
    patterns: TagPatterns,
    /// Wallets that carried the tag, under `sticky`. Membership only — one lookup per
    /// trade, on already-hashed addresses.
    sticky_wallets: HashedSet,
    creator_wallet_hash: Option<u64>,
    lifetime: SplitTotals,
    windows: BTreeMap<WindowKey, TagWindow>,
    /// The creation slot, from the launch print. `None` until it is folded.
    birth_slot: Option<u64>,
    /// Wallets excluded under `exclude_creation_slot`.
    birth_wallets: HashedSet,
    /// Cluster groups of the slot being folded; cleared when the slot moves.
    cluster_slot: u64,
    cluster_groups: Vec<ClusterGroup>,
    /// Each half's bag in token units, signed (buys in, sells out) — `profit_sol`.
    tagged_tokens: f64,
    rest_tokens: f64,
    /// `vsol` and `vtok` after the last folded trade: the curve a bag sells into.
    last_vsol: f64,
    last_vtok: f64,
}

impl TagState {
    pub fn new(patterns: TagPatterns) -> Self {
        Self {
            patterns,
            sticky_wallets: HashedSet::default(),
            creator_wallet_hash: None,
            lifetime: SplitTotals::default(),
            windows: BTreeMap::new(),
            birth_slot: None,
            birth_wallets: HashedSet::default(),
            cluster_slot: 0,
            cluster_groups: Vec::new(),
            tagged_tokens: 0.0,
            rest_tokens: 0.0,
            last_vsol: f64::NAN,
            last_vtok: f64::NAN,
        }
    }

    /// Point the creator matcher at a wallet. Under `sticky` the creator also joins the
    /// sticky set, so a creator the coin later re-points away from (the first-slot
    /// stand-in) keeps the tag it had.
    pub fn set_creator(&mut self, hash: u64) {
        self.creator_wallet_hash = Some(hash);
        if self.patterns.creator && self.patterns.sticky {
            self.sticky_wallets.insert(hash);
        }
    }

    /// Adopt an edited definition (rules reload). Trades already folded keep the half
    /// they were folded under — the totals are running sums — so an edit moves a live
    /// coin's future, never its past. The sticky set is kept: a wallet shown to carry
    /// the tag does not stop carrying it because the matcher that caught it changed.
    pub fn set_patterns(&mut self, patterns: &TagPatterns) {
        if &self.patterns != patterns {
            self.patterns = patterns.clone();
        }
    }

    pub fn ensure_window(&mut self, spec: WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| TagWindow::new(spec));
    }

    /// Classify and fold one trade into the lifetime totals and every window.
    pub fn on_trade(&mut self, t: &TradeLite, cur: Cursor) {
        if !t.sol.is_finite() || t.sol < 0.0 {
            return;
        }
        if t.is_launch && self.birth_slot.is_none() {
            self.birth_slot = Some(t.slot);
        }
        if t.priced_reserve_sol.is_finite() && t.priced_reserve_sol > 0.0 && t.price.is_finite() && t.price > 0.0 {
            self.last_vsol = t.priced_reserve_sol;
            self.last_vtok = t.priced_reserve_sol / t.price;
        }
        let tagged = match self.fold_half(t) {
            Half::Excluded => return,
            Half::Tagged => true,
            Half::Rest => false,
        };
        if tagged && self.patterns.sticky {
            self.sticky_wallets.insert(t.wallet_hash);
        }
        // A missing amount poisons that half's bag for good: NaN propagates, and
        // `profit_sol` reads NaN rather than a bag short by one trade.
        let delta = match t.side {
            Side::Buy => t.token_amount,
            Side::Sell => -t.token_amount,
        };
        if tagged {
            self.tagged_tokens += delta;
        } else {
            self.rest_tokens += delta;
        }
        let first_leg = t.leg_index == 0;
        self.lifetime.half(tagged).add(t.side, t.sol, first_leg);
        for w in self.windows.values_mut() {
            let pos = w.spec.pos(t.at, cur.at_trade(t));
            let now_pos = w.spec.now_pos(t.at, cur);
            w.push(t.side, t.sol, tagged, first_leg, pos, now_pos);
        }
    }

    pub fn on_tick(&mut self, now: Ts, cur: Cursor) {
        for w in self.windows.values_mut() {
            let now_pos = w.spec.now_pos(now, cur);
            w.evict(now_pos);
        }
    }

    /// One read. `window: None` = the coin's life.
    pub fn value(&self, metric: Metric, negated: bool, window: Option<WindowSpec>, now: Ts, cur: Cursor) -> f64 {
        match window {
            None if metric == Metric::ProfitSol => self.profit_sol(negated),
            None => self.lifetime.value(metric, negated),
            Some(spec) => match self.windows.get(&spec.key()) {
                Some(w) => w.totals_at(spec.now_pos(now, cur)).value(metric, negated),
                None => f64::NAN,
            },
        }
    }

    /// `profit_sol`: what one half would net selling its whole bag into the curve now,
    /// minus the SOL it has put in — `vsol - vsol*vtok/(vtok + bag) - (buy - sell)`,
    /// the bag floored at zero. Profit already taken counts: a half that sold out holds
    /// nothing and reads its net SOL out. `NaN` before a print with a reserve pair, and
    /// for good once one of its trades had no token amount.
    pub fn profit_sol(&self, negated: bool) -> f64 {
        let (bag, half) = if negated {
            (self.rest_tokens, self.lifetime.rest)
        } else {
            (self.tagged_tokens, self.lifetime.tagged)
        };
        let (v, vt) = (self.last_vsol, self.last_vtok);
        if !v.is_finite() || !vt.is_finite() || vt <= 0.0 || !bag.is_finite() {
            return f64::NAN;
        }
        let bag = bag.max(0.0);
        let liquidation = v - v * vt / (vt + bag);
        liquidation - (half.buy_sol - half.sell_sol)
    }

    /// The half a trade folds on: the matchers, then the cluster rule (which counts the
    /// trade into its slot group, so it runs once per trade and only here, and only
    /// when nothing else qualified it), then the creation-slot rule.
    fn fold_half(&mut self, t: &TradeLite) -> Half {
        let on_side = self.patterns.side.is_none_or(|s| s == t.side);
        if on_side && (self.matches(t) || self.cluster_hit(t)) {
            return Half::Tagged;
        }
        if self.patterns.exclude_creation_slot {
            if self.birth_slot == Some(t.slot) && t.side == Side::Buy {
                self.birth_wallets.insert(t.wallet_hash);
                return Half::Excluded;
            }
            if self.birth_wallets.contains(&t.wallet_hash) {
                return Half::Excluded;
            }
        }
        Half::Rest
    }

    /// Whether this trade would carry the tag on its matchers alone (the cluster rule
    /// counts trades into its groups, so it is left out of a question that must not
    /// change state).
    #[cfg(test)]
    pub(crate) fn carries(&self, t: &TradeLite) -> bool {
        self.patterns.side.is_none_or(|s| s == t.side) && self.matches(t)
    }

    /// Whether any stateless matcher (or the sticky set) qualifies the trade.
    fn matches(&self, t: &TradeLite) -> bool {
        let p = &self.patterns;
        p.marks(t.marker_bits)
            || (p.creator && self.creator_wallet_hash == Some(t.wallet_hash))
            || (p.sticky && self.sticky_wallets.contains(&t.wallet_hash))
            || p.builds.matches(t.ix_hash, t.fee)
            || t.program_hash.is_some_and(|h| p.programs.contains(&h))
            || t.template_hash.is_some_and(|h| p.templates.contains(&h))
            || (t.wallet_hash != 0 && p.wallets.contains(&t.wallet_hash))
    }

    /// Count this trade into its slot's cluster group; `true` once it is at least the
    /// `min_prints`-th close member.
    fn cluster_hit(&mut self, t: &TradeLite) -> bool {
        let Some(c) = self.patterns.cluster else { return false };
        if t.slot != self.cluster_slot {
            self.cluster_groups.clear();
            self.cluster_slot = t.slot;
        }
        let idx = match self
            .cluster_groups
            .iter()
            .position(|g| g.ix_hash == t.ix_hash && g.side == t.side && g.fee == t.fee)
        {
            Some(i) => i,
            None => {
                self.cluster_groups.push(ClusterGroup {
                    ix_hash: t.ix_hash,
                    side: t.side,
                    fee: t.fee,
                    first_sol: t.sol,
                    close: 0,
                });
                self.cluster_groups.len() - 1
            }
        };
        let g = &mut self.cluster_groups[idx];
        let close = (t.sol - g.first_sol).abs() <= f64::from(c.sol_tol_pct) / 100.0 * g.first_sol;
        if close {
            g.close += 1;
        }
        close && g.close >= c.min_prints
    }

    /// How many entries one window retains. Test-only: reads are corrected at both
    /// ends, so an un-evicted buffer returns the RIGHT number while it grows — only its
    /// length shows a tick never reached it.
    #[cfg(test)]
    pub(crate) fn window_len(&self, spec: WindowSpec) -> Option<usize> {
        self.windows.get(&spec.key()).map(TagWindow::len)
    }
}
