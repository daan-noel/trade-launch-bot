//! The volume tag's live label. Dump and every other tag never build one.
//!
//! A loaded rule that does not read `@volume` never opens this state. A print the
//! loaded fingerprint already matches does not reach [`VolumeWalk::label`]: it only
//! updates the counters. A yes sticks the wallet forward. A list is queued by hash;
//! the live side writes the label text off the decision loop. A this-coin wallet is
//! not queued.
//!
//! Market lookups stay unmatched from here: sniper, timer, wide service, service
//! payer, first trade, never sells, stays in the group, mostly here, and the gate.
//! Each is a later function. Rise has no number here, so it stays unmatched too.

use crate::hash::HashedSet;
use crate::metrics::trade_keys::ROUTER_MARKERS;
use crate::metrics::{Side, TradeLite};

/// Buys this close, and inside this span, are one pack.
const PACK_MS: i64 = 100;
/// A buy this soon after the create is not a person who saw the coin.
const INSTANT_MS: i64 = 15;
/// Many wallets: this many trades, from more than one wallet.
const MANY: u32 = 20;
/// Dominates: this many trades, and at least a fifth of the coin's prints.
const DOM: u32 = 10;
const RING: usize = 32;
const IX_CAP: usize = 32;
const SLOT_CAP: usize = 12;
const BAG_CAP: usize = 256;
const PENDING_CAP: usize = 8;
/// AdvanceNonceAccount | CreateAccountWithSeed.
const PRIVATE: u16 = (1 << 0) | (1 << 1);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Print {
    at_ms: i64,
    slot: u64,
    tx: u32,
    has_tx: bool,
    wallet: u64,
    ix: u64,
    bits: u16,
    buy: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct IxStat {
    used: bool,
    hash: u64,
    trades: u32,
    w0: u64,
    w1: u64,
    bits: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Bag {
    buy: f64,
    sell: f64,
}

/// What the label decided. Partners join the sticky set; the wallet of this print
/// joins it in `TagState` when `owner` is set.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Verdict {
    pub owner: bool,
    pub partners: [u64; 8],
    pub n: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VolumeWalk {
    created_ms: Option<i64>,
    slot: u64,
    slot_set: bool,
    counts: [(u64, u8); SLOT_CAP],
    n_slot: u8,
    ring: [Print; RING],
    ring_len: u8,
    ring_next: u8,
    ix: [IxStat; IX_CAP],
    total: u32,
    bags: crate::hash::HashedMap<Bag>,
    pending: [u64; PENDING_CAP],
    pending_n: u8,
    seen: [u64; 16],
    seen_n: u8,
}

impl Default for VolumeWalk {
    fn default() -> Self {
        Self {
            created_ms: None,
            slot: 0,
            slot_set: false,
            counts: [(0, 0); SLOT_CAP],
            n_slot: 0,
            ring: [Print::default(); RING],
            ring_len: 0,
            ring_next: 0,
            ix: [IxStat::default(); IX_CAP],
            total: 0,
            bags: crate::hash::HashedMap::default(),
            pending: [0; PENDING_CAP],
            pending_n: 0,
            seen: [0; 16],
            seen_n: 0,
        }
    }
}

impl VolumeWalk {
    pub(crate) fn seed_born(&mut self, created_ms: i64) {
        if self.created_ms.is_none() {
            self.created_ms = Some(created_ms);
        }
    }

    /// Counters for every print, including one the loaded fingerprint already matched.
    pub(crate) fn observe(&mut self, t: &TradeLite) {
        self.total = self.total.saturating_add(1);
        self.note_slot(t);
        self.note_ix(t);
        self.note_bag(t);
    }

    pub(crate) fn push(&mut self, t: &TradeLite) {
        self.ring[self.ring_next as usize] = Print {
            at_ms: t.at.timestamp_millis(),
            slot: t.slot,
            tx: t.tx_index.unwrap_or(0),
            has_tx: t.tx_index.is_some(),
            wallet: t.wallet_hash,
            ix: t.ix_hash.unwrap_or(0),
            bits: t.marker_bits,
            buy: t.side == Side::Buy,
        };
        self.ring_next = (self.ring_next + 1) % RING as u8;
        if (self.ring_len as usize) < RING {
            self.ring_len += 1;
        }
    }

    /// Same transaction as a wallet already sticky. The current print is not in the ring yet.
    pub(crate) fn beside(&self, t: &TradeLite, sticky: &HashedSet) -> bool {
        let Some(tx) = t.tx_index else { return false };
        if t.wallet_hash == 0 {
            return false;
        }
        for i in 0..self.ring_len as usize {
            let p = self.print_at(i);
            if p.has_tx
                && p.slot == t.slot
                && p.tx == tx
                && p.wallet != t.wallet_hash
                && sticky.contains(&p.wallet)
            {
                return true;
            }
        }
        false
    }

    /// Owner, and maybe a list to write. `beside` was read before [`observe`](Self::observe).
    pub(crate) fn label(&mut self, t: &TradeLite, creator: Option<u64>, beside: bool) -> Verdict {
        if outsider_stop(t) {
            return Verdict::default();
        }
        let mut v = Verdict::default();
        if self.this_coin(t, creator, beside, &mut v) {
            v.owner = true;
        }
        if let Some(ix) = self.list_rule(t) {
            v.owner = true;
            self.queue(ix);
        }
        v
    }

    pub(crate) fn take_pending(&mut self) -> (u8, [u64; PENDING_CAP]) {
        let n = self.pending_n;
        self.pending_n = 0;
        (n, self.pending)
    }

    fn this_coin(
        &self,
        t: &TradeLite,
        creator: Option<u64>,
        beside: bool,
        v: &mut Verdict,
    ) -> bool {
        let mut yes = false;
        if t.side == Side::Buy
            && t.is_launch
            && t.wallet_hash != 0
            && Some(t.wallet_hash) != creator
        {
            yes = true;
        }
        if t.side == Side::Buy && Some(t.wallet_hash) != creator {
            if let Some(born) = self.created_ms {
                let dt = t.at.timestamp_millis().saturating_sub(born);
                if (0..=INSTANT_MS).contains(&dt) {
                    yes = true;
                }
            }
        }
        if self.slot_set && t.slot == self.slot && t.side == Side::Buy {
            let n = self.slot_count(t.wallet_hash);
            if n >= 2 {
                yes = true;
            }
            if self.n_slot >= 3 {
                yes = true;
                for i in 0..self.n_slot as usize {
                    let w = self.counts[i].0;
                    if w != t.wallet_hash {
                        add_partner(v, w);
                    }
                }
            }
        }
        if self.several_sells(t, v) || self.several_lists(t, v) || beside {
            yes = true;
        }
        if t.side == Side::Sell && self.bag_rules(t, v) {
            yes = true;
        }
        yes
    }

    fn list_rule(&self, t: &TradeLite) -> Option<u64> {
        let ix = t.ix_hash?;
        let s = self.ix.iter().find(|s| s.used && s.hash == ix)?;
        if s.bits == 0 || s.w1 == 0 || s.w1 == s.w0 {
            return None;
        }
        // A private list two wallets used inside the recent window. A bare public verb
        // (no marker bit) never reaches here.
        if s.bits & PRIVATE != 0 && self.private_in_window(t, ix) {
            return Some(ix);
        }
        if s.bits & ROUTER_MARKERS != 0 && s.trades >= MANY {
            return Some(ix);
        }
        if s.trades >= DOM && self.total > 0 && s.trades.saturating_mul(5) >= self.total {
            return Some(ix);
        }
        None
    }

    fn private_in_window(&self, t: &TradeLite, ix: u64) -> bool {
        if t.side != Side::Buy || t.marker_bits & PRIVATE == 0 {
            return false;
        }
        let mut n = 1u8;
        let mut wallets = [t.wallet_hash, 0, 0, 0];
        for i in 0..self.ring_len as usize {
            let p = self.print_at(i);
            if !p.buy || p.ix != ix || !in_window(t, p) {
                continue;
            }
            if p.wallet != 0
                && !wallets[..n as usize].contains(&p.wallet)
                && (n as usize) < wallets.len()
            {
                wallets[n as usize] = p.wallet;
                n += 1;
            }
        }
        n >= 2
    }

    fn several_lists(&self, t: &TradeLite, v: &mut Verdict) -> bool {
        if t.side != Side::Buy {
            return false;
        }
        let Some(my) = t.ix_hash else { return false };
        let mut ixs = [my, 0, 0, 0];
        let mut n = 1u8;
        let mut privy = t.marker_bits & PRIVATE != 0;
        let mut wallets = [0u64; 8];
        let mut nw = 0u8;
        for i in 0..self.ring_len as usize {
            let p = self.print_at(i);
            if !p.buy || !in_window(t, p) {
                continue;
            }
            if p.bits & PRIVATE != 0 {
                privy = true;
            }
            if p.ix != 0 && !ixs[..n as usize].contains(&p.ix) && (n as usize) < ixs.len() {
                ixs[n as usize] = p.ix;
                n += 1;
            }
            if p.wallet != t.wallet_hash
                && (nw as usize) < wallets.len()
                && !wallets[..nw as usize].contains(&p.wallet)
            {
                wallets[nw as usize] = p.wallet;
                nw += 1;
            }
        }
        if !(privy && n >= 2) {
            return false;
        }
        for w in &wallets[..nw as usize] {
            add_partner(v, *w);
        }
        true
    }

    fn several_sells(&self, t: &TradeLite, v: &mut Verdict) -> bool {
        if t.side != Side::Sell {
            return false;
        }
        let Some(tx) = t.tx_index else { return false };
        let mut yes = false;
        for i in 0..self.ring_len as usize {
            let p = self.print_at(i);
            if !p.buy && p.has_tx && p.slot == t.slot && p.tx == tx && p.wallet != t.wallet_hash {
                add_partner(v, p.wallet);
                yes = true;
            }
        }
        yes
    }

    fn bag_rules(&self, t: &TradeLite, v: &mut Verdict) -> bool {
        let Some(mine) = self.bags.get(&t.wallet_hash) else {
            return false;
        };
        let mut yes = false;
        if mine.buy > 0.0 && (mine.sell - mine.buy) * 100.0 > mine.sell {
            yes = true;
            if let Some(w) = self.one_holder(t.wallet_hash, mine.sell - mine.buy) {
                add_partner(v, w);
            }
        }
        if mine.buy == 0.0 && mine.sell > 0.0 {
            if let Some(w) = self.one_buy_only(t.wallet_hash, t.token_amount) {
                add_partner(v, w);
                yes = true;
            }
        }
        yes
    }

    /// Exactly one other wallet holds the shortfall, within 0.5%.
    fn one_holder(&self, self_w: u64, shortfall: f64) -> Option<u64> {
        let mut found = None;
        for (&w, b) in &self.bags {
            if w == self_w {
                continue;
            }
            let held = b.buy - b.sell;
            if held > 0.0 && close(held, shortfall) {
                if found.is_some() {
                    return None;
                }
                found = Some(w);
            }
        }
        found
    }

    /// Exactly one wallet that bought this token amount and has not sold.
    fn one_buy_only(&self, self_w: u64, amount: f64) -> Option<u64> {
        if !(amount > 0.0) {
            return None;
        }
        let mut found = None;
        for (&w, b) in &self.bags {
            if w == self_w || b.sell != 0.0 || b.buy <= 0.0 {
                continue;
            }
            if close(b.buy, amount) {
                if found.is_some() {
                    return None;
                }
                found = Some(w);
            }
        }
        found
    }

    fn note_slot(&mut self, t: &TradeLite) {
        if !self.slot_set && t.is_launch {
            self.slot = t.slot;
            self.slot_set = true;
        }
        if !self.slot_set || t.slot != self.slot || t.side != Side::Buy || t.wallet_hash == 0 {
            return;
        }
        if let Some(c) = self
            .counts
            .iter_mut()
            .take(self.n_slot as usize)
            .find(|c| c.0 == t.wallet_hash)
        {
            c.1 = c.1.saturating_add(1);
            return;
        }
        if (self.n_slot as usize) < SLOT_CAP {
            self.counts[self.n_slot as usize] = (t.wallet_hash, 1);
            self.n_slot += 1;
        }
    }

    fn note_ix(&mut self, t: &TradeLite) {
        let Some(ix) = t.ix_hash else { return };
        if let Some(s) = self.ix.iter_mut().find(|s| s.used && s.hash == ix) {
            s.trades = s.trades.saturating_add(1);
            if t.wallet_hash != 0 && t.wallet_hash != s.w0 && s.w1 == 0 {
                s.w1 = t.wallet_hash;
            }
            return;
        }
        if let Some(s) = self.ix.iter_mut().find(|s| !s.used) {
            *s = IxStat {
                used: true,
                hash: ix,
                trades: 1,
                w0: t.wallet_hash,
                w1: 0,
                bits: t.marker_bits,
            };
        }
    }

    fn note_bag(&mut self, t: &TradeLite) {
        if t.wallet_hash == 0 || !t.token_amount.is_finite() || t.token_amount < 0.0 {
            return;
        }
        if !self.bags.contains_key(&t.wallet_hash) && self.bags.len() >= BAG_CAP {
            return;
        }
        let b = self.bags.entry(t.wallet_hash).or_default();
        match t.side {
            Side::Buy => b.buy += t.token_amount,
            Side::Sell => b.sell += t.token_amount,
        }
    }

    fn slot_count(&self, wallet: u64) -> u8 {
        self.counts
            .iter()
            .take(self.n_slot as usize)
            .find(|c| c.0 == wallet)
            .map(|c| c.1)
            .unwrap_or(0)
    }

    fn print_at(&self, i: usize) -> &Print {
        let n = self.ring_len as usize;
        let start = (self.ring_next as usize + RING - n) % RING;
        &self.ring[(start + i) % RING]
    }

    fn queue(&mut self, ix: u64) {
        if ix == 0 || self.seen[..self.seen_n as usize].contains(&ix) {
            return;
        }
        if (self.seen_n as usize) >= self.seen.len() || (self.pending_n as usize) >= PENDING_CAP {
            return;
        }
        self.seen[self.seen_n as usize] = ix;
        self.seen_n += 1;
        self.pending[self.pending_n as usize] = ix;
        self.pending_n += 1;
    }
}

fn in_window(t: &TradeLite, p: &Print) -> bool {
    let at = t.at.timestamp_millis();
    at >= p.at_ms && at.saturating_sub(p.at_ms) <= PACK_MS
}

fn add_partner(v: &mut Verdict, wallet: u64) {
    if wallet == 0 || v.n as usize >= v.partners.len() {
        return;
    }
    if v.partners[..v.n as usize].contains(&wallet) {
        return;
    }
    v.partners[v.n as usize] = wallet;
    v.n += 1;
}

fn close(a: f64, b: f64) -> bool {
    b > 0.0 && (a - b).abs() * 1000.0 <= b * 5.0
}

/// Outsider stops that need a market lookup. A missing lookup leaves the print
/// unmatched here, so a this-coin or list rule can still name it.
fn outsider_stop(_t: &TradeLite) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::tags::config::compile_tags;
    use crate::metrics::tags::state::TagState;
    use crate::metrics::{Cursor, TradeLite};
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    fn ts(ms: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds(ms)
    }

    fn buy(wallet: u64, ix: u64, ms: i64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol: 0.1,
            price: 1.0,
            at: ts(ms),
            ix_hash: Some(ix),
            wallet_hash: wallet,
            slot: 10,
            token_amount: 1.0,
            ..Default::default()
        }
    }

    fn step(w: &mut VolumeWalk, t: &TradeLite, creator: Option<u64>) -> Verdict {
        let beside = false;
        w.observe(t);
        let v = w.label(t, creator, beside);
        w.push(t);
        v
    }

    /// The creator's create transaction names the slot. A later buy in that slot,
    /// from one new wallet, is not a bundle and is not a second buy.
    #[test]
    fn one_creation_slot_buy_stays_outsider() {
        let mut w = VolumeWalk::default();
        let mut create = buy(1, 7, 0);
        create.is_launch = true;
        create.slot = 3;
        assert!(!step(&mut w, &create, Some(1)).owner);
        let mut t = buy(2, 8, 20);
        t.slot = 3;
        assert!(!step(&mut w, &t, Some(1)).owner);
    }

    #[test]
    fn third_creation_slot_wallet_is_owner_and_names_the_others() {
        let mut w = VolumeWalk::default();
        let mut create = buy(1, 7, 0);
        create.is_launch = true;
        create.slot = 3;
        assert!(!step(&mut w, &create, Some(1)).owner);
        let mut b = buy(2, 8, 20);
        b.slot = 3;
        assert!(!step(&mut w, &b, Some(1)).owner);
        let mut c = buy(3, 8, 40);
        c.slot = 3;
        let v = step(&mut w, &c, Some(1));
        assert!(v.owner);
        assert!(v.partners[..v.n as usize].contains(&1));
        assert!(v.partners[..v.n as usize].contains(&2));
    }

    #[test]
    fn a_second_buy_in_the_creation_slot_is_owner() {
        let mut w = VolumeWalk::default();
        let mut create = buy(9, 7, 0);
        create.is_launch = true;
        create.slot = 3;
        assert!(!step(&mut w, &create, Some(9)).owner);
        let mut a = buy(1, 8, 10);
        a.slot = 3;
        assert!(!step(&mut w, &a, Some(9)).owner);
        let mut b = buy(1, 8, 30);
        b.slot = 3;
        assert!(step(&mut w, &b, Some(9)).owner);
    }

    #[test]
    fn a_buy_within_15ms_is_owner() {
        let mut w = VolumeWalk::default();
        w.seed_born(ts(0).timestamp_millis());
        let t = buy(4, 1, 10);
        assert!(step(&mut w, &t, None).owner);
    }

    #[test]
    fn a_private_list_two_wallets_use_is_queued() {
        let mut w = VolumeWalk::default();
        let mut a = buy(1, 42, 0);
        a.marker_bits = PRIVATE;
        assert!(!step(&mut w, &a, None).owner);
        assert_eq!(w.take_pending().0, 0);
        let mut b = buy(2, 42, 2);
        b.marker_bits = PRIVATE;
        let v = step(&mut w, &b, None);
        assert!(v.owner);
        let (n, pending) = w.take_pending();
        assert_eq!(pending[..n as usize], [42]);
    }

    #[test]
    fn a_plain_verb_is_not_queued_at_20_trades() {
        let mut w = VolumeWalk::default();
        for i in 0..20 {
            let t = buy(if i == 0 { 1 } else { 2 }, 9, i);
            let _ = step(&mut w, &t, None);
        }
        assert_eq!(w.take_pending().0, 0);
    }

    #[test]
    fn a_router_list_at_20_trades_is_queued_once() {
        let mut w = VolumeWalk::default();
        for i in 0..20 {
            let mut t = buy(if i == 0 { 1 } else { 2 }, 9, i * 1000);
            t.marker_bits = 1 << 5;
            let _ = step(&mut w, &t, None);
        }
        let (n, pending) = w.take_pending();
        assert_eq!(pending[..n as usize], [9]);
        let mut again = buy(3, 9, 30_000);
        again.marker_bits = 1 << 5;
        assert!(step(&mut w, &again, None).owner);
        assert_eq!(w.take_pending().0, 0);
    }

    #[test]
    fn same_amount_names_the_buyer() {
        let mut w = VolumeWalk::default();
        let mut buy_t = buy(1, 3, 0);
        buy_t.token_amount = 1000.0;
        assert!(!step(&mut w, &buy_t, None).owner);
        let mut sell = buy(2, 4, 50);
        sell.side = Side::Sell;
        sell.token_amount = 1000.0;
        let v = step(&mut w, &sell, None);
        assert!(v.owner);
        assert_eq!(v.partners[..v.n as usize], [1]);
        assert_eq!(w.take_pending().0, 0);
    }

    #[test]
    fn dump_does_not_walk() {
        let tags = compile_tags(&json!({"dump": {"match": {"ix_shape": [["S"]]}, "side": "sell"}}));
        let mut st = TagState::new(tags[0].patterns.clone());
        let mut a = buy(1, 42, 0);
        a.marker_bits = PRIVATE;
        a.side = Side::Sell;
        let mut b = a;
        b.wallet_hash = 2;
        b.at = ts(2);
        let cur = Cursor { slot: 10, print: 0 };
        assert!(!st.on_trade(&a, cur));
        assert!(!st.on_trade(&b, cur));
        assert_eq!(st.take_volume_pending().0, 0);
    }

    #[test]
    fn volume_sticks_the_wallet_and_queues_the_list() {
        let tags = compile_tags(&json!({"volume": {"match": {"creator": true}, "sticky": true}}));
        assert!(tags[0].patterns.volume_walk);
        let mut st = TagState::new(tags[0].patterns.clone());
        let mut a = buy(1, 42, 0);
        a.marker_bits = PRIVATE;
        let mut b = a;
        b.wallet_hash = 2;
        b.at = ts(2);
        let cur = Cursor { slot: 10, print: 0 };
        assert!(!st.on_trade(&a, cur));
        assert!(st.on_trade(&b, cur));
        let (n, pending) = st.take_volume_pending();
        assert_eq!(pending[..n as usize], [42]);
        let later = buy(2, 99, 500);
        assert!(
            st.on_trade(&later, cur),
            "sticky covers the wallet's next list"
        );
    }
}
