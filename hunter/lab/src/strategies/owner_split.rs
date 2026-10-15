//! The owner-split Mark rules Flow Discovery offers as auto-tag toggles: the
//! creation-slot rules (Birth) and the transfer rules (Hidden) of
//! `hunter/docs/plans/strategies/_!___owner-split.md`.
//!
//! Each rule names the WALLETS it marks owner, per coin and per group. A mark labels
//! that wallet's trades on that coin; [`super::owner_stage`] turns the labels into the
//! rows and wallets a tag stores. Nothing here writes a tag. The thresholds below are
//! the measured ones (`_!___owner-split.md` Appendix B, "Auto-tag thresholds"), and
//! they travel on the result ([`OwnerSplitParams`]) so the tooltips quote the numbers
//! the code ran.
//!
//! Two facts come from the market beyond the run's coins ([`MarketFacts`], one lake
//! read): a sniper's breadth over the [`SNIPER_WINDOW_DAYS`] up to each coin's create,
//! and the wallets that never sell over the [`NEVER_SELLS_WINDOW_DAYS`] ending at the
//! run's last trade. Without them the two rules that need them are absent from the
//! marks, never empty, and snipers stay in the other creation-slot rules.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::{creation_slot, CorpusTrade};

/// Instant buy: a buy this many ms or less after the create.
pub const INSTANT_BUY_MS: i64 = 15;
/// Creation-slot bundle: each buy lands this many ms or less from another buy of the pack.
pub const BUNDLE_GAP_MS: i64 = 10;
/// Sniper: opened at least this many coins over [`SNIPER_WINDOW_DAYS`]...
pub const SNIPER_MIN_COINS: u32 = 40;
/// ...from at least this many creators...
pub const SNIPER_MIN_CREATORS: u32 = 20;
/// ...with at most this share of those coins born from this coin's create list.
pub const SNIPER_MAX_FAMILY_SHARE: f64 = 0.5;
/// The sniper look-back, ending at each coin's create.
pub const SNIPER_WINDOW_DAYS: i64 = 1;
/// Below zero: the sell takes the bag below zero by more than this share of the sell.
pub const BELOW_ZERO_MIN_SHARE: f64 = 0.01;
/// A one-coin wallet mark (Below zero, Transfer pair) reaches the group, and can be
/// stored, once it holds on this many group coins: the wallet comes back.
pub const COMES_BACK_MIN_COINS: usize = 2;
/// Transfer pair: two token amounts match within this share.
pub const TRANSFER_MATCH_SHARE: f64 = 0.001;
/// Never sells: at least this many coins bought over [`NEVER_SELLS_WINDOW_DAYS`]...
pub const NEVER_SELLS_MIN_COINS: u32 = 10;
/// ...and at least this many buys, with no sell.
pub const NEVER_SELLS_MIN_BUYS: u32 = 20;
/// The never-sells look-back, ending at the run's last trade.
pub const NEVER_SELLS_WINDOW_DAYS: i64 = 7;

/// One auto-tag rule. Serialized as the key the page's buttons read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerRule {
    /// A buy within [`INSTANT_BUY_MS`] of the create, by a wallet that is not a sniper.
    InstantBuy,
    /// A later buy in the creation slot by a wallet that is not a sniper.
    CreationSlot,
    /// 2+ wallets buying in the creation slot, each within [`BUNDLE_GAP_MS`] of another; snipers out.
    SlotBundle,
    /// One wallet buying more than once in the creation slot, sniper or not.
    BoughtTwice,
    /// A sell that takes the wallet's token bag below zero.
    BelowZero,
    /// Two wallets moving one token amount off the tape: same amount, or matching holder.
    TransferPair,
    /// A wallet with no sell anywhere over the never-sells window.
    NeverSells,
}

impl OwnerRule {
    pub const ALL: [OwnerRule; 7] = [
        OwnerRule::InstantBuy,
        OwnerRule::CreationSlot,
        OwnerRule::SlotBundle,
        OwnerRule::BoughtTwice,
        OwnerRule::BelowZero,
        OwnerRule::TransferPair,
        OwnerRule::NeverSells,
    ];
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
    pub sniper_max_family_pct: f64,
    pub sniper_window_days: i64,
    pub below_zero_min_pct: f64,
    pub comes_back_min_coins: usize,
    pub transfer_match_pct: f64,
    pub never_sells_min_coins: u32,
    pub never_sells_min_buys: u32,
    pub never_sells_window_days: i64,
    /// Row test: owner share of a row's SOL across the group, and its trade floor.
    pub row_min_owner_pct: f64,
    pub row_min_trades: u64,
    /// Recent window: a stored wallet's latest group trade is this many days or less
    /// before the group's last trade.
    pub recent_window_days: i64,
    /// The never-sells window read, or `None` when the facts were unavailable.
    pub market_from: Option<DateTime<Utc>>,
    pub market_to: Option<DateTime<Utc>>,
}

impl OwnerSplitParams {
    pub fn new(facts: Option<&MarketFacts>) -> Self {
        use super::owner_stage::{RECENT_WINDOW_DAYS, ROW_MIN_OWNER_SHARE, ROW_MIN_TRADES};
        Self {
            instant_buy_ms: INSTANT_BUY_MS,
            bundle_gap_ms: BUNDLE_GAP_MS,
            sniper_min_coins: SNIPER_MIN_COINS,
            sniper_min_creators: SNIPER_MIN_CREATORS,
            sniper_max_family_pct: SNIPER_MAX_FAMILY_SHARE * 100.0,
            sniper_window_days: SNIPER_WINDOW_DAYS,
            below_zero_min_pct: BELOW_ZERO_MIN_SHARE * 100.0,
            comes_back_min_coins: COMES_BACK_MIN_COINS,
            transfer_match_pct: TRANSFER_MATCH_SHARE * 100.0,
            never_sells_min_coins: NEVER_SELLS_MIN_COINS,
            never_sells_min_buys: NEVER_SELLS_MIN_BUYS,
            never_sells_window_days: NEVER_SELLS_WINDOW_DAYS,
            row_min_owner_pct: ROW_MIN_OWNER_SHARE * 100.0,
            row_min_trades: ROW_MIN_TRADES,
            recent_window_days: RECENT_WINDOW_DAYS,
            market_from: facts.map(|f| f.hold_from),
            market_to: facts.map(|f| f.to),
        }
    }
}

/// One coin's sniper count for one opener: (coins opened, creators, coins from this
/// coin's create list), over [`SNIPER_WINDOW_DAYS`] up to this coin's create.
pub type Openings = (u32, u32, u32);

/// What the lake knows about a wallet beyond the run's coins.
#[derive(Clone, Debug, Default)]
pub struct MarketFacts {
    /// Start of the never-sells window; it ends at `to`.
    pub hold_from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Creation-slot openers on the run's coins: (mint, wallet) -> [`Openings`].
    pub openings: HashMap<(String, String), Openings>,
    /// Wallets with [`NEVER_SELLS_MIN_COINS`] coins, [`NEVER_SELLS_MIN_BUYS`] buys and no sell.
    pub never_sellers: HashSet<String>,
}

/// One creation-slot opener to count: the coin, the wallet, the coin's create time.
#[derive(Clone, Debug)]
pub struct Opener {
    pub mint: String,
    pub wallet: String,
    pub born: DateTime<Utc>,
}

/// The lake question for [`MarketFacts`].
#[derive(Clone, Debug)]
pub struct MarketQuery {
    /// Openers on the run's coins (sniper candidates), each counted over its own window.
    pub openers: Vec<Opener>,
    /// Wallets that bought a run coin and never sold it (never-sells candidates).
    pub holders: Vec<String>,
    /// The never-sells window `[hold_from, to)`.
    pub hold_from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// The market question for a run's coins, or `None` for a run with no trades.
pub fn market_query(tokens: &[CorpusToken]) -> Option<MarketQuery> {
    let to = tokens.iter().filter_map(|t| t.trades.last()).map(|t| t.block_time).max()?;
    let mut openers: Vec<Opener> = Vec::new();
    let mut holders: BTreeSet<&str> = BTreeSet::new();
    for tok in tokens {
        if let Some(b) = Birth::of(&tok.trades) {
            openers.extend(b.wallets().into_iter().map(|w| Opener {
                mint: tok.mint.clone(),
                wallet: w.to_string(),
                born: b.create.block_time,
            }));
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
        openers,
        holders: holders.into_iter().map(str::to_string).collect(),
        hold_from: to - Duration::days(NEVER_SELLS_WINDOW_DAYS),
        // Exclusive bound, one microsecond past the last trade.
        to: to + Duration::microseconds(1),
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
    /// The wallets in the create transaction: the creator and any wallet it packs in.
    create_wallets: BTreeSet<&'a str>,
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
        let create_wallets: BTreeSet<&str> = in_slot
            .iter()
            .filter(|t| t.tx_index == create.tx_index)
            .filter_map(|t| t.wallet.as_deref())
            .collect();
        let buys = in_slot
            .into_iter()
            .filter(|t| t.is_buy && t.tx_index != create.tx_index)
            .filter(|t| t.wallet.as_deref().is_some_and(|w| !create_wallets.contains(w)))
            .collect();
        Some(Self { create, create_wallets, buys })
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

/// The wallets in a coin's create transaction (the doc's Creator and Same
/// transaction rows): owner on that coin whatever else is toggled.
pub fn create_wallets(trades: &[CorpusTrade]) -> BTreeSet<&str> {
    Birth::of(trades).map(|b| b.create_wallets).unwrap_or_default()
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

fn sorted<I: IntoIterator<Item = T>, T: Into<String>>(set: I) -> Vec<String> {
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

/// Whether `w` is a sniper on coin `mint`: it opened [`SNIPER_MIN_COINS`] coins from
/// [`SNIPER_MIN_CREATORS`] creators over the day up to this coin's create, at most
/// [`SNIPER_MAX_FAMILY_SHARE`] of them from this coin's create list. `false` without facts.
fn is_sniper(facts: Option<&MarketFacts>, mint: &str, w: &str) -> bool {
    let Some(f) = facts else { return false };
    let Some(&(n, creators, family)) = f.openings.get(&(mint.to_string(), w.to_string())) else { return false };
    n >= SNIPER_MIN_COINS && creators >= SNIPER_MIN_CREATORS && f64::from(family) <= SNIPER_MAX_FAMILY_SHARE * f64::from(n)
}

/// Run the rules over the group's coins `idxs` of `tokens`.
///
/// Per coin a rule marks what it finds there. The group form is every coin's marks
/// together, except Below zero and Transfer pair, which reach the group only for a
/// wallet marked on [`COMES_BACK_MIN_COINS`] coins.
pub fn group_marks(tokens: &[CorpusToken], idxs: &[usize], facts: Option<&MarketFacts>) -> GroupMarks {
    let mut by_token: HashMap<usize, OwnerMarks> = HashMap::new();
    let mut group: BTreeMap<OwnerRule, BTreeSet<String>> = BTreeMap::new();
    let mut coins: HashMap<(OwnerRule, String), usize> = HashMap::new();
    for &ti in idxs {
        let tok = &tokens[ti];
        let trades = &tok.trades;
        let sniper = |w: &str| is_sniper(facts, &tok.mint, w);
        let mut m: OwnerMarks = BTreeMap::new();
        if let Some(b) = Birth::of(trades) {
            let twice = b.bought_twice();
            // A sniper is outsider in every creation-slot rule but Bought twice: a
            // second buy in the slot is never the sniper's.
            let not_sniper = |set: BTreeSet<&str>| sorted(set.into_iter().filter(|w| !sniper(w)));
            m.insert(OwnerRule::InstantBuy, not_sniper(b.instant()));
            m.insert(OwnerRule::SlotBundle, not_sniper(b.bundle()));
            if facts.is_some() {
                m.insert(
                    OwnerRule::CreationSlot,
                    sorted(b.wallets().into_iter().filter(|w| twice.contains(w) || !sniper(w))),
                );
            }
            m.insert(OwnerRule::BoughtTwice, sorted(twice));
        } else {
            m.insert(OwnerRule::InstantBuy, Vec::new());
            m.insert(OwnerRule::SlotBundle, Vec::new());
            m.insert(OwnerRule::BoughtTwice, Vec::new());
            if facts.is_some() {
                m.insert(OwnerRule::CreationSlot, Vec::new());
            }
        }
        let hidden = Hidden::of(trades);
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
            let entry = group.entry(*rule).or_default();
            if matches!(rule, OwnerRule::BelowZero | OwnerRule::TransferPair) {
                for w in ws {
                    *coins.entry((*rule, w.clone())).or_insert(0) += 1;
                }
            } else {
                entry.extend(ws.iter().cloned());
            }
        }
        by_token.insert(ti, m);
    }
    for ((rule, w), n) in coins {
        if n >= COMES_BACK_MIN_COINS {
            group.entry(rule).or_default().insert(w);
        }
    }
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
                tr("packB", true, 10.0, 10, 3, Some(308)),
                // 92 ms after packB: past the 10 ms gap, its own pack of one.
                tr("loner", true, 10.0, 10, 6, Some(400)),
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

    fn facts(openings: &[(&str, Openings)]) -> MarketFacts {
        MarketFacts {
            hold_from: Utc.timestamp_opt(1_750_000_000, 0).unwrap(),
            to: Utc.timestamp_opt(1_770_000_000, 0).unwrap(),
            openings: openings.iter().map(|(w, o)| (("m".to_string(), (*w).to_string()), *o)).collect(),
            never_sellers: HashSet::new(),
        }
    }

    #[test]
    fn a_sniper_is_outsider_in_every_creation_slot_rule_but_bought_twice() {
        let f = facts(&[("sniper", (50, 25, 2)), ("person", (2, 2, 0)), ("family", (50, 25, 40))]);
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, Some(0)),
                // Instant and packed with mate (5 ms apart): still the sniper's.
                tr("sniper", true, 10.0, 10, 1, Some(5)),
                tr("mate", true, 10.0, 10, 2, Some(10)),
                tr("person", true, 10.0, 10, 3, Some(900)),
                // Opens 50 coins, but 40 of them from this create list: not a sniper.
                tr("family", true, 10.0, 10, 4, Some(950)),
            ],
            Some(&f),
        );
        assert_eq!(rule(&m, OwnerRule::InstantBuy), ["mate"]);
        assert_eq!(rule(&m, OwnerRule::SlotBundle), ["mate"]);
        assert_eq!(rule(&m, OwnerRule::CreationSlot), ["family", "mate", "person"]);
        // A second buy in the slot is never the sniper's.
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, Some(0)),
                tr("sniper", true, 10.0, 10, 1, Some(200)),
                tr("sniper", true, 10.0, 10, 2, Some(600)),
            ],
            Some(&f),
        );
        assert_eq!(rule(&m, OwnerRule::BoughtTwice), ["sniper"]);
        assert_eq!(rule(&m, OwnerRule::CreationSlot), ["sniper"]);
    }

    #[test]
    fn without_facts_snipers_stay_in_the_timed_rules() {
        let m = marks(
            vec![
                tr("dev", true, 100.0, 10, 0, Some(0)),
                tr("sniper", true, 10.0, 10, 1, Some(5)),
                tr("mate", true, 10.0, 10, 2, Some(10)),
            ],
            None,
        );
        assert_eq!(rule(&m, OwnerRule::InstantBuy), ["mate", "sniper"]);
        assert_eq!(rule(&m, OwnerRule::SlotBundle), ["mate", "sniper"]);
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

    #[test]
    fn a_transfer_pair_reaches_the_group_from_two_coins() {
        let pair = |m: &str| tok(m, vec![tr("dev", true, 1.0, 1, 0, Some(0)), tr("A", true, 1000.0, 2, 0, Some(9)), tr("B", false, 1000.0, 3, 0, Some(20))]);
        let one = vec![pair("a"), tok("b", vec![tr("dev", true, 1.0, 1, 0, Some(0))])];
        assert!(group_marks(&one, &[0, 1], None).group[&OwnerRule::TransferPair].is_empty());
        let two = vec![pair("a"), pair("b")];
        assert_eq!(group_marks(&two, &[0, 1], None).group[&OwnerRule::TransferPair], ["A", "B"]);
    }

    #[test]
    fn a_transfer_pair_matches_within_a_tenth_of_a_percent() {
        let pair = |sold: f64| marks(vec![tr("dev", true, 1.0, 1, 0, Some(0)), tr("A", true, 1000.0, 2, 0, Some(9)), tr("B", false, sold, 3, 0, Some(20))], None);
        assert_eq!(rule(&pair(1000.9), OwnerRule::TransferPair), ["A", "B"]);
        assert!(rule(&pair(1003.0), OwnerRule::TransferPair).is_empty());
    }
}
