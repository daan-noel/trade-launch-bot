//! `m_crowd_after_age` — who has BOUGHT this token since it was `after_age_sec`
//! old, other than the creator (anchored metrics).
//!
//! * `non_creator_buyers` — distinct wallets, other than the creator, whose buy
//!   landed at or after the anchor; a count.
//! * `this_buyer_is_new` — 0/1: the print being folded is a buy that just added a
//!   wallet to that set. The arrival edge, so a rule can fire ONCE on the print
//!   where the N-th buyer shows up rather than on every print after it.
//!
//! **Anchored, not windowed.** `m_crowd_window` counts every trade over a trailing
//! span; this group counts buys over the token's life FROM an age anchor, which is a
//! basis no window can spell: a window ending at now always holds the launch
//! scramble as long as now is within it. The anchor is what lets a rule ask for the
//! second buyer *after* the scramble instead of the second buyer overall.
//!
//! **The creator is excluded by definition.** The creator's own launch buy is not
//! somebody arriving; `m_crowd_window.unique_wallets` is the count-everyone reading.
//!
//! **The set is capped, and the cap is derived.** A rule only ever asks whether the
//! count reached a threshold, so the state holds at most `cap` wallets, where `cap`
//! is the largest value any loaded condition names under this anchor, plus one —
//! computed at rule compile, never guessed. Every operator stays exact at the cap
//! (`>= 2` passes, `<= 5` fails at a cap of 6, `= 3` fails at 6), a hot token
//! drawing thousands of buyers costs a handful of entries, and the set is an inline
//! vector with a linear scan rather than a hash set: at a cap of three the scan is
//! cheaper than the hash. Once the set is closed [`this_buyer_is_new`] reads 0 — the
//! flag is exact while the set is open, which is every print up to the cap.
//!
//! No tick work: nothing here can move without a buy, so a settled token pays
//! nothing and the group declares no clock horizon.
//!
//! [`this_buyer_is_new`]: super::MetricId::ThisBuyerIsNew

use smallvec::SmallVec;

use super::{secs_between, MetricId, Side, TradeLite, Ts};

/// The strict param naming the anchor: buys before this age never enter the set.
pub const AFTER_AGE_PARAM: &str = "after_age_sec";

/// Dedup identity of one anchored aggregator — the age, in milliseconds. Two rules
/// on the same anchor share one set; two anchors are two sets. Carried on a
/// requirement's read scope ([`Windows::anchor`](super::Windows::anchor)) the way a
/// span is, so a requirement's identity includes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgeAnchor {
    pub after_age_ms: u64,
}

impl AgeAnchor {
    /// The anchor a rule spelled as `after_age_sec: N`.
    pub fn secs(after_age_sec: f64) -> Self {
        Self { after_age_ms: super::quantize(after_age_sec) }
    }

    /// The anchor as the seconds a rule author wrote.
    pub fn secs_f64(self) -> f64 {
        self.after_age_ms as f64 / 1000.0
    }

    /// Whether a print at `at` sits at or after the anchor for a token born at
    /// `created_at`.
    fn admits(self, created_at: Ts, at: Ts) -> bool {
        secs_between(created_at, at) * 1000.0 >= self.after_age_ms as f64
    }
}

/// How many wallet hashes live inline before the set spills. A cap of three is the
/// shape the shipping rule needs; four leaves room for `<= 3`.
const INLINE_BUYERS: usize = 4;

/// One anchored buyer set for a single [`AgeAnchor`].
#[derive(Debug, Clone)]
pub struct CrowdAfterAgeState {
    anchor: AgeAnchor,
    /// Largest number of wallets the set will ever hold — see the module docs.
    cap: u32,
    /// Distinct non-creator buyer wallet hashes since the anchor, insertion order.
    buyers: SmallVec<[u64; INLINE_BUYERS]>,
    /// Whether the print folded last added a wallet. Cleared by a tick, so a
    /// reading between prints never repeats the last arrival.
    last_print_added: bool,
}

impl CrowdAfterAgeState {
    pub fn new(anchor: AgeAnchor, cap: u32) -> Self {
        Self { anchor, cap: cap.max(1), buyers: SmallVec::new(), last_print_added: false }
    }

    pub fn anchor(&self) -> AgeAnchor {
        self.anchor
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Raise the cap. A reload that adds a rule naming a higher threshold on an
    /// anchor a live token already tracks must not leave that token's set closed
    /// below the new threshold; lowering never happens, because a closed set cannot
    /// be reopened truthfully.
    pub fn raise_cap(&mut self, cap: u32) {
        if cap > self.cap {
            self.cap = cap;
        }
    }

    /// Fold one print. `creator` is the creator wallet hash when known.
    pub fn on_trade(&mut self, t: &TradeLite, created_at: Ts, creator: Option<u64>) {
        self.last_print_added = false;
        if t.side != Side::Buy || !t.sol.is_finite() || t.sol < 0.0 {
            return;
        }
        if creator == Some(t.wallet_hash) || !self.anchor.admits(created_at, t.at) {
            return;
        }
        if self.buyers.len() as u32 >= self.cap {
            return;
        }
        if self.buyers.contains(&t.wallet_hash) {
            return;
        }
        self.buyers.push(t.wallet_hash);
        self.last_print_added = true;
    }

    /// A tick is not a print: the arrival flag is a property of the print folded
    /// last, and a reading on a tick must not repeat it.
    pub fn on_tick(&mut self) {
        self.last_print_added = false;
    }

    /// Value of one `m_crowd_after_age` metric.
    pub fn value(&self, id: MetricId) -> f64 {
        match id {
            MetricId::NonCreatorBuyers => self.buyers.len() as f64,
            MetricId::ThisBuyerIsNew => f64::from(u8::from(self.last_print_added)),
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn buy(wallet: u64, secs: f64) -> TradeLite {
        TradeLite { side: Side::Buy, sol: 0.5, wallet_hash: wallet, at: ts(secs), ..TradeLite::default() }
    }

    fn sell(wallet: u64, secs: f64) -> TradeLite {
        TradeLite { side: Side::Sell, ..buy(wallet, secs) }
    }

    const CREATOR: u64 = 99;

    #[test]
    fn counts_distinct_non_creator_buyers_from_the_anchor_only() {
        let mut s = CrowdAfterAgeState::new(AgeAnchor::secs(5.0), 3);
        let born = ts(0.0);
        // The launch scramble: creator and two wallets before the anchor. None count.
        s.on_trade(&buy(CREATOR, 0.1), born, Some(CREATOR));
        s.on_trade(&buy(1, 0.4), born, Some(CREATOR));
        s.on_trade(&buy(2, 3.9), born, Some(CREATOR));
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 0.0);
        // Exactly at the anchor counts; a sell never does; the creator never does.
        s.on_trade(&buy(1, 5.0), born, Some(CREATOR));
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 1.0);
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 1.0);
        s.on_trade(&sell(3, 6.0), born, Some(CREATOR));
        s.on_trade(&buy(CREATOR, 7.0), born, Some(CREATOR));
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 1.0);
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 0.0);
        // The same wallet again is not a new buyer; a second wallet is.
        s.on_trade(&buy(1, 8.0), born, Some(CREATOR));
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 0.0);
        s.on_trade(&buy(4, 9.0), born, Some(CREATOR));
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 2.0);
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 1.0);
    }

    #[test]
    fn a_tick_clears_the_arrival_flag() {
        let mut s = CrowdAfterAgeState::new(AgeAnchor::secs(0.0), 3);
        s.on_trade(&buy(1, 1.0), ts(0.0), None);
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 1.0);
        s.on_tick();
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 0.0);
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 1.0);
    }

    #[test]
    fn the_set_closes_at_the_cap_and_reports_the_cap() {
        let mut s = CrowdAfterAgeState::new(AgeAnchor::secs(0.0), 3);
        for w in 1..=50u64 {
            s.on_trade(&buy(w, w as f64), ts(0.0), None);
        }
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 3.0);
        assert_eq!(s.buyers.len(), 3);
        // A closed set cannot tell whether the 50th wallet was new, so it says no.
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 0.0);
        // Raising the cap re-opens it for the NEXT arrivals only.
        s.raise_cap(5);
        s.on_trade(&buy(77, 60.0), ts(0.0), None);
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 4.0);
        assert_eq!(s.value(MetricId::ThisBuyerIsNew), 1.0);
        s.raise_cap(2);
        assert_eq!(s.cap(), 5, "a cap is never lowered");
    }

    #[test]
    fn an_unknown_creator_excludes_nobody() {
        let mut s = CrowdAfterAgeState::new(AgeAnchor::secs(0.0), 3);
        s.on_trade(&buy(CREATOR, 1.0), ts(0.0), None);
        assert_eq!(s.value(MetricId::NonCreatorBuyers), 1.0);
    }

    #[test]
    fn the_anchor_is_millisecond_exact() {
        let a = AgeAnchor::secs(5.0);
        assert!(!a.admits(ts(0.0), ts(4.999)));
        assert!(a.admits(ts(0.0), ts(5.0)));
        assert_eq!(a.secs_f64(), 5.0);
    }
}
