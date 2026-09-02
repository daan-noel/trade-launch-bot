//! `m_copy` / `m_copy_window` — what a **named wallet list** did on this token.
//!
//! One subject: the trades signed by the wallets a fingerprint names. `m_copy` is
//! that lifetime (since the token entered tracking), `m_copy_window` the same four
//! quantities over a trailing window. The list lives on the fingerprint
//! (`metric_config`), so the group stays reusable — one fingerprint per target.
//!
//! **The trigger is the window, never the lifetime.** `m_copy.buy_count >= 1` is a
//! latch: once he has bought, it is true for the rest of the token's life and the
//! rule fires on every later print. A copy trigger is `m_copy_window.buy_sol` on a
//! `1p` (this print) or `1sl` (this slot) window. The lifetime side is for filters —
//! "he has already put 2 SOL in", "he has not sold yet".
//!
//! **Legs and transactions**, the same split [`dump_ix`](super::dump_ix) makes and
//! for the same reason: one transaction can carry several legs, every leg moves SOL
//! and moves the price, but a decision is made once per transaction.
//! * `buy_sol` / `sell_sol` sum **every leg**.
//! * `buy_count` / `sell_count` count **leg 0 only**, so both are TRANSACTION counts.
//!
//! **No scope filter.** Every print signed by a listed wallet counts: curve and AMM,
//! launch creates included. The subject is what that wallet did, and a hidden
//! exclusion here would be a second, invisible rule. Curve-only entry is a property
//! of the *engine*, not of this group — a token is tracked from creation and
//! `Event::Migrated` disarms it, so an entry can only fire while the token is on the
//! curve, while an open position rides migration out and keeps reading AMM prints.
//!
//! **The wallet is who the venue credited**, not necessarily who signed: an
//! aggregator routes its customers through a PDA of its own, so a router address on
//! this list reads as hundreds of thousands of unrelated people. List the target's
//! own address (see the `wallet_dict.is_proxy` rule in `hunter/CLAUDE.md`).

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;

use crate::hash::HashedSet;

use super::flow_ix::wallet_hash;
use super::flow_window::push_sorted;
use super::{Cursor, MetricId, Side, TradeLite, Ts, WindowKey, WindowSpec};

/// The config key both groups read, inside `fingerprints.metric_config`.
pub const CONFIG_KEY: &str = "m_copy";

/// The one field under [`CONFIG_KEY`] — base58 wallet addresses.
pub const TARGETS_FIELD: &str = "target_wallets";

// ── Patterns ─────────────────────────────────────────────────────────────────

/// Compiled target-wallet list for one fingerprint (`m_copy.target_wallets`).
///
/// Addresses in, [`wallet_hash`]es out — the same FNV-1a digest every adapter puts
/// on [`TradeLite::wallet_hash`], so a list written as base58 matches live prints,
/// lake rows and replayed events identically.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyPatterns {
    wallets: HashedSet,
}

impl CopyPatterns {
    /// Compile from addresses — the shape callers outside the JSON path want.
    pub fn from_addresses<I, S>(addrs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut wallets = HashedSet::default();
        for a in addrs {
            let a = a.as_ref();
            if !a.is_empty() {
                wallets.insert(wallet_hash(a));
            }
        }
        Self { wallets }
    }

    /// Parse `metric_config["m_copy"]`. `None` = key absent or unusable ⇒ the group
    /// is unconfigured and every metric reads `NaN`.
    ///
    /// Unconfigured must NOT read `0`: `buy_sol >= 0.5` would merely never fire, but
    /// `sell_count <= 0` would fire on everything — a bound satisfied by a MISSING
    /// target list is the failure this whole group exists to avoid. `NaN` satisfies
    /// nothing either way.
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get(CONFIG_KEY)?;
        if !obj.is_object() {
            return None;
        }
        let arr = obj.get(TARGETS_FIELD)?.as_array()?;
        let mut wallets = HashedSet::default();
        for row in arr {
            let addr = row.as_str()?;
            if !addr.is_empty() {
                wallets.insert(wallet_hash(addr));
            }
        }
        Some(Self { wallets })
    }

    pub fn validate_metric_config(cfg: &Value) -> Result<(), String> {
        let Some(obj) = cfg.get(CONFIG_KEY) else {
            return Ok(());
        };
        let Some(map) = obj.as_object() else {
            return Err(format!("{CONFIG_KEY} must be an object"));
        };
        let Some(arr) = map.get(TARGETS_FIELD) else {
            return Err(format!("{CONFIG_KEY} carries no {TARGETS_FIELD}"));
        };
        let Some(rows) = arr.as_array() else {
            return Err(format!(
                "{CONFIG_KEY}.{TARGETS_FIELD} must be an array of wallet addresses"
            ));
        };
        for row in rows {
            let Some(addr) = row.as_str() else {
                return Err(format!(
                    "{CONFIG_KEY}.{TARGETS_FIELD} entry must be a base58 wallet address string"
                ));
            };
            if addr.trim().is_empty() {
                return Err(format!("{CONFIG_KEY}.{TARGETS_FIELD} entry must not be blank"));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.wallets.is_empty()
    }

    /// Whether this print was signed by a listed wallet — the whole classifier.
    /// Wallet `0` is "no wallet column", never an identity, so it matches nothing.
    fn matches(&self, t: &TradeLite) -> bool {
        t.wallet_hash != 0 && self.wallets.contains(&t.wallet_hash)
    }
}

// ── Totals ───────────────────────────────────────────────────────────────────

/// SOL over every matching leg, and transactions (matching leg 0s), per side.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CopyTotals {
    pub buy_sol: f64,
    pub buy_txs: u32,
    pub sell_sol: f64,
    pub sell_txs: u32,
}

impl CopyTotals {
    fn add(&mut self, side: Side, sol: f64, first_leg: bool) {
        match side {
            Side::Buy => {
                self.buy_sol += sol;
                if first_leg {
                    self.buy_txs += 1;
                }
            }
            Side::Sell => {
                self.sell_sol += sol;
                if first_leg {
                    self.sell_txs += 1;
                }
            }
        }
    }

    fn sub(&mut self, side: Side, sol: f64, first_leg: bool) {
        match side {
            Side::Buy => {
                self.buy_sol -= sol;
                if first_leg {
                    self.buy_txs = self.buy_txs.saturating_sub(1);
                }
            }
            Side::Sell => {
                self.sell_sol -= sol;
                if first_leg {
                    self.sell_txs = self.sell_txs.saturating_sub(1);
                }
            }
        }
    }

    pub fn value(self, id: MetricId) -> f64 {
        use MetricId::*;
        match id {
            CopyBuySol | WinCopyBuySol => self.buy_sol,
            CopyBuyCount | WinCopyBuyCount => f64::from(self.buy_txs),
            CopySellSol | WinCopySellSol => self.sell_sol,
            CopySellCount | WinCopySellCount => f64::from(self.sell_txs),
            _ => f64::NAN,
        }
    }
}

// ── Window ───────────────────────────────────────────────────────────────────

/// One trailing window. Same O(1)-read shape as the flow windows — running totals
/// over the whole deque, corrected at the two out-of-window ends on read — but the
/// deque holds ONLY the target's prints, so a token he never touches carries an
/// empty one.
#[derive(Debug, Clone, PartialEq)]
struct CopyWindowState {
    spec: WindowSpec,
    /// `(pos, (side, sol, first_leg))`, oldest at front, position-sorted.
    buf: VecDeque<(i64, (Side, f64, bool))>,
    totals: CopyTotals,
}

impl CopyWindowState {
    fn new(spec: WindowSpec) -> Self {
        Self { spec, buf: VecDeque::new(), totals: CopyTotals::default() }
    }

    fn on_match(&mut self, side: Side, sol: f64, first_leg: bool, pos: i64, now_pos: i64) {
        push_sorted(&mut self.buf, pos, (side, sol, first_leg));
        self.totals.add(side, sol, first_leg);
        self.evict(now_pos);
    }

    fn evict(&mut self, now_pos: i64) {
        let (lo, _) = self.spec.bounds(now_pos);
        while let Some(&(pos, (side, sol, first_leg))) = self.buf.front() {
            if pos >= lo {
                break;
            }
            self.buf.pop_front();
            self.totals.sub(side, sol, first_leg);
        }
    }

    fn totals_at(&self, now_pos: i64) -> CopyTotals {
        let (lo, hi) = self.spec.bounds(now_pos);
        let mut out = self.totals;
        for &(pos, (side, sol, first_leg)) in self.buf.iter() {
            if pos >= lo {
                break;
            }
            out.sub(side, sol, first_leg);
        }
        for &(pos, (side, sol, first_leg)) in self.buf.iter().rev() {
            if pos <= hi {
                break;
            }
            out.sub(side, sol, first_leg);
        }
        out
    }
}

// ── State ────────────────────────────────────────────────────────────────────

/// Per-(token, fingerprint) copy state: the compiled list, lifetime totals, and one
/// deque per registered window.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyState {
    patterns: CopyPatterns,
    lifetime: CopyTotals,
    windows: BTreeMap<WindowKey, CopyWindowState>,
}

impl CopyState {
    pub fn new(patterns: CopyPatterns) -> Self {
        Self { patterns, lifetime: CopyTotals::default(), windows: BTreeMap::new() }
    }

    /// Adopt an edited list. Same contract as [`DumpState::set_patterns`]: trades
    /// already folded keep the verdict they were folded under (the totals are running
    /// sums and no trades are retained to redo), so swapping the target moves a live
    /// token's future, never its past.
    ///
    /// [`DumpState::set_patterns`]: super::dump_ix::DumpState::set_patterns
    pub fn set_patterns(&mut self, patterns: &CopyPatterns) {
        if &self.patterns != patterns {
            self.patterns = patterns.clone();
        }
    }

    pub fn ensure_window(&mut self, spec: WindowSpec) {
        self.windows.entry(spec.key()).or_insert_with(|| CopyWindowState::new(spec));
    }

    /// Fold one trade. A print from anyone else costs one hash-set lookup and
    /// nothing else — no push, no eviction, no allocation.
    pub fn on_trade(&mut self, t: &TradeLite, cur: Cursor) {
        if !t.sol.is_finite() || t.sol < 0.0 || !self.patterns.matches(t) {
            return;
        }
        let first_leg = t.leg_index == 0;
        self.lifetime.add(t.side, t.sol, first_leg);
        for w in self.windows.values_mut() {
            let pos = w.spec.pos(t.at, cur.at_trade(t));
            let now_pos = w.spec.now_pos(t.at, cur);
            w.on_match(t.side, t.sol, first_leg, pos, now_pos);
        }
    }

    /// How many of the target's prints one window still retains. Test-only: reads are
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

    /// One metric. `window: None` ⇒ the lifetime group (`m_copy`). An unregistered
    /// window reads `NaN` rather than a lifetime value, so a missing registration is
    /// loud instead of silently answering a trigger with a latch.
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
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    const HIM: &str = "8dtxTargetWalletAddress";
    const HER: &str = "SomeoneElseEntirely";

    fn t0() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn at(secs: f64) -> Ts {
        t0() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn c(slot: u64, print: u64) -> Cursor {
        Cursor { slot, print }
    }

    fn print(side: Side, who: &str, sol: f64, secs: f64, slot: u64, leg: u8) -> TradeLite {
        TradeLite {
            side,
            sol,
            at: at(secs),
            slot,
            leg_index: leg,
            wallet_hash: wallet_hash(who),
            ..Default::default()
        }
    }

    fn state() -> CopyState {
        CopyState::new(CopyPatterns::from_addresses([HIM]))
    }

    /// The whole group in one read: his SOL and his transactions, per side, and
    /// nobody else's.
    #[test]
    fn only_the_target_counts_and_both_sides_are_kept_apart() {
        let mut st = state();
        st.on_trade(&print(Side::Buy, HIM, 0.6, 1.0, 100, 0), c(100, 1));
        st.on_trade(&print(Side::Buy, HER, 9.0, 1.0, 100, 0), c(100, 2));
        st.on_trade(&print(Side::Sell, HIM, 0.4, 2.0, 102, 0), c(102, 3));

        let v = |id| st.value(id, None, at(2.0), c(102, 3));
        assert!((v(MetricId::CopyBuySol) - 0.6).abs() < 1e-9, "her 9 SOL is not his");
        assert_eq!(v(MetricId::CopyBuyCount), 1.0);
        assert!((v(MetricId::CopySellSol) - 0.4).abs() < 1e-9);
        assert_eq!(v(MetricId::CopySellCount), 1.0);
    }

    /// `buy_count` is a TRANSACTION count: one four-leg buy is one decision and all
    /// of its SOL. Sizing a copy off `buy_sol / buy_count` depends on this.
    #[test]
    fn a_four_leg_transaction_is_one_buy_and_all_of_its_sol() {
        let mut st = state();
        for leg in 0..4u8 {
            st.on_trade(&print(Side::Buy, HIM, 0.25, 1.0, 100, leg), c(100, 1));
        }
        assert_eq!(st.value(MetricId::CopyBuyCount, None, at(1.0), c(100, 1)), 1.0);
        assert!((st.value(MetricId::CopyBuySol, None, at(1.0), c(100, 1)) - 1.0).abs() < 1e-9);
    }

    /// The trigger contract. A `1p` window is THIS print alone, so three split buys
    /// are three separate fires — while the lifetime total latches after the first
    /// and would fire the rule on every later print of the token.
    #[test]
    fn a_one_print_window_is_this_print_and_the_lifetime_latches() {
        let w = WindowSpec::prints(1.0, 0.0);
        let mut st = state();
        st.ensure_window(w);

        let read =
            |st: &CopyState, n: u64| st.value(MetricId::WinCopyBuySol, Some(w), at(1.0), c(100, n));

        st.on_trade(&print(Side::Buy, HIM, 0.6, 1.0, 100, 0), c(100, 1));
        assert!((read(&st, 1) - 0.6).abs() < 1e-9, "this print is the window");
        st.on_trade(&print(Side::Buy, HIM, 0.3, 1.0, 100, 0), c(100, 2));
        assert!((read(&st, 2) - 0.3).abs() < 1e-9, "the second split is its own fire");
        // Someone else prints: the window releases, so no third fire.
        st.on_trade(&print(Side::Buy, HER, 5.0, 1.0, 100, 0), c(100, 3));
        assert!(read(&st, 3).abs() < 1e-9);
        // The lifetime latched on the first buy and never releases.
        assert_eq!(st.value(MetricId::CopyBuyCount, None, at(1.0), c(100, 3)), 2.0);
    }

    /// A `1sl` window sums his whole slot — the trigger a split burst needs, and the
    /// one that must release when the slot turns over.
    #[test]
    fn a_one_slot_window_sums_the_slot_and_then_releases() {
        let w = WindowSpec::slots(1.0, 0.0);
        let mut st = state();
        st.ensure_window(w);
        let read = |st: &CopyState, slot: u64| {
            st.value(MetricId::WinCopyBuySol, Some(w), at(1.0), c(slot, 0))
        };

        st.on_trade(&print(Side::Buy, HIM, 0.3, 1.0, 100, 0), c(100, 1));
        st.on_trade(&print(Side::Buy, HIM, 0.4, 1.0, 100, 0), c(100, 2));
        assert!((read(&st, 100) - 0.7).abs() < 1e-9, "a split burst is one slot's size");
        assert_eq!(st.value(MetricId::WinCopyBuyCount, Some(w), at(1.0), c(100, 0)), 2.0);
        assert!(read(&st, 101).abs() < 1e-9, "the next slot is not his burst");
    }

    /// A tick with no trade must decay the window, or a quiet token keeps answering
    /// with a fire that is minutes old.
    #[test]
    fn a_tick_decays_the_window() {
        let w = WindowSpec::secs(10.0);
        let mut st = state();
        st.ensure_window(w);
        st.on_trade(&print(Side::Buy, HIM, 0.6, 1.0, 100, 0), c(100, 1));
        assert_eq!(st.window_len(w), Some(1));
        st.on_tick(at(30.0), c(100, 1));
        assert_eq!(st.window_len(w), Some(0), "the tick never reached this group");
        assert!(st.value(MetricId::WinCopyBuySol, Some(w), at(30.0), c(100, 1)).abs() < 1e-9);
    }

    /// Unconfigured reads NaN, never 0 — `sell_count <= 0` on a zero would exit
    /// every position on a fingerprint that names no target at all.
    #[test]
    fn an_unconfigured_group_is_none_and_reads_nan() {
        assert!(CopyPatterns::from_metric_config(&json!({})).is_none());
        assert!(
            CopyPatterns::from_metric_config(&json!({"m_dump_ix": {"ix_patterns": []}})).is_none()
        );
        let cfg = json!({ CONFIG_KEY: { TARGETS_FIELD: [HIM] } });
        assert!(CopyPatterns::from_metric_config(&cfg).is_some());
    }

    /// An unregistered window reads NaN rather than silently answering with the
    /// lifetime latch, which is a much larger number that satisfies the bound.
    #[test]
    fn an_unregistered_window_reads_nan() {
        let mut st = state();
        st.on_trade(&print(Side::Buy, HIM, 0.6, 1.0, 100, 0), c(100, 1));
        let unregistered = WindowSpec::prints(1.0, 0.0);
        assert!(st
            .value(MetricId::WinCopyBuySol, Some(unregistered), at(1.0), c(100, 1))
            .is_nan());
        assert!((st.value(MetricId::CopyBuySol, None, at(1.0), c(100, 1)) - 0.6).abs() < 1e-9);
    }

    /// A print with no wallet column is not the target, however large. Offline that
    /// is the shape of a load that did not ask for wallet identity.
    #[test]
    fn an_anonymous_print_is_never_the_target() {
        let mut st = state();
        let mut anon = print(Side::Buy, HIM, 9.0, 1.0, 100, 0);
        anon.wallet_hash = 0;
        st.on_trade(&anon, c(100, 1));
        assert_eq!(st.value(MetricId::CopyBuySol, None, at(1.0), c(100, 1)), 0.0);
    }

    /// The list is addresses, and a base58 string is what a rule author has. The
    /// compiled form must be the same digest every adapter puts on the print.
    #[test]
    fn the_list_is_addresses_hashed_the_way_every_adapter_hashes_them() {
        let p = CopyPatterns::from_metric_config(&json!({ CONFIG_KEY: { TARGETS_FIELD: [HIM] } }))
            .expect("configured");
        assert!(p.matches(&print(Side::Buy, HIM, 1.0, 1.0, 100, 0)));
        assert!(!p.matches(&print(Side::Buy, HER, 1.0, 1.0, 100, 0)));
        assert_eq!(p, CopyPatterns::from_addresses([HIM]));
    }

    /// Swapping the target moves the token's future, not its past.
    #[test]
    fn an_edited_list_does_not_rewrite_folded_trades() {
        let mut st = state();
        st.on_trade(&print(Side::Buy, HIM, 0.6, 1.0, 100, 0), c(100, 1));
        st.set_patterns(&CopyPatterns::from_addresses([HER]));
        assert!((st.value(MetricId::CopyBuySol, None, at(1.0), c(100, 1)) - 0.6).abs() < 1e-9);
        st.on_trade(&print(Side::Buy, HIM, 5.0, 2.0, 101, 0), c(101, 2));
        assert!((st.value(MetricId::CopyBuySol, None, at(2.0), c(101, 2)) - 0.6).abs() < 1e-9);
        st.on_trade(&print(Side::Buy, HER, 1.0, 3.0, 102, 0), c(102, 3));
        assert!((st.value(MetricId::CopyBuySol, None, at(3.0), c(102, 3)) - 1.6).abs() < 1e-9);
    }

    #[test]
    fn shape_errors_are_rejected() {
        for bad in [
            json!({ CONFIG_KEY: [] }),
            json!({ CONFIG_KEY: {} }),
            json!({ CONFIG_KEY: { TARGETS_FIELD: "x" } }),
            json!({ CONFIG_KEY: { TARGETS_FIELD: [1] } }),
            json!({ CONFIG_KEY: { TARGETS_FIELD: ["  "] } }),
        ] {
            assert!(CopyPatterns::validate_metric_config(&bad).is_err(), "{bad}");
        }
        assert!(CopyPatterns::validate_metric_config(&json!({})).is_ok());
        assert!(CopyPatterns::validate_metric_config(
            &json!({ CONFIG_KEY: { TARGETS_FIELD: [HIM, HER] } })
        )
        .is_ok());
    }
}
