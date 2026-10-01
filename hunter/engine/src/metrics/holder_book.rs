//! The per-wallet holder book behind `m_holdings.bag_share_pct @bundled` /
//! `@public_app`: who holds this coin's supply.
//!
//! An exact per-wallet token book: each wallet's tokens bought minus sold on this
//! coin, every leg, never below zero ([`TradeLite::token_amount`]). Live supply is
//! the sum of the bags. Two wallet classes share it, both read after the print is folded:
//!
//! * `@public_app` — held by wallets whose first buy of this token went through
//!   a PUBLIC APP ([`is_public_app`]): on the UTC day before that buy, the app had
//!   more than [`PUBLIC_MIN_BUYERS`] distinct buying wallets and they came back, at
//!   least [`PUBLIC_MIN_REPEAT`] buys per wallet ([`TradeLite::build_day_public`],
//!   stamped by `reduce` from the daily build-breadth table). A bot swarm spreads
//!   thousands of wallets over many programs at about one buy per wallet per program
//!   a day; the named apps run 2.9 and up (hot-tape case file L12).
//! * `@bundled` — held by wallets whose first buy of this token landed in a
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
//! **The biggest holders** ([`TopHolders`]): the book also ranks the bags by size and
//! keeps the pool's token reserve the last print left, so it reads how far the price
//! falls if the biggest holders sold their whole bags at once ([`dump_drop_pct`]).
//!
//! **Opened only when a loaded rule reads it.** The book holds one entry per wallet
//! that ever bought the token; a track opens it through
//! [`TokenTrack::ensure_holder_book`](super::track::TokenTrack::ensure_holder_book)
//! only when some rule reads a wallet class. It needs the wallet and label columns
//! ([`Metric::needs_wallet_identity`](super::registry::Metric::needs_wallet_identity), [`Metric::needs_ix_labels`](super::registry::Metric::needs_ix_labels)).

use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate;

use crate::event::BuildBreadth;
use crate::hash::{HashedMap, HashedSet};

use super::tags::{BUNDLED, PUBLIC_APP};
use super::{Side, TradeLite};

/// A public app had MORE than this many distinct buying wallets on the previous UTC day.
pub const PUBLIC_MIN_BUYERS: u32 = 100;

/// A public app's wallets bought at least this many times each, on average, on the
/// previous UTC day.
pub const PUBLIC_MIN_REPEAT: u32 = 2;

/// Whether a build-breadth row's app was a public app on its day.
pub fn is_public_app(b: &BuildBreadth) -> bool {
    b.app_buyers > PUBLIC_MIN_BUYERS && u64::from(b.app_buys) >= u64::from(PUBLIC_MIN_REPEAT) * u64::from(b.app_buyers)
}

/// One day's public-app recipes, by build-recipe hash: the rows of that day's
/// build-breadth table that pass [`is_public_app`].
pub fn public_recipes(breadth: &[BuildBreadth]) -> HashedSet {
    breadth.iter().filter(|b| is_public_app(b)).map(|b| b.build_hash).collect()
}

/// Stamp a buy with [`TradeLite::build_day_public`] on one day's public recipes;
/// `None` (no table for the day) stamps it unknown. A sell passes unchanged. **The one
/// stamp**: the engine's live fold and the closed-position readout both call it.
pub fn stamp_public(mut t: TradeLite, public: Option<&HashedSet>) -> TradeLite {
    if t.side == Side::Buy {
        t.build_day_public = public.map(|s| t.build_hash.is_some_and(|h| s.contains(&h)));
    }
    t
}

/// Stamp every buy on the public recipes of its own UTC day, as a replay that loads
/// each day's table at 00:00 UTC stamps it. A day `tables` lacks stays unknown.
pub fn stamp_by_day(trades: &mut [TradeLite], tables: &[(NaiveDate, HashedSet)]) {
    for t in trades.iter_mut() {
        let day = t.at.date_naive();
        let public = tables.iter().find(|(d, _)| *d == day).map(|(_, s)| s);
        *t = stamp_public(*t, public);
    }
}

/// The biggest holders of a coin: the largest bags, by tokens held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopHolders {
    /// The single biggest holder.
    One,
    /// The 10 biggest (every holder when there are fewer).
    Ten,
    /// The biggest 1 % of holders, rounded up: 300 holders = 3.
    OnePct,
    /// The biggest 10 % of holders, rounded up: 300 holders = 30.
    TenPct,
}

impl TopHolders {
    /// How many wallets the group is out of `holders`; at least 1 while anyone holds.
    pub fn size(self, holders: usize) -> usize {
        let n = match self {
            Self::One => 1,
            Self::Ten => 10,
            Self::OnePct => holders.div_ceil(100),
            Self::TenPct => holders.div_ceil(10),
        };
        n.min(holders)
    }
}

/// How far the price falls, in percent, when `sold` tokens are sold into a pool
/// holding `pool_tokens` in one go, before fees. The pool keeps SOL x tokens
/// constant, and price = SOL / tokens, so the price after is the price before x
/// `(pool_tokens / (pool_tokens + sold))^2`. Example: a pool of 100 tokens and 10
/// SOL (price 0.1), 25 tokens sold: 1 - (100 / 125)^2 = 36 %, and the pool ends at
/// 8 SOL and 125 tokens (price 0.064).
pub fn dump_drop_pct(pool_tokens: f64, sold: f64) -> f64 {
    let keep = pool_tokens / (pool_tokens + sold);
    (1.0 - keep * keep) * 100.0
}

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
    /// Every bag above zero as `(tokens as bits, wallet)`: a non-negative `f64`'s bits
    /// sort as the number does, so the last entries are the biggest bags.
    ranked: BTreeSet<(u64, u64)>,
    /// The pool's token reserve the last priced print left (`priced_reserve_sol /
    /// price`); `0` before one.
    pool_tokens: f64,
}

impl HolderBookState {
    /// Fold one print: move the wallet's bag, then class a first buy.
    pub fn on_trade(&mut self, t: &TradeLite) {
        if t.price > 0.0 && t.priced_reserve_sol > 0.0 {
            self.pool_tokens = t.priced_reserve_sol / t.price;
        }
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
        let old = bag.tokens;
        let new = (old + signed).max(0.0);
        let delta = new - old;
        bag.tokens = new;
        if old > 0.0 {
            self.ranked.remove(&(old.to_bits(), t.wallet_hash));
        }
        if new > 0.0 {
            self.ranked.insert((new.to_bits(), t.wallet_hash));
        }
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
        let public = t.build_day_public;
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

    /// Wallets holding more than zero tokens.
    pub fn holders(&self) -> usize {
        self.ranked.len()
    }

    /// How far the price falls, in percent, if `top`'s wallets sold their whole bags
    /// at once into the pool the last print left ([`dump_drop_pct`]). `0` while
    /// nobody holds; `NaN` before a priced print or once the book lost a print's
    /// amount.
    pub fn top_dump_drop_pct(&self, top: TopHolders) -> f64 {
        if self.broken || self.pool_tokens <= 0.0 {
            return f64::NAN;
        }
        let n = top.size(self.ranked.len());
        let sold: f64 = self.ranked.iter().rev().take(n).map(|(bits, _)| f64::from_bits(*bits)).sum();
        dump_drop_pct(self.pool_tokens, sold)
    }

    /// `m_holdings.bag_share_pct @class`: the class's share of live supply, in
    /// percent. `NaN` when no wallet holds tokens, the book lost a print's amount, or
    /// `class` is not a built-in wallet class; `@public_app` is also `NaN` while any
    /// held supply was classed with no breadth table loaded.
    pub fn bag_share_pct(&self, class: &str) -> f64 {
        if self.broken || self.live <= 0.0 {
            return f64::NAN;
        }
        match class {
            PUBLIC_APP if self.unknown > 0.0 => f64::NAN,
            PUBLIC_APP => self.public / self.live * 100.0,
            BUNDLED => self.bundled / self.live * 100.0,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(side: Side, wallet: u64, tokens: f64, slot: u64, build: u64, public: Option<bool>) -> TradeLite {
        TradeLite {
            side,
            sol: 1.0,
            wallet_hash: wallet,
            token_amount: tokens,
            slot,
            build_hash: Some(build),
            build_day_public: public,
            ..TradeLite::default()
        }
    }

    const PUB: Option<bool> = Some(true);
    const BOT: Option<bool> = Some(false);

    /// Each buy is classed on its own UTC day's table; a day with no table stays
    /// unknown, and a sell is never stamped.
    #[test]
    fn a_buy_is_stamped_on_its_own_days_table() {
        use chrono::{TimeZone, Utc};
        let day1 = Utc.with_ymd_and_hms(2026, 9, 13, 12, 0, 0).unwrap();
        let day2 = day1 + chrono::Duration::days(1);
        let day3 = day2 + chrono::Duration::days(1);
        let public = |h| public_recipes(&[BuildBreadth { build_hash: h, app_buyers: 101, app_buys: 202 }]);
        let tables = [(day1.date_naive(), public(7)), (day2.date_naive(), public(9))];
        let at = |side, when| TradeLite { at: when, ..print(side, 1, 1.0, 1, 7, None) };
        let mut trades = [at(Side::Buy, day1), at(Side::Buy, day2), at(Side::Buy, day3), at(Side::Sell, day1)];
        stamp_by_day(&mut trades, &tables);
        let got: Vec<Option<bool>> = trades.iter().map(|t| t.build_day_public).collect();
        assert_eq!(got, [PUB, BOT, None, None]);
    }

    #[test]
    fn a_public_app_is_wide_and_comes_back() {
        let row = |app_buyers, app_buys| BuildBreadth { build_hash: 1, app_buyers, app_buys };
        assert!(is_public_app(&row(101, 202)));
        // Wide but one buy per wallet: the swarm.
        assert!(!is_public_app(&row(3_000, 3_150)));
        assert!(!is_public_app(&row(101, 201)));
        // Comes back but narrow: a private bot.
        assert!(!is_public_app(&row(100, 1_000)));
        // The repeat test cannot overflow.
        assert!(!is_public_app(&row(u32::MAX, u32::MAX)));
    }

    #[test]
    fn public_share_is_the_supply_public_first_buyers_hold() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 700.0, 10, 7, PUB));
        b.on_trade(&print(Side::Buy, 2, 300.0, 11, 8, BOT));
        assert_eq!(b.bag_share_pct(PUBLIC_APP), 70.0);
        // The public buyer sells half: 350 of 650.
        b.on_trade(&print(Side::Sell, 1, 350.0, 12, 7, PUB));
        assert!((b.bag_share_pct(PUBLIC_APP) - 350.0 / 650.0 * 100.0).abs() < 1e-12);
    }

    #[test]
    fn a_holders_class_is_fixed_at_its_first_buy() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, BOT));
        // A later buy through a public app does not reclass the wallet.
        b.on_trade(&print(Side::Buy, 1, 100.0, 11, 9, PUB));
        assert_eq!(b.bag_share_pct(PUBLIC_APP), 0.0);
    }

    #[test]
    fn three_first_buys_in_one_slot_and_build_are_bundled() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, BOT));
        b.on_trade(&print(Side::Buy, 2, 100.0, 10, 7, BOT));
        b.on_trade(&print(Side::Buy, 9, 200.0, 10, 8, BOT)); // same slot, other build
        assert_eq!(b.bag_share_pct(BUNDLED), 0.0);
        b.on_trade(&print(Side::Buy, 3, 100.0, 10, 7, BOT));
        assert_eq!(b.bag_share_pct(BUNDLED), 60.0);
        b.on_trade(&print(Side::Buy, 4, 100.0, 10, 7, BOT));
        assert!((b.bag_share_pct(BUNDLED) - 400.0 / 600.0 * 100.0).abs() < 1e-12);
        // A member's sell leaves the bundled sum with its tokens.
        b.on_trade(&print(Side::Sell, 1, 100.0, 11, 5, BOT));
        assert_eq!(b.bag_share_pct(BUNDLED), 60.0);
    }

    #[test]
    fn a_bag_never_goes_below_zero_and_an_empty_book_is_nan() {
        let mut b = HolderBookState::default();
        assert!(b.bag_share_pct(BUNDLED).is_nan());
        b.on_trade(&print(Side::Sell, 5, 50.0, 9, 7, BOT)); // never bought: nothing
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, PUB));
        b.on_trade(&print(Side::Sell, 1, 150.0, 11, 7, PUB));
        assert!(b.bag_share_pct(PUBLIC_APP).is_nan());
        b.on_trade(&print(Side::Buy, 2, 40.0, 12, 8, BOT));
        assert_eq!(b.bag_share_pct(PUBLIC_APP), 0.0);
    }

    #[test]
    fn no_table_at_a_first_buy_reads_nan_while_that_supply_is_held() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, None));
        b.on_trade(&print(Side::Buy, 2, 100.0, 11, 8, PUB));
        assert!(b.bag_share_pct(PUBLIC_APP).is_nan());
        assert_eq!(b.bag_share_pct(BUNDLED), 0.0);
        b.on_trade(&print(Side::Sell, 1, 100.0, 12, 7, None));
        assert_eq!(b.bag_share_pct(PUBLIC_APP), 100.0);
    }

    /// A print that leaves the pool at `pool` tokens (price 1e-6, so SOL = pool x 1e-6).
    fn priced(side: Side, wallet: u64, tokens: f64, pool: f64) -> TradeLite {
        TradeLite { price: 1e-6, priced_reserve_sol: pool * 1e-6, ..print(side, wallet, tokens, 1, 7, BOT) }
    }

    #[test]
    fn the_drop_is_the_constant_product_price_after_the_sale() {
        // 100 tokens and 10 SOL; 25 sold: 8 SOL and 125 tokens, price 0.1 -> 0.064.
        assert!((dump_drop_pct(100.0, 25.0) - 36.0).abs() < 1e-9);
        assert_eq!(dump_drop_pct(100.0, 0.0), 0.0);
    }

    #[test]
    fn a_group_is_the_biggest_bags_and_never_empty_while_anyone_holds() {
        assert_eq!([0, 5, 300].map(|h| TopHolders::One.size(h)), [0, 1, 1]);
        assert_eq!([5, 300].map(|h| TopHolders::Ten.size(h)), [5, 10]);
        assert_eq!([5, 300, 301].map(|h| TopHolders::OnePct.size(h)), [1, 3, 4]);
        assert_eq!([5, 300].map(|h| TopHolders::TenPct.size(h)), [1, 30]);
    }

    #[test]
    fn the_top_holders_drop_reads_the_biggest_bags_against_the_pool() {
        let mut b = HolderBookState::default();
        assert!(b.top_dump_drop_pct(TopHolders::One).is_nan());
        // Bags 30, 20, 10, 5; the last print leaves 500 in the pool.
        for (w, n) in [(1, 30.0), (2, 20.0), (3, 10.0), (4, 5.0)] {
            b.on_trade(&priced(Side::Buy, w, n, 600.0));
        }
        b.on_trade(&priced(Side::Sell, 4, 5.0, 500.0));
        assert_eq!(b.holders(), 3);
        let drop = |sold: f64| dump_drop_pct(500.0, sold);
        assert!((b.top_dump_drop_pct(TopHolders::One) - drop(30.0)).abs() < 1e-9);
        assert!((b.top_dump_drop_pct(TopHolders::Ten) - drop(60.0)).abs() < 1e-9);
        // A sell moves a bag down the ranking: wallet 1 holds 5 after selling 25.
        b.on_trade(&priced(Side::Sell, 1, 25.0, 525.0));
        assert!((b.top_dump_drop_pct(TopHolders::One) - dump_drop_pct(525.0, 20.0)).abs() < 1e-9);
        assert!((b.top_dump_drop_pct(TopHolders::TenPct) - dump_drop_pct(525.0, 20.0)).abs() < 1e-9);
        // Everyone out: nobody can sell, so nothing falls.
        for (w, n) in [(1, 5.0), (2, 20.0), (3, 10.0)] {
            b.on_trade(&priced(Side::Sell, w, n, 560.0));
        }
        assert_eq!(b.top_dump_drop_pct(TopHolders::Ten), 0.0);
    }

    #[test]
    fn a_print_without_a_token_amount_breaks_the_book() {
        let mut b = HolderBookState::default();
        b.on_trade(&print(Side::Buy, 1, 100.0, 10, 7, PUB));
        b.on_trade(&print(Side::Buy, 2, f64::NAN, 11, 7, PUB));
        assert!(b.bag_share_pct(PUBLIC_APP).is_nan());
        assert!(b.bag_share_pct(BUNDLED).is_nan());
    }
}
