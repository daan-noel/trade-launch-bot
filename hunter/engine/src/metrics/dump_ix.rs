//! `m_dump_ix` / `m_dump_ix_window` — sells whose instruction build is on a named
//! list, counted and summed.
//!
//! **Not a split.** [`flow_ix`](super::flow_ix) partitions every trade into tagged
//! and untagged; this group partitions nothing. It answers one question about one
//! side: *how many sells carrying one of these builds landed, and for how much SOL*.
//! That is why it is its own group rather than a third axis on the flow classifier —
//! different subject, its own list, and no state at all on a trade that does not
//! match.
//!
//! **The two lists overlap, by design.** A build on `m_dump_ix.ix_patterns` may sit
//! on `m_flow_ix.ix_patterns` too, and normally does: a dev's dump shape is a sell
//! build of a family whose whole trade history the flow split already tags. The
//! lists answer different questions about one transaction — is this trade part of
//! the family's flow, and is this sell the dump shape — so a build that answers yes
//! to both belongs on both. Nothing sums across the groups: [`TokenTrack`](super::track::TokenTrack) reads
//! them from separate state, so one sell landing in `tagged_sell` AND in `dump_sell`
//! is two classifiers agreeing, not one event counted twice. Read them as two
//! answers, never as parts of a whole.
//!
//! **No wallet rules.** `m_flow_ix` can tag by contagion or by creator identity;
//! this cannot, and deliberately has no such knob. A build is a property of the
//! transaction. Contagion would make every later sell from a wallet that once sold
//! with a listed build also count as one, which is the opposite of reading the build.
//!
//! **Legs and transactions.** One Solana transaction can carry several
//! `Pump.Fun: Sell` instructions — four different wallets' bags sold at once is a
//! real and common shape — and every leg of a transaction carries the SAME
//! `ix_labels`, so a build matches all of a transaction's legs or none. The two
//! metrics therefore count different things on purpose:
//!
//! * [`DumpSell`](super::MetricId::DumpSell) sums **every leg**, because every leg
//!   moves SOL out of the curve and moves the price.
//! * [`DumpSellCount`](super::MetricId::DumpSellCount) counts **leg 0 only**, so it
//!   is a count of TRANSACTIONS.
//!
//! `dump_sell / dump_sell_count` is therefore SOL per transaction, not per sell.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

// `push_sorted` is shared rather than re-implemented: where a regressed `block_time`
// lands in the deque is one decision, and a second copy of it is free to disagree.
use super::flow_ix::ix_hash;
use super::flow_window::push_sorted;
use super::{Cursor, MetricId, Side, TradeLite, Ts, WindowKey, WindowSpec};

/// The config key this group reads, inside `fingerprints.metric_config`.
pub const CONFIG_KEY: &str = "m_dump_ix";

// ── Patterns ─────────────────────────────────────────────────────────────────

/// Compiled build list for one fingerprint (`m_dump_ix.ix_patterns`).
///
/// A bare hash set — there is nothing else to configure. Every knob `FlowPatterns`
/// carries (markers, contagion, creator) is a statement about who sent a trade, and
/// this group only asks what the transaction was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpPatterns {
    hashes: BTreeSet<u64>,
}

impl DumpPatterns {
    pub fn new(hashes: BTreeSet<u64>) -> Self {
        Self { hashes }
    }

    /// Compile an ordered list of label sequences.
    pub fn from_label_sequences(patterns: &[Vec<String>]) -> Self {
        let mut hashes = BTreeSet::new();
        for p in patterns {
            if !p.is_empty() {
                hashes.insert(ix_hash(p));
            }
        }
        Self { hashes }
    }

    /// Parse `metric_config["m_dump_ix"]`. `None` = key absent or unusable ⇒ the
    /// group is unconfigured and both metrics read `NaN`.
    ///
    /// Unconfigured must NOT read `0`: a rule spelling `dump_sell_count >= 2` would
    /// merely never fire, but `dump_sell_count <= 0` would fire on everything, and a
    /// bound that is satisfied by a missing configuration is the failure mode this
    /// whole group exists to avoid. `NaN` satisfies nothing either way.
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get(CONFIG_KEY)?;
        if !obj.is_object() {
            return None;
        }
        let arr = obj.get("ix_patterns")?.as_array()?;
        let mut hashes = BTreeSet::new();
        for row in arr {
            let labels = row.as_array()?;
            let mut seq: Vec<&str> = Vec::with_capacity(labels.len());
            for l in labels {
                seq.push(l.as_str()?);
            }
            if !seq.is_empty() {
                hashes.insert(ix_hash(&seq));
            }
        }
        Some(Self { hashes })
    }

    /// Shape errors in `metric_config["m_dump_ix"]`. Shape only — there is no
    /// cross-group rule, because the two lists overlap by design (see the module
    /// header).
    pub fn validate_metric_config(cfg: &Value) -> Result<(), String> {
        let Some(obj) = cfg.get(CONFIG_KEY) else {
            return Ok(());
        };
        let Some(map) = obj.as_object() else {
            return Err(format!("{CONFIG_KEY} must be an object"));
        };
        let Some(arr) = map.get("ix_patterns") else {
            return Err(format!("{CONFIG_KEY} carries no ix_patterns"));
        };
        let Some(rows) = arr.as_array() else {
            return Err(format!("{CONFIG_KEY}.ix_patterns must be an array of label arrays"));
        };
        for row in rows {
            let Some(labels) = row.as_array() else {
                return Err(format!("{CONFIG_KEY}.ix_patterns row must be an array of strings"));
            };
            for l in labels {
                if !l.is_string() {
                    return Err(format!("{CONFIG_KEY}.ix_patterns label must be a string"));
                }
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Whether this trade is a sell built from a listed shape — the whole classifier.
    fn matches(&self, t: &TradeLite) -> bool {
        t.side == Side::Sell && t.ix_hash.is_some_and(|h| self.hashes.contains(&h))
    }
}

// ── Totals ───────────────────────────────────────────────────────────────────

/// SOL over every matching leg, and transactions (matching leg 0s).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DumpTotals {
    pub sol: f64,
    pub txs: u32,
}

impl DumpTotals {
    fn add(&mut self, sol: f64, first_leg: bool) {
        self.sol += sol;
        if first_leg {
            self.txs += 1;
        }
    }

    fn sub(&mut self, sol: f64, first_leg: bool) {
        self.sol -= sol;
        if first_leg {
            self.txs = self.txs.saturating_sub(1);
        }
    }

    pub fn value(self, id: MetricId) -> f64 {
        use MetricId::*;
        match id {
            DumpSell | WinDumpSell => self.sol,
            DumpSellCount | WinDumpSellCount => f64::from(self.txs),
            _ => f64::NAN,
        }
    }
}

// ── Window ───────────────────────────────────────────────────────────────────

/// One trailing window. Same O(1)-read shape as the flow windows — running totals
/// over the whole deque, corrected at the two out-of-window ends on read — but the
/// deque holds ONLY matching sells, so a token nobody dumps on carries an empty one.
#[derive(Debug, Clone, PartialEq)]
struct DumpWindowState {
    spec: WindowSpec,
    /// `(pos, sol, first_leg)`, oldest at front, position-sorted.
    buf: VecDeque<(i64, (f64, bool))>,
    totals: DumpTotals,
}

impl DumpWindowState {
    fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), totals: DumpTotals::default() }
    }

    fn on_match(&mut self, sol: f64, first_leg: bool, pos: i64, now_pos: i64) {
        push_sorted(&mut self.buf, pos, (sol, first_leg));
        self.totals.add(sol, first_leg);
        self.evict(now_pos);
    }

    fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, (sol, first_leg))) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            self.totals.sub(sol, first_leg);
        }
    }

    fn totals_at(&self, now_pos: i64) -> DumpTotals {
        let (lo, hi) = self.spec.bounds(now_pos);
        let mut out = self.totals;
        for &(pos, (sol, first_leg)) in self.buf.iter() {
            if pos >= lo {
                break;
            }
            out.sub(sol, first_leg);
        }
        for &(pos, (sol, first_leg)) in self.buf.iter().rev() {
            if pos <= hi {
                break;
            }
            out.sub(sol, first_leg);
        }
        out
    }
}

// ── State ────────────────────────────────────────────────────────────────────

/// Per-(token, fingerprint) dump state: the compiled list, lifetime totals, and one
/// deque per registered window.
#[derive(Debug, Clone, PartialEq)]
pub struct DumpState {
    patterns: DumpPatterns,
    lifetime: DumpTotals,
    windows: BTreeMap<WindowKey, DumpWindowState>,
}

impl DumpState {
    pub fn new(patterns: DumpPatterns) -> Self {
        Self { patterns, lifetime: DumpTotals::default(), windows: BTreeMap::new() }
    }

    /// Adopt an edited list. Same contract as `FlowState::set_patterns`: trades
    /// already folded keep the verdict they were folded under (the totals are running
    /// sums and no trades are retained to redo), so an edit moves a live token's
    /// future, never its past.
    pub fn set_patterns(&mut self, patterns: &DumpPatterns) {
        if &self.patterns != patterns {
            self.patterns = patterns.clone();
        }
    }

    pub fn ensure_window(&mut self, spec: WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| DumpWindowState::new(spec));
    }

    /// Fold one trade. A non-matching trade costs one hash-set lookup and nothing
    /// else — no push, no eviction, no allocation.
    pub fn on_trade(&mut self, t: &TradeLite, cur: Cursor) {
        if !t.sol.is_finite() || t.sol < 0.0 || !self.patterns.matches(t) {
            return;
        }
        let first_leg = t.leg_index == 0;
        self.lifetime.add(t.sol, first_leg);
        for w in self.windows.values_mut() {
            let pos = w.spec.pos(t.at, cur.at_trade(t));
            let now_pos = w.spec.now_pos(t.at, cur);
            w.on_match(t.sol, first_leg, pos, now_pos);
        }
    }

    /// How many matched sells one window still retains. Test-only: reads are
    /// corrected at both window ends, so an un-evicted deque returns the RIGHT number
    /// while it grows — the retention is the only thing that can show a tick never
    /// reached this group.
    #[cfg(test)]
    pub(crate) fn window_len(&self, spec: WindowSpec) -> Option<usize> {
        self.windows.get(&spec.key()).map(|w| w.buf.len())
    }

    /// Evict on a tick, so a quiet token's windows decay without a trade.
    pub fn on_tick(&mut self, now: Ts, cur: Cursor) {
        for w in self.windows.values_mut() {
            let now_pos = w.spec.now_pos(now, cur);
            w.evict(now_pos);
        }
    }

    /// One metric. `window: None` ⇒ the lifetime group. An unregistered window reads
    /// `NaN` rather than a lifetime value, so a missing registration is loud.
    pub fn value(&self, id: MetricId, window: Option<WindowSpec>, now: Ts, cur: Cursor) -> f64 {
        match window {
            None => self.lifetime.value(id),
            Some(spec) => match self.windows.get(&spec.key()) {
                Some(w) => w.totals_at(spec.now_pos(now, cur)).value(id),
                None => f64::NAN,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn ts() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn c(slot: u64) -> Cursor {
        Cursor { slot, print: 0 }
    }

    const DUMP: &[&str] = &["Pump.Fun: Sell", "Token Program: CloseAccount"];

    fn sell(sol: f64, ix: Option<u64>, slot: u64, leg: u8) -> TradeLite {
        TradeLite {
            side: Side::Sell,
            sol,
            at: ts(),
            slot,
            leg_index: leg,
            ix_hash: ix,
            ..Default::default()
        }
    }

    fn state() -> DumpState {
        DumpState::new(DumpPatterns::new(BTreeSet::from([ix_hash(DUMP)])))
    }

    /// The reason the two metrics exist side by side: one bundled transaction is ONE
    /// transaction and FOUR legs of SOL, and both readings are wanted.
    #[test]
    fn a_four_leg_transaction_is_one_tx_and_all_of_its_sol() {
        let w = WindowSpec::slots(1.0, 0.0);
        let mut st = state();
        st.ensure_window(w);
        let h = Some(ix_hash(DUMP));
        for leg in 0..4u8 {
            st.on_trade(&sell(0.12, h, 100, leg), c(100));
        }
        assert_eq!(st.value(MetricId::WinDumpSellCount, Some(w), ts(), c(100)), 1.0);
        assert!((st.value(MetricId::WinDumpSell, Some(w), ts(), c(100)) - 0.48).abs() < 1e-9);
        // Lifetime agrees.
        assert_eq!(st.value(MetricId::DumpSellCount, None, ts(), c(100)), 1.0);
    }

    /// `dump_sell_count(1sl) >= 2` — two dump-built transactions in one slot.
    #[test]
    fn two_transactions_in_one_slot_count_two() {
        let w = WindowSpec::slots(1.0, 0.0);
        let mut st = state();
        st.ensure_window(w);
        let h = Some(ix_hash(DUMP));
        let n = |st: &DumpState, slot: u64| st.value(MetricId::WinDumpSellCount, Some(w), ts(), c(slot));

        st.on_trade(&sell(1.0, h, 100, 0), c(100));
        assert_eq!(n(&st, 100), 1.0, "one transaction is not the fire");
        st.on_trade(&sell(1.0, h, 100, 0), c(100));
        assert_eq!(n(&st, 100), 2.0, "the second one is");

        // The window releases — a latched counter would exit every later token.
        st.on_trade(&sell(1.0, h, 105, 0), c(105));
        assert_eq!(n(&st, 105), 1.0);
        assert_eq!(n(&st, 106), 0.0);
    }

    /// Only sells, only listed builds. A BUY matching a listed shape counts nothing:
    /// the list names sell builds, and a mis-entered buy row must not inflate a dump.
    #[test]
    fn only_listed_sells_count() {
        let w = WindowSpec::slots(1.0, 0.0);
        let mut st = state();
        st.ensure_window(w);
        let h = Some(ix_hash(DUMP));
        let n = |st: &DumpState| st.value(MetricId::WinDumpSellCount, Some(w), ts(), c(100));

        st.on_trade(&sell(9.0, Some(ix_hash(&["Pump.Fun: Sell"])), 100, 0), c(100));
        assert_eq!(n(&st), 0.0, "an unlisted build is not a dump, however big");
        st.on_trade(&sell(9.0, None, 100, 0), c(100));
        assert_eq!(n(&st), 0.0, "missing labels are not a dump either");
        st.on_trade(&TradeLite { side: Side::Buy, ..sell(9.0, h, 100, 0) }, c(100));
        assert_eq!(n(&st), 0.0, "a BUY on a listed build is not a sell");
        st.on_trade(&sell(1.0, h, 100, 0), c(100));
        assert_eq!(n(&st), 1.0);
    }

    /// Unconfigured must read NaN, never 0 — `<= 0` on a zero would fire on every
    /// token that has no dump list at all.
    #[test]
    fn an_unconfigured_group_is_none_and_reads_nan() {
        assert!(DumpPatterns::from_metric_config(&json!({})).is_none());
        assert!(DumpPatterns::from_metric_config(&json!({"m_flow_ix": {"ix_patterns": []}})).is_none());
        let cfg = json!({"m_dump_ix": {"ix_patterns": [["Pump.Fun: Sell"]]}});
        assert!(DumpPatterns::from_metric_config(&cfg).is_some());
    }

    /// An unregistered window reads NaN rather than silently answering with the
    /// lifetime total, which would be a much larger number that satisfies a bound.
    #[test]
    fn an_unregistered_window_reads_nan() {
        let mut st = state();
        st.on_trade(&sell(1.0, Some(ix_hash(DUMP)), 100, 0), c(100));
        let unregistered = WindowSpec::slots(5.0, 0.0);
        assert!(st.value(MetricId::WinDumpSellCount, Some(unregistered), ts(), c(100)).is_nan());
        assert_eq!(st.value(MetricId::DumpSellCount, None, ts(), c(100)), 1.0);
    }

    /// One build in BOTH lists is legal and is the normal case — the dev's dump
    /// shape is a sell build of a family the flow split already tags. Rejecting it
    /// would force the only fix to be deleting the build from `m_flow_ix`, which
    /// silently moves those sells to untagged.
    #[test]
    fn a_build_may_sit_in_both_lists() {
        let shared = json!(["Pump.Fun: Sell", "Token Program: CloseAccount"]);
        let cfg = json!({
            "m_flow_ix": {"ix_patterns": [shared]},
            "m_dump_ix": {"ix_patterns": [shared]},
        });
        assert!(DumpPatterns::validate_metric_config(&cfg).is_ok());
        // And it compiles into this group's list unchanged: the overlap is not
        // quietly dropped on the way in either.
        let p = DumpPatterns::from_metric_config(&cfg).expect("configured");
        assert!(p.matches(&sell(1.0, Some(ix_hash(DUMP)), 100, 0)));
    }

    #[test]
    fn shape_errors_are_rejected() {
        for bad in [
            json!({"m_dump_ix": []}),
            json!({"m_dump_ix": {}}),
            json!({"m_dump_ix": {"ix_patterns": "x"}}),
            json!({"m_dump_ix": {"ix_patterns": ["x"]}}),
            json!({"m_dump_ix": {"ix_patterns": [[1]]}}),
        ] {
            assert!(DumpPatterns::validate_metric_config(&bad).is_err(), "{bad}");
        }
        assert!(DumpPatterns::validate_metric_config(&json!({})).is_ok());
    }
}
