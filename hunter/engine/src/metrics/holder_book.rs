//! `m_holder_book` — who holds this token's supply (static metrics).
//!
//! An exact per-wallet token book: each wallet's tokens bought minus sold on this
//! token, every leg, never below zero ([`TradeLite::token_amount`]). Live supply is
//! the sum of the bags. Two shares of it, both read after the print is folded:
//!
//! * `public_app_share` — held by wallets whose first buy of this token used a
//!   PUBLIC-APP build: a build recipe more than [`PUBLIC_MIN_BUYERS`] distinct
//!   wallets bought with, on any token, on the UTC day before that buy
//!   ([`TradeLite::build_day_buyers`], stamped by `reduce` from the daily
//!   build-breadth table).
//! * `bundled_share` — held by wallets whose first buy of this token landed in a
//!   slot where at least [`BUNDLE_MIN_WALLETS`] wallets made their first buy of it
//!   with the same build recipe: one trigger behind many wallets.
//!
//! The hot-tape loss door (evidence 1.28): supply a private bot holds, or a group
//! that bought together holds, leaves together, and the price never shows it.
//!
//! **A holder's class is fixed at its first buy.** The public test reads the table
//! that stood at that buy, so a reload changes no tracked token's reading, and the
//! value moves only on this token's own prints (no cross-epoch bump).
//!
//! **Opened only when a loaded rule reads it.** The book holds one entry per wallet
//! that ever bought the token; a track opens it through
//! [`TokenTrack::ensure_holder_book`](super::track::TokenTrack::ensure_holder_book)
//! only when some rule names the group. It needs the wallet and label columns
//! ([`MetricId::needs_wallet_identity`], [`MetricId::needs_ix_labels`]).

use std::collections::HashMap;

use crate::hash::HashedMap;

use super::{MetricId, Side, TradeLite};

/// A build is a public app when MORE than this many distinct wallets bought with it
/// on the previous UTC day. Private bots sit far below it, public apps far above.
pub const PUBLIC_MIN_BUYERS: u32 = 100;

/// A first-buy group is bundled at this many wallets in one (slot, build).
pub const BUNDLE_MIN_WALLETS: u32 = 3;

/// One wallet's bag and the class its first buy gave it.
#[derive(Debug, Clone, Copy)]
struct Bag {
    /// Tokens held, raw units; an exact integer in `f64`.
    tokens: f64,
    /// Index into `groups` once the wallet has bought; `None` before.
    group: Option<u32>,
    /// `Some(true)` public app, `Some(false)` not, `None` stamped without a table.
    public: Option<bool>,
}

/// Wallets whose first buy shared one (slot, build).
#[derive(Debug, Clone, Copy, Default)]
struct Group {
    n: u32,
    /// The members before the group reached [`BUNDLE_MIN_WALLETS`], whose bags join
    /// the bundled sum at that crossing.
    early: [u64; (BUNDLE_MIN_WALLETS - 1) as usize],
}

/// One token's holder book and its running sums.
#[derive(Debug, Clone, Default)]
pub struct HolderBookState {
    bags: HashedMap<Bag>,
    groups: Vec<Group>,
    group_of: HashMap<(u64, Option<u64>), u32>,
    /// Sum of every bag.
    live: f64,
    /// Sum of the bags of public-app first buyers.
    public: f64,
    /// Sum of the bags of first buyers stamped with no table loaded.
    unknown: f64,
    /// Sum of the bags of wallets in a bundled group.
    bundled: f64,
    /// A print arrived without a token amount: the book no longer adds up.
    broken: bool,
}

impl HolderBookState {
    /// Fold one print: move the wallet's bag, then class a first buy.
    pub fn on_trade(&mut self, t: &TradeLite) {
        if self.broken {
            return;
        }
        if !(t.token_amount.is_finite() && t.token_amount >= 0.0) {
            self.broken = true;
            return;
        }
        let first_buy = t.side == Side::Buy && self.bags.get(&t.wallet_hash).is_none_or(|b| b.group.is_none());
        if t.side == Side::Sell && !self.bags.contains_key(&t.wallet_hash) {
            return; // a wallet that never bought holds nothing to sell here
        }
        let signed = if t.side == Side::Buy { t.token_amount } else { -t.token_amount };
        let bag = self
            .bags
            .entry(t.wallet_hash)
            .or_insert(Bag { tokens: 0.0, group: None, public: None });
        let new = (bag.tokens + signed).max(0.0);
        let delta = new - bag.tokens;
        bag.tokens = new;
        let (group, public) = (bag.group, bag.public);
        self.live += delta;
        if let Some(g) = group {
            match public {
                Some(true) => self.public += delta,
                None => self.unknown += delta,
                Some(false) => {}
            }
            if self.groups[g as usize].n >= BUNDLE_MIN_WALLETS {
                self.bundled += delta;
            }
        }
        if first_buy {
            self.class_first_buy(t, new);
        }
    }

    fn class_first_buy(&mut self, t: &TradeLite, tokens: f64) {
        let public = t.build_day_buyers.map(|n| n > PUBLIC_MIN_BUYERS);
        match public {
            Some(true) => self.public += tokens,
            None => self.unknown += tokens,
            Some(false) => {}
        }
        let next = self.groups.len() as u32;
        let g = *self.group_of.entry((t.slot, t.build_hash)).or_insert(next);
        if g == next {
            self.groups.push(Group::default());
        }
        let group = &mut self.groups[g as usize];
        group.n += 1;
        let n = group.n;
        if n < BUNDLE_MIN_WALLETS {
            group.early[(n - 1) as usize] = t.wallet_hash;
        } else if n == BUNDLE_MIN_WALLETS {
            let early = group.early;
            self.bundled += tokens + early.iter().map(|w| self.bags[w].tokens).sum::<f64>();
        } else {
            self.bundled += tokens;
        }
        let bag = self.bags.get_mut(&t.wallet_hash).expect("folded above");
        bag.group = Some(g);
        bag.public = public;
    }

    /// Value of one `m_holder_book` metric, in percent of live supply. `NaN` when no
    /// wallet holds tokens or the book lost a print's amount; `public_app_share` is
    /// also `NaN` while any held supply was classed with no table loaded.
    pub fn value(&self, id: MetricId) -> f64 {
        if self.broken || self.live <= 0.0 {
            return f64::NAN;
        }
        match id {
            MetricId::PublicAppShare if self.unknown > 0.0 => f64::NAN,
            MetricId::PublicAppShare => self.public / self.live * 100.0,
            MetricId::BundledShare => self.bundled / self.live * 100.0,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(side: Side, wallet: u64, tokens: f64, slot: u64, build: u64, buyers: Option<u32>) -> TradeLite {
        TradeLite {
            side,
            sol: 1.0,
            wallet_hash: wallet,
            token_amount: tokens,
            slot,
            build_hash: Some(build),
            build_day_buyers: buyers,
            ..TradeLite::default()
        }
    }

    const PUB: Option<u32> = Some(PUBLIC_MIN_BUYERS + 1);
    const BOT: Option<u32> = Some(PUBLIC_MIN_BUYERS);

    #[test]
    fn public_share_is_the_supply_public_first_buyers_hold() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 700.0, 10, 7, PUB));
        b.on_trade(&print(Side::Buy, 2, 300.0, 11, 8, BOT));
        assert_eq!(b.value(MetricId::PublicAppShare), 70.0);
        // The public buyer sells half: 350 of 650.
        b.on_trade(&print(Side::Sell, 1, 350.0, 12, 7, PUB));
        assert!((b.value(MetricId::PublicAppShare) - 350.0 / 650.0 * 100.0).abs() < 1e-12);
    }

    #[test]
    fn a_holders_class_is_fixed_at_its_first_buy() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, BOT));
        // A later buy through a public app does not reclass the wallet.
        b.on_trade(&print(Side::Buy, 1, 100.0, 11, 9, PUB));
        assert_eq!(b.value(MetricId::PublicAppShare), 0.0);
    }

    #[test]
    fn three_first_buys_in_one_slot_and_build_are_bundled() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, BOT));
        b.on_trade(&print(Side::Buy, 2, 100.0, 10, 7, BOT));
        b.on_trade(&print(Side::Buy, 9, 200.0, 10, 8, BOT)); // same slot, other build
        assert_eq!(b.value(MetricId::BundledShare), 0.0);
        b.on_trade(&print(Side::Buy, 3, 100.0, 10, 7, BOT));
        assert_eq!(b.value(MetricId::BundledShare), 60.0);
        b.on_trade(&print(Side::Buy, 4, 100.0, 10, 7, BOT));
        assert!((b.value(MetricId::BundledShare) - 400.0 / 600.0 * 100.0).abs() < 1e-12);
        // A member's sell leaves the bundled sum with its tokens.
        b.on_trade(&print(Side::Sell, 1, 100.0, 11, 5, BOT));
        assert_eq!(b.value(MetricId::BundledShare), 60.0);
    }

    #[test]
    fn a_bag_never_goes_below_zero_and_an_empty_book_is_nan() {
        let mut b = HolderBookState::default();
        assert!(b.value(MetricId::BundledShare).is_nan());
        b.on_trade(&print(Side::Sell, 5, 50.0, 9, 7, BOT)); // never bought: nothing
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, PUB));
        b.on_trade(&print(Side::Sell, 1, 150.0, 11, 7, PUB));
        assert!(b.value(MetricId::PublicAppShare).is_nan());
        b.on_trade(&print(Side::Buy, 2, 40.0, 12, 8, BOT));
        assert_eq!(b.value(MetricId::PublicAppShare), 0.0);
    }

    #[test]
    fn no_table_at_a_first_buy_reads_nan_while_that_supply_is_held() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, None));
        b.on_trade(&print(Side::Buy, 2, 100.0, 11, 8, PUB));
        assert!(b.value(MetricId::PublicAppShare).is_nan());
        assert_eq!(b.value(MetricId::BundledShare), 0.0);
        b.on_trade(&print(Side::Sell, 1, 100.0, 12, 7, None));
        assert_eq!(b.value(MetricId::PublicAppShare), 100.0);
    }

    #[test]
    fn a_print_without_a_token_amount_breaks_the_book() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, PUB));
        b.on_trade(&print(Side::Buy, 2, f64::NAN, 11, 7, PUB));
        assert!(b.value(MetricId::PublicAppShare).is_nan());
        assert!(b.value(MetricId::BundledShare).is_nan());
    }
}
