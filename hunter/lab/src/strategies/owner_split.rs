//! The owner-split Mark rules Flow Discovery offers as auto-tag toggles: the
//! creation-slot rules (Birth) and the transfer rules (Hidden) of
//! `hunter/docs/plans/strategies/_!___owner-split.md`.
//!
//! Each rule names the WALLETS it marks owner, per coin and per group. The page
//! toggles a rule's wallets in and out of the draft tag's `wallet` list; nothing
//! here writes a tag. The thresholds below are the doc's, and they travel on the
//! result ([`OwnerSplitParams`]) so the tooltips quote the numbers the code ran.
//!
//! Two rules read the market beyond the run's coins (a sniper's breadth, a wallet
//! that never sells anywhere). Those facts come from the lake over
//! [`MARKET_WINDOW_DAYS`] ([`MarketFacts`]); without them the two rules are absent
//! from the marks, never empty.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::{creation_slot, CorpusTrade};

/// Instant buy: a buy this many ms or less after the create.
pub const INSTANT_BUY_MS: i64 = 15;
/// Creation-slot bundle: each buy lands this many ms or less from another buy of the pack.
pub const BUNDLE_GAP_MS: i64 = 100;
/// Sniper: opened at least this many coins over the market window...
pub const SNIPER_MIN_COINS: u32 = 20;
/// ...from at least this many creators...
pub const SNIPER_MIN_CREATORS: u32 = 10;
/// ...with at most this share of those coins in the launch group.
pub const SNIPER_MAX_GROUP_SHARE: f64 = 0.20;
/// Below zero: the sell takes the bag below zero by more than this share of the sell.
pub const BELOW_ZERO_MIN_SHARE: f64 = 0.01;
/// Below zero on this many group coins carries the wallet to the group.
pub const BELOW_ZERO_GROUP_COINS: usize = 2;
/// Transfer pair: two token amounts match within this share.
pub const TRANSFER_MATCH_SHARE: f64 = 0.005;
/// Never sells: at least this many coins bought over the market window...
pub const NEVER_SELLS_MIN_COINS: u32 = 10;
/// ...and at least this many buys, with no sell.
pub const NEVER_SELLS_MIN_BUYS: u32 = 20;
/// The market window the sniper and never-sells facts read, ending at the run's last trade.
pub const MARKET_WINDOW_DAYS: i64 = 14;

/// One auto-tag rule. Serialized as the key the page's buttons read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerRule {
    /// A buy within [`INSTANT_BUY_MS`] of the create.
    InstantBuy,
    /// A later buy in the creation slot by a wallet that is not a sniper.
    CreationSlot,
    /// 2+ wallets buying in the creation slot, each within [`BUNDLE_GAP_MS`] of another.
    SlotBundle,
    /// One wallet buying more than once in the creation slot.
    BoughtTwice,
    /// A sell that takes the wallet's token bag below zero.
    BelowZero,
    /// Two wallets moving one token amount off the tape: same amount, or matching holder.
    TransferPair,
    /// A wallet with no sell anywhere over the market window.
    NeverSells,
}

/// The wallets each rule marks, sorted. A rule missing from the map was not run
/// (its market facts were unavailable); an empty list is a real zero.
pub type OwnerMarks = BTreeMap<OwnerRule, Vec<String>>;

/// The thresholds a run applied, echoed on the result for the tooltips.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OwnerSplitParams {
    pub instant_buy_ms: i64,
    pub bundle_gap_ms: i64,
    pub sniper_min_coins: u32,
    pub sniper_min_creators: u32,
    pub sniper_max_group_pct: f64,
    pub below_zero_min_pct: f64,
    pub below_zero_group_coins: usize,
    pub transfer_match_pct: f64,
    pub never_sells_min_coins: u32,
    pub never_sells_min_buys: u32,
    pub market_window_days: i64,
    /// The market window read, or `None` when the facts were unavailable.
    pub market_from: Option<DateTime<Utc>>,
    pub market_to: Option<DateTime<Utc>>,
}

impl OwnerSplitParams {
    pub fn new(facts: Option<&MarketFacts>) -> Self {
        Self {
            instant_buy_ms: INSTANT_BUY_MS,
            bundle_gap_ms: BUNDLE_GAP_MS,
            sniper_min_coins: SNIPER_MIN_COINS,
            sniper_min_creators: SNIPER_MIN_CREATORS,
            sniper_max_group_pct: SNIPER_MAX_GROUP_SHARE * 100.0,
            below_zero_min_pct: BELOW_ZERO_MIN_SHARE * 100.0,
            below_zero_group_coins: BELOW_ZERO_GROUP_COINS,
            transfer_match_pct: TRANSFER_MATCH_SHARE * 100.0,
            never_sells_min_coins: NEVER_SELLS_MIN_COINS,
            never_sells_min_buys: NEVER_SELLS_MIN_BUYS,
            market_window_days: MARKET_WINDOW_DAYS,
            market_from: facts.map(|f| f.from),
            market_to: facts.map(|f| f.to),
        }
    }
}

/// What the lake knows about a wallet beyond the run's coins, over `[from, to)`.
#[derive(Clone, Debug, Default)]
pub struct MarketFacts {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Creation-slot buys outside the create transaction: wallet -> (coins, creators).
    pub openings: HashMap<String, (u32, u32)>,
    /// Wallets with [`NEVER_SELLS_MIN_COINS`] coins, [`NEVER_SELLS_MIN_BUYS`] buys and no sell.
    pub never_sellers: HashSet<String>,
}

/// The lake question for [`MarketFacts`]: the window and the wallets worth asking about.
#[derive(Clone, Debug)]
pub struct MarketQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Wallets with a creation-slot buy outside the create on a run coin (sniper candidates).
    pub openers: Vec<String>,
    /// Wallets that bought a run coin and never sold it (never-sells candidates).
    pub holders: Vec<String>,
}

/// The market question for a run's coins, or `None` for a run with no trades.
pub fn market_query(tokens: &[CorpusToken]) -> Option<MarketQuery> {
    let to = tokens.iter().filter_map(|t| t.trades.last()).map(|t| t.block_time).max()?;
    let mut openers: BTreeSet<&str> = BTreeSet::new();
    let mut holders: BTreeSet<&str> = BTreeSet::new();
    for tok in tokens {
        if let Some(b) = Birth::of(&tok.trades) {
            openers.extend(b.buys.iter().filter_map(|t| t.wallet.as_deref()));
        }
        let mut sold: HashSet<&str> = HashSet::new();
        let mut bought: HashSet<&str> = HashSet::new();
        for t in tok.trades.iter() {
            let Some(w) = t.wallet.as_deref() else { continue };
            if t.is_buy { bought.insert(w); } else { sold.insert(w); }
        }
        holders.extend(bought.difference(&sold));
    }
    Some(MarketQuery {
        from: to - Duration::days(MARKET_WINDOW_DAYS),
        // Exclusive bound, one microsecond past the last trade.
        to: to + Duration::microseconds(1),
        openers: openers.into_iter().map(str::to_string).collect(),
        holders: holders.into_iter().map(str::to_string).collect(),
    })
}

/// A time read at millisecond grain. A row synced from chain time carries whole
/// seconds only, which cannot say 15 ms from 900 ms, so it takes no part in a
/// timing rule.
fn has_ms(t: &CorpusTrade) -> bool {
    t.block_time.timestamp_subsec_micros() != 0
}

fn ms_between(a: &CorpusTrade, b: &CorpusTrade) -> i64 {
    (b.block_time - a.block_time).num_milliseconds()
}

/// One coin's creation slot. The create transaction is the creation slot's first
/// buy (`creation_slot_first_buyer`'s transaction): the lake carries no create flag,
/// so this is the same stand-in the sweep's `creator` reads. The market query in
/// `duck.rs` (`owner_market_facts`) reads the create the same way.
struct Birth<'a> {
    /// The create's own trade, the clock the instant buy reads.
    create: &'a CorpusTrade,
    /// Buys in the creation slot outside the create, by wallets not in the create.
    buys: Vec<&'a CorpusTrade>,
}

impl<'a> Birth<'a> {
    fn of(trades: &'a [CorpusTrade]) -> Option<Self> {
        let slot = creation_slot(trades)?;
        let in_slot: Vec<&CorpusTrade> = trades.iter().filter(|t| t.slot == slot).collect();
        let create = *in_slot
            .iter()
            .filter(|t| t.is_buy)
            .min_by_key(|t| (t.tx_index, t.leg_index))?;
        let create_wallets: HashSet<&str> = in_slot
            .iter()
            .filter(|t| t.tx_index == create.tx_index)
            .filter_map(|t| t.wallet.as_deref())
            .collect();
        let buys = in_slot
            .into_iter()
            .filter(|t| t.is_buy && t.tx_index != create.tx_index)
            .filter(|t| t.wallet.as_deref().is_some_and(|w| !create_wallets.contains(w)))
            .collect();
        Some(Self { create, buys })
    }

    fn wallets(&self) -> BTreeSet<&'a str> {
        self.buys.iter().filter_map(|t| t.wallet.as_deref()).collect()
    }

    fn instant(&self) -> BTreeSet<&'a str> {
        if !has_ms(self.create) {
            return BTreeSet::new();
        }
        self.buys
            .iter()
            .filter(|t| has_ms(t) && (0..=INSTANT_BUY_MS).contains(&ms_between(self.create, t)))
            .filter_map(|t| t.wallet.as_deref())
            .collect()
    }

    /// Packs: the slot's timed buys in time order, split wherever two neighbours sit
    /// more than [`BUNDLE_GAP_MS`] apart. A pack of 2+ wallets marks each of them.
    fn bundle(&self) -> BTreeSet<&'a str> {
        let mut timed: Vec<&CorpusTrade> = self.buys.iter().copied().filter(|t| has_ms(t)).collect();
        timed.sort_by_key(|t| (t.block_time, t.tx_index, t.leg_index));
        let mut out = BTreeSet::new();
        let mut pack: BTreeSet<&str> = BTreeSet::new();
        let mut last: Option<&CorpusTrade> = None;
        for t in timed {
            if last.is_some_and(|l| ms_between(l, t) > BUNDLE_GAP_MS) {
                if pack.len() >= 2 { out.append(&mut pack); }
                pack.clear();
            }
            pack.extend(t.wallet.as_deref());
            last = Some(t);
        }
        if pack.len() >= 2 { out.append(&mut pack); }
        out
    }

    /// A wallet with buys in 2+ transactions of the slot.
    fn bought_twice(&self) -> BTreeSet<&'a str> {
        let mut txs: HashMap<&str, BTreeSet<u32>> = HashMap::new();
        for t in &self.buys {
            if let Some(w) = t.wallet.as_deref() {
                txs.entry(w).or_default().insert(t.tx_index);
            }
        }
        txs.into_iter().filter(|(_, s)| s.len() >= 2).map(|(w, _)| w).collect()
    }
}

/// `a` and `b` are one amount within [`TRANSFER_MATCH_SHARE`].
fn same_amount(a: f64, b: f64) -> bool {
    a > 0.0 && b > 0.0 && (a - b).abs() <= TRANSFER_MATCH_SHARE * a.max(b)
}

/// The transfer rules on one coin, from its whole tape in order.
struct Hidden<'a> {
    below_zero: BTreeSet<&'a str>,
    pairs: BTreeSet<&'a str>,
}

impl<'a> Hidden<'a> {
    fn of(trades: &'a [CorpusTrade]) -> Self {
        let mut bag: HashMap<&str, f64> = HashMap::new();
        let mut buys: HashMap<&str, u32> = HashMap::new();
        let mut sells: HashMap<&str, u32> = HashMap::new();
        let mut below_zero = BTreeSet::new();
        let mut pairs = BTreeSet::new();
        for t in trades {
            let Some(w) = t.wallet.as_deref() else { continue };
            let amt = t.token_amount.abs();
            if !amt.is_finite() { continue; }
            if t.is_buy {
                *bag.entry(w).or_insert(0.0) += amt;
                *buys.entry(w).or_insert(0) += 1;
                continue;
            }
            *sells.entry(w).or_insert(0) += 1;
            let after = { let b = bag.entry(w).or_insert(0.0); *b -= amt; *b };
            if after < 0.0 && -after > BELOW_ZERO_MIN_SHARE * amt {
                below_zero.insert(w);
                // Matching holder: exactly one other wallet still holds the amount
                // this sell is short.
                let short = (-after).min(amt);
                let mut holders = bag
                    .iter()
                    .filter(|&(&h, &b)| h != w && same_amount(b, short))
                    .map(|(&h, _)| h);
                if let (Some(h), None) = (holders.next(), holders.next()) {
                    pairs.insert(h);
                    pairs.insert(w);
                }
            }
        }
        // Same amount: a sell by a wallet that never bought here, matched to the
        // nearest earlier buy of that amount by a wallet that never sold here.
        let only_buys = |w: &str| buys.contains_key(w) && !sells.contains_key(w);
        let only_sells = |w: &str| sells.contains_key(w) && !buys.contains_key(w);
        let mut open: Vec<(f64, usize, &str)> = Vec::new();
        for (i, t) in trades.iter().enumerate() {
            let Some(w) = t.wallet.as_deref() else { continue };
            if t.is_buy && only_buys(w) {
                open.push((t.token_amount.abs(), i, w));
            } else if !t.is_buy && only_sells(w) {
                let s = t.token_amount.abs();
                if let Some(&(_, _, b)) = open.iter().rev().find(|(a, _, _)| same_amount(*a, s)) {
                    pairs.insert(b);
                    pairs.insert(w);
                }
            }
        }
        Self { below_zero, pairs }
    }
}

fn sorted(set: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    let mut v: Vec<String> = set.into_iter().map(Into::into).collect();
    v.sort();
    v.dedup();
    v
}

/// The marks of one group: the group's own (every coin of it) and each coin's.
pub struct GroupMarks {
    pub group: OwnerMarks,
    pub by_token: HashMap<usize, OwnerMarks>,
}

/// Run the rules over the group's coins `idxs` of `tokens`.
///
/// Per coin a rule marks what it finds there. The group form is every coin's marks
/// together, except Below zero, which the doc carries to the group only from
/// [`BELOW_ZERO_GROUP_COINS`] coins on.
pub fn group_marks(tokens: &[CorpusToken], idxs: &[usize], facts: Option<&MarketFacts>) -> GroupMarks {
    let births: Vec<(usize, Option<Birth>)> =
        idxs.iter().map(|&ti| (ti, Birth::of(&tokens[ti].trades))).collect();

    // A sniper's launch-group share counts the group coins inside the market window.
    let mut group_openings: HashMap<&str, u32> = HashMap::new();
    if let Some(f) = facts {
        for (ti, b) in &births {
            let Some(b) = b else { continue };
            let born = tokens[*ti].created_at;
            if born < f.from || born >= f.to { continue; }
            for w in b.wallets() {
                *group_openings.entry(w).or_insert(0) += 1;
            }
        }
    }
    let sniper = |w: &str| -> bool {
        let Some(f) = facts else { return false };
        let Some(&(coins, creators)) = f.openings.get(w) else { return false };
        let here = group_openings.get(w).copied().unwrap_or(0);
        coins >= SNIPER_MIN_COINS
            && creators >= SNIPER_MIN_CREATORS
            && f64::from(here) <= SNIPER_MAX_GROUP_SHARE * f64::from(coins)
    };

    let mut by_token: HashMap<usize, OwnerMarks> = HashMap::new();
    let mut group: BTreeMap<OwnerRule, BTreeSet<String>> = BTreeMap::new();
    let mut below_zero_coins: HashMap<String, usize> = HashMap::new();
    for (ti, birth) in births {
        let trades = &tokens[ti].trades;
        let mut m: OwnerMarks = BTreeMap::new();
        let (instant, bundle, twice, slot) = match &birth {
            Some(b) => {
                let bundle = b.bundle();
                let twice = b.bought_twice();
                // A buy in a bundle, or a second buy, is never the sniper's.
                let slot: BTreeSet<&str> = b
                    .wallets()
                    .into_iter()
                    .filter(|w| bundle.contains(w) || twice.contains(w) || !sniper(w))
                    .collect();
                (b.instant(), bundle, twice, slot)
            }
            None => Default::default(),
        };
        m.insert(OwnerRule::InstantBuy, sorted(instant));
        m.insert(OwnerRule::SlotBundle, sorted(bundle));
        m.insert(OwnerRule::BoughtTwice, sorted(twice));
        if facts.is_some() {
            m.insert(OwnerRule::CreationSlot, sorted(slot));
        }
        let hidden = Hidden::of(trades);
        for w in &hidden.below_zero {
            *below_zero_coins.entry((*w).to_string()).or_insert(0) += 1;
        }
        m.insert(OwnerRule::BelowZero, sorted(hidden.below_zero));
        m.insert(OwnerRule::TransferPair, sorted(hidden.pairs));
        if let Some(f) = facts {
            let buyers: BTreeSet<&str> = trades
                .iter()
                .filter(|t| t.is_buy)
                .filter_map(|t| t.wallet.as_deref())
                .filter(|w| f.never_sellers.contains(*w))
                .collect();
            m.insert(OwnerRule::NeverSells, sorted(buyers));
        }
        for (rule, ws) in &m {
            if *rule == OwnerRule::BelowZero { continue; }
            group.entry(*rule).or_default().extend(ws.iter().cloned());
        }
        by_token.insert(ti, m);
    }
    group.insert(
        OwnerRule::BelowZero,
        below_zero_coins.into_iter().filter(|(_, n)| *n >= BELOW_ZERO_GROUP_COINS).map(|(w, _)| w).collect(),
    );
    GroupMarks {
        group: group.into_iter().map(|(r, ws)| (r, ws.into_iter().collect())).collect(),
        by_token,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone;

    use super::*;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::FlowKeys;

    /// A trade at `ms` past a fixed second (`ms = None` = whole-second chain time).
    fn tr(wallet: &str, is_buy: bool, tokens: f64, slot: u64, tx: u32, ms: Option<i64>) -> CorpusTrade {
        let base = Utc.timestamp_opt(1_760_000_000, 0).unwrap();
        CorpusTrade {
            block_time: base + Duration::microseconds(ms.map(|m| m * 1000 + 1).unwrap_or(0)),
            amount_sol: 1.0,
            token_amount: tokens,
            price_per_token: 1.0,
            reserve_sol: None,
            reserve_token: None,
            real_reserve_sol: None,
            real_token_reserves: None,
            slot,
            tx_index: tx,
            leg_index: 0,
            is_buy,
            on_curve: true,
            venue_fee_bps: None,
            tx_signature: None,
            flow: FlowKeys::default(),
            ix_labels: None,
            wallet: Some(wallet.into()),
        }
    }

    fn tok(mint: &str, trades: Vec<CorpusTrade>) -> CorpusToken {
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: Utc.timestamp_opt(1_760_000_000, 0).unwrap(),
            trades: Arc::new(trades),
            fp: TokenFingerprint::default(),
            identity: None,
        }
    }

    fn marks(trades: Vec<CorpusTrade>, facts: Option<&MarketFacts>) -> OwnerMarks {
        let tokens = vec![tok("m", trades)];
        group_marks(&tokens, &[0], facts).by_token.remove(&0).unwrap()
    }

    fn rule(m: &OwnerMarks, r: OwnerRule) -> Vec<&str> {
        m.get(&r).map(|v| v.iter().map(String::as_str).collect()).unwrap_or_default()
    }

    #[test]
    fn creation_slot_rules_follow_the_doc() {
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, Some(0)),
                tr("fast", true, 10.0, 10, 1, Some(12)),
                tr("packA", true, 10.0, 10, 2, Some(300)),
                tr("packB", true, 10.0, 10, 3, Some(380)),
                tr("twice", true, 10.0, 10, 4, Some(900)),
                tr("twice", true, 10.0, 10, 5, Some(1500)),
                tr("late", true, 10.0, 11, 0, Some(2000)),
            ],
            None,
        );
        assert_eq!(rule(&m, OwnerRule::InstantBuy), ["fast"]);
        // fast (12 ms) is 288 ms from packA: its own pack of one.
        assert_eq!(rule(&m, OwnerRule::SlotBundle), ["packA", "packB"]);
        assert_eq!(rule(&m, OwnerRule::BoughtTwice), ["twice"]);
        // The creator and the later slot never appear; without market facts the
        // sniper-gated rules are absent, not empty.
        assert!(!m.contains_key(&OwnerRule::CreationSlot));
        assert!(!m.contains_key(&OwnerRule::NeverSells));
    }

    #[test]
    fn whole_second_times_take_no_part_in_timing_rules() {
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, None),
                tr("a", true, 10.0, 10, 1, None),
                tr("b", true, 10.0, 10, 2, None),
            ],
            None,
        );
        assert!(rule(&m, OwnerRule::InstantBuy).is_empty());
        assert!(rule(&m, OwnerRule::SlotBundle).is_empty());
    }

    #[test]
    fn creation_slot_drops_the_sniper_unless_bundled() {
        let trades = vec![
            tr("dev", true, 100.0, 10, 0, Some(0)),
            tr("sniper", true, 10.0, 10, 1, Some(200)),
            tr("person", true, 10.0, 10, 2, Some(900)),
        ];
        let facts = MarketFacts {
            from: Utc.timestamp_opt(1_750_000_000, 0).unwrap(),
            to: Utc.timestamp_opt(1_770_000_000, 0).unwrap(),
            openings: HashMap::from([("sniper".into(), (40, 15)), ("person".into(), (2, 2))]),
            never_sellers: HashSet::new(),
        };
        let m = marks(trades.clone(), Some(&facts));
        assert_eq!(rule(&m, OwnerRule::CreationSlot), ["person"]);
        // The same sniper inside a 100 ms pack is the pack's.
        let mut packed = trades;
        packed.push(tr("mate", true, 10.0, 10, 3, Some(250)));
        let m = marks(packed, Some(&facts));
        assert_eq!(rule(&m, OwnerRule::CreationSlot), ["mate", "person", "sniper"]);
    }

    #[test]
    fn transfer_rules_find_below_zero_and_pairs() {
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, Some(0)),
                // Same amount: A buys 1,000 and never sells; B sells 1,000 and never bought.
                tr("A", true, 1000.0, 20, 0, Some(5000)),
                tr("B", false, 1000.0, 21, 0, Some(5500)),
                // Matching holder: H holds 500; S had 100 and sells 600 (short 500).
                tr("H", true, 500.0, 22, 0, Some(6000)),
                tr("S", true, 100.0, 23, 0, Some(6100)),
                tr("S", false, 600.0, 24, 0, Some(6200)),
                // Bag balances: bought and sold the same.
                tr("ok", true, 50.0, 25, 0, Some(7000)),
                tr("ok", false, 50.0, 26, 0, Some(7100)),
            ],
            None,
        );
        assert_eq!(rule(&m, OwnerRule::BelowZero), ["B", "S"]);
        assert_eq!(rule(&m, OwnerRule::TransferPair), ["A", "B", "H", "S"]);
    }

    #[test]
    fn below_zero_reaches_the_group_from_two_coins() {
        let sell_short = |m: &str| tok(m, vec![tr("dev", true, 1.0, 1, 0, Some(0)), tr("w", false, 100.0, 2, 0, Some(9))]);
        let one = vec![sell_short("a"), tok("b", vec![tr("dev", true, 1.0, 1, 0, Some(0))])];
        assert!(group_marks(&one, &[0, 1], None).group[&OwnerRule::BelowZero].is_empty());
        let two = vec![sell_short("a"), sell_short("b")];
        assert_eq!(group_marks(&two, &[0, 1], None).group[&OwnerRule::BelowZero], ["w"]);
    }
}
