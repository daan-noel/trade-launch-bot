//! What the owner split stores, from the trades the toggled Mark rules label owner
//! (`_!___owner-split.md` Build steps 4 and 5, run by Flow Discovery's auto-tag).
//!
//! A toggled rule labels its wallets' trades owner on the coins it marked them on
//! ([`super::owner_split`]); the create transaction's wallets are owner on their coin
//! whatever is toggled. From those labels:
//!
//! - **Rows.** Every ix structure an owner trade used, read at the doc's seven match
//!   levels. A level passes at [`ROW_MIN_OWNER_SHARE`] owner SOL across the group and
//!   [`ROW_MIN_TRADES`] trades. The fewest passing rows that cover every owner trade
//!   they can are staged, the most owner SOL first. A level that pins a fee field is
//!   read only on trades that carry that field, so a row's numbers are the trades it
//!   matches.
//! - **Wallets.** Only a wallet that comes back is staged: Below zero or Transfer pair
//!   on [`COMES_BACK_MIN_COINS`] group coins, or a never-seller. It is staged when its
//!   latest trade on the group is inside the recent window: the
//!   [`RECENT_WINDOW_DAYS`] before the group's last trade.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use hunter_engine::metrics::trade_keys::{core_labels, core_marks_text};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::flow_discovery::parse_trade_ix_labels;
use super::owner_split::{create_wallets, OwnerMarks, OwnerRule, COMES_BACK_MIN_COINS};
use crate::sweep::corpus::CorpusToken;
use crate::sweep::projection::CorpusTrade;

/// A row is stored at this owner share of its SOL across the group, or more.
pub const ROW_MIN_OWNER_SHARE: f64 = 0.9;
/// ...over at least this many trades.
pub const ROW_MIN_TRADES: u64 = 10;
/// The recent window: a wallet is stored when its latest trade on the group is this
/// many days or less before the group's last trade. The smallest window that keeps
/// 90% of the wallets that trade the group again (`_!___owner-split.md` Appendix B,
/// "Recent window").
pub const RECENT_WINDOW_DAYS: i64 = 4;
/// Failing rows shown with their reason, the most owner SOL first.
const FAILING_ROWS_SHOWN: usize = 8;

/// One toggle's reach on its own: the coins it marks, and those wallets' trades there.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RuleReach {
    pub coins: usize,
    pub trades: u64,
    pub sol: f64,
}

/// One candidate row: the stored `ix_shape` entry and how it scores.
#[derive(Clone, Debug, Serialize)]
pub struct StagedRow {
    /// The `match.ix_shape` entry Apply writes.
    pub row: Value,
    /// 1 (core, side) .. 7 (exact list and budget), the doc's match levels.
    pub level: u8,
    pub trades: u64,
    pub coins: usize,
    pub sol: f64,
    pub owner_sol: f64,
    pub owner_pct: f64,
    /// Why it is not staged; `None` on a staged row.
    pub reason: Option<String>,
}

/// One candidate wallet and the rules that mark it.
#[derive(Clone, Debug, Serialize)]
pub struct StagedWallet {
    pub wallet: String,
    pub rules: Vec<OwnerRule>,
    /// Group coins a rule marks it on.
    pub coins: usize,
    /// Days from its latest trade on the group to the group's last trade.
    pub days_since: f64,
    /// Why it is not staged; `None` on a staged wallet.
    pub reason: Option<String>,
}

/// The stage of one group for one set of toggles.
#[derive(Clone, Debug, Default, Serialize)]
pub struct OwnerStage {
    pub group_coins: usize,
    pub total_trades: u64,
    pub total_sol: f64,
    pub owner_trades: u64,
    pub owner_sol: f64,
    /// Each rule on its own, toggled or not.
    pub reach: BTreeMap<OwnerRule, RuleReach>,
    pub rows: Vec<StagedRow>,
    pub failing_rows: Vec<StagedRow>,
    pub wallets: Vec<StagedWallet>,
    /// Wallets a one-coin rule marks on fewer than [`COMES_BACK_MIN_COINS`] coins.
    pub one_coin_wallets: usize,
    /// The recent window in days, ending at the group's last trade.
    pub recent_window_days: i64,
    /// Per coin: (owner trades, trades).
    pub by_mint: BTreeMap<String, (u64, u64)>,
}

/// A match-level key: what a stored row at that level pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RowKey {
    level: u8,
    hash: u64,
    side: Option<bool>,
    marks: Option<u32>,
    cu_limit: Option<u32>,
    cu_price: Option<u64>,
    tip: Option<u64>,
}

#[derive(Default)]
struct RowAgg {
    trades: u64,
    sol: f64,
    owner_sol: f64,
    coins: HashSet<usize>,
    /// Owner trades it matches, by their id in the owner-trade list.
    owner_ids: Vec<u32>,
    /// One trade it matches, for the labels.
    sample: (usize, usize),
}

/// The seven keys of one trade, loosest first; a level pinning a fee field the
/// trade lacks is skipped.
fn row_keys(t: &CorpusTrade) -> Vec<RowKey> {
    let keys = &t.flow.labels;
    let fee = t.flow.fee;
    let (cl, cp, tip) = (fee.cu_limit(), fee.cu_price(), fee.tip_lamports());
    let mut out = Vec::with_capacity(7);
    if let Some(core) = keys.core_hash {
        let base = RowKey { level: 1, hash: core, side: Some(t.is_buy), marks: None, cu_limit: None, cu_price: None, tip: None };
        let marks = Some(keys.core_marks);
        out.push(base);
        out.push(RowKey { level: 2, marks, ..base });
        if cp.is_some() {
            out.push(RowKey { level: 3, cu_price: cp, ..base });
        }
        if tip.is_some() {
            out.push(RowKey { level: 4, tip, ..base });
        }
        if cp.is_some() && tip.is_some() {
            out.push(RowKey { level: 5, cu_price: cp, tip, ..base });
            out.push(RowKey { level: 6, marks, cu_price: cp, tip, ..base });
        }
    }
    if let (Some(exact), Some(_), Some(_), Some(_)) = (keys.ix_hash, cl, cp, tip) {
        out.push(RowKey { level: 7, hash: exact, side: None, marks: None, cu_limit: cl, cu_price: cp, tip });
    }
    out
}

/// The stored `ix_shape` entry for `key`, labelled from a trade it matches.
fn row_json(key: &RowKey, labels: &[String]) -> Value {
    let mut m = Map::new();
    if key.level == 7 {
        m.insert("labels".into(), json!(labels));
    } else {
        m.insert("labels".into(), json!(core_labels(labels)));
        m.insert("level".into(), json!("core"));
        m.insert("side".into(), json!(if key.side == Some(true) { "buy" } else { "sell" }));
        if let Some(marks) = key.marks {
            m.insert("marks".into(), json!(core_marks_text(marks)));
        }
    }
    if let Some(v) = key.cu_limit {
        m.insert("cu_limit".into(), json!(v));
    }
    if let Some(v) = key.cu_price {
        m.insert("cu_price".into(), json!(v));
    }
    if let Some(v) = key.tip {
        m.insert("tip_lamports".into(), json!(v));
    }
    Value::Object(m)
}

/// Stage the group's coins `idxs` of `tokens` under the toggled `rules`, from each
/// coin's marks `marks` (keyed like `idxs`).
pub fn stage(
    tokens: &[CorpusToken],
    idxs: &[usize],
    marks: &HashMap<usize, OwnerMarks>,
    rules: &BTreeSet<OwnerRule>,
) -> OwnerStage {
    let mut out = OwnerStage {
        group_coins: idxs.len(),
        recent_window_days: RECENT_WINDOW_DAYS,
        ..Default::default()
    };
    // Every wallet's latest trade on the group, and the group's last trade.
    let mut wallet_last: HashMap<&str, DateTime<Utc>> = HashMap::new();
    let mut group_last: Option<DateTime<Utc>> = None;

    let mut aggs: HashMap<RowKey, RowAgg> = HashMap::new();
    let mut owner_id: u32 = 0;
    for &ti in idxs {
        let trades = &tokens[ti].trades;
        let m = marks.get(&ti);
        let mut owners: HashSet<&str> = create_wallets(trades).into_iter().collect();
        for r in rules {
            owners.extend(m.and_then(|m| m.get(r)).into_iter().flatten().map(String::as_str));
        }
        for r in OwnerRule::ALL {
            let Some(ws) = m.and_then(|m| m.get(&r)).filter(|ws| !ws.is_empty()) else { continue };
            let ws: HashSet<&str> = ws.iter().map(String::as_str).collect();
            let reach = out.reach.entry(r).or_default();
            reach.coins += 1;
            for t in trades.iter().filter(|t| t.wallet.as_deref().is_some_and(|w| ws.contains(w))) {
                reach.trades += 1;
                reach.sol += t.amount_sol.abs();
            }
        }
        let (mut coin_owner, mut coin_all) = (0u64, 0u64);
        for (k, t) in trades.iter().enumerate() {
            let sol = t.amount_sol.abs();
            let w = t.wallet.as_deref();
            if let Some(w) = w {
                let last = wallet_last.entry(w).or_insert(t.block_time);
                *last = (*last).max(t.block_time);
            }
            group_last = Some(group_last.map_or(t.block_time, |g| g.max(t.block_time)));
            let owner = w.is_some_and(|w| owners.contains(w));
            coin_all += 1;
            out.total_trades += 1;
            out.total_sol += sol;
            if owner {
                coin_owner += 1;
                out.owner_trades += 1;
                out.owner_sol += sol;
            }
            for key in row_keys(t) {
                let a = aggs.entry(key).or_insert_with(|| RowAgg { sample: (ti, k), ..Default::default() });
                a.trades += 1;
                a.sol += sol;
                a.coins.insert(ti);
                if owner {
                    a.owner_sol += sol;
                    a.owner_ids.push(owner_id);
                }
            }
            if owner {
                owner_id += 1;
            }
        }
        out.by_mint.insert(tokens[ti].mint.clone(), (coin_owner, coin_all));
    }

    // Rows: the passing levels, the most owner SOL first, each kept only when it
    // covers an owner trade no kept row covers.
    let labels_of = |(ti, k): (usize, usize)| parse_trade_ix_labels(tokens[ti].trades[k].ix_labels.as_deref()).unwrap_or_default();
    let score = |key: &RowKey, a: &RowAgg, reason: Option<String>| StagedRow {
        row: row_json(key, &labels_of(a.sample)),
        level: key.level,
        trades: a.trades,
        coins: a.coins.len(),
        sol: a.sol,
        owner_sol: a.owner_sol,
        owner_pct: if a.sol > 0.0 { a.owner_sol / a.sol * 100.0 } else { 0.0 },
        reason,
    };
    let mut cands: Vec<(&RowKey, &RowAgg)> = aggs.iter().filter(|(_, a)| a.owner_sol > 0.0).collect();
    cands.sort_by(|(ka, a), (kb, b)| b.owner_sol.total_cmp(&a.owner_sol).then(ka.level.cmp(&kb.level)));
    let passes = |a: &RowAgg| a.sol > 0.0 && a.owner_sol >= ROW_MIN_OWNER_SHARE * a.sol && a.trades >= ROW_MIN_TRADES;
    let mut covered = vec![false; owner_id as usize];
    for (key, a) in &cands {
        if !passes(a) {
            continue;
        }
        let new: Vec<u32> = a.owner_ids.iter().copied().filter(|&i| !covered[i as usize]).collect();
        if new.is_empty() {
            continue;
        }
        for i in new {
            covered[i as usize] = true;
        }
        out.rows.push(score(key, a, None));
    }
    out.failing_rows = cands
        .iter()
        .filter(|(k, a)| k.level == 1 && !passes(a))
        .take(FAILING_ROWS_SHOWN)
        .map(|(k, a)| {
            let why = if a.trades < ROW_MIN_TRADES {
                format!("{} trades, under {ROW_MIN_TRADES}", a.trades)
            } else {
                format!("{:.0}% owner SOL, under {:.0}%", a.owner_sol / a.sol * 100.0, ROW_MIN_OWNER_SHARE * 100.0)
            };
            score(k, a, Some(why))
        })
        .collect();

    // Wallets that come back, inside the recent window.
    let mut found: BTreeMap<&str, (BTreeSet<OwnerRule>, usize)> = BTreeMap::new();
    for r in rules.iter().filter(|r| matches!(r, OwnerRule::BelowZero | OwnerRule::TransferPair | OwnerRule::NeverSells)) {
        let mut coins: HashMap<&str, usize> = HashMap::new();
        for &ti in idxs {
            for w in marks.get(&ti).and_then(|m| m.get(r)).into_iter().flatten() {
                *coins.entry(w.as_str()).or_insert(0) += 1;
            }
        }
        for (w, n) in coins {
            // A never-seller came back by definition: its count is market-wide.
            if *r != OwnerRule::NeverSells && n < COMES_BACK_MIN_COINS {
                out.one_coin_wallets += 1;
                continue;
            }
            let e = found.entry(w).or_default();
            e.0.insert(*r);
            e.1 = e.1.max(n);
        }
    }
    out.wallets = found
        .into_iter()
        .map(|(w, (rules, coins))| {
            let since = match (group_last, wallet_last.get(w)) {
                (Some(g), Some(&l)) => (g - l).num_milliseconds() as f64 / 86_400_000.0,
                _ => f64::INFINITY,
            };
            let inside = group_last.zip(wallet_last.get(w)).is_some_and(|(g, &l)| g - l <= Duration::days(RECENT_WINDOW_DAYS));
            let reason = (!inside).then(|| {
                format!("latest trade {since:.1} days before the group's last; the recent window is {RECENT_WINDOW_DAYS} days")
            });
            StagedWallet { wallet: w.to_string(), rules: rules.into_iter().collect(), coins, days_since: since, reason }
        })
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, TimeZone, Utc};
    use hunter_engine::metrics::fee::FeeKeys;
    use hunter_engine::metrics::trade_keys::LabelKeys;

    use super::*;
    use crate::strategies::owner_split::group_marks;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::FlowKeys;

    const OWNED: &[&str] = &["Compute Budget: SetComputeUnitLimit", "Private: ix#01", "Pump.Fun: Buy"];
    const PUBLIC: &[&str] = &["Pump.Fun: Buy"];

    fn tr(wallet: &str, labels: &[&str], is_buy: bool, sol: f64, slot: u64, tx: u32) -> CorpusTrade {
        let base = Utc.timestamp_opt(1_760_000_000, 0).unwrap();
        CorpusTrade {
            block_time: base + Duration::microseconds(slot as i64 * 400_000 + tx as i64 * 1000 + 1),
            amount_sol: sol,
            token_amount: sol * 1000.0,
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
            flow: FlowKeys {
                labels: LabelKeys::of(labels),
                wallet_hash: 0,
                fee: FeeKeys::new(Some(200_000), Some(1_000), None),
            },
            ix_labels: Some(serde_json::to_string(labels).unwrap().into()),
            wallet: Some(wallet.into()),
        }
    }

    fn tok(mint: &str, age_s: i64, trades: Vec<CorpusTrade>) -> CorpusToken {
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: Utc.timestamp_opt(1_760_000_000 - age_s, 0).unwrap(),
            trades: Arc::new(trades),
            fp: TokenFingerprint::default(),
            identity: None,
        }
    }

    /// A coin where the dev's crew buys twice in the creation slot on a private
    /// list, and outsiders buy later on the public one.
    fn coin(mint: &str, age_s: i64) -> CorpusToken {
        let mut t = vec![tr("dev", OWNED, true, 1.0, 10, 0)];
        for i in 0..6 {
            t.push(tr(&format!("crew{i}"), OWNED, true, 0.5, 10, 1 + i * 2));
            t.push(tr(&format!("crew{i}"), OWNED, true, 0.5, 10, 2 + i * 2));
        }
        for i in 0..12 {
            t.push(tr(&format!("{mint}-out{i}"), PUBLIC, true, 0.2, 20 + i as u64, 0));
        }
        tok(mint, age_s, t)
    }

    #[test]
    fn a_private_list_passes_and_the_public_list_fails() {
        let tokens = vec![coin("a", 0), coin("b", 100)];
        let marks = group_marks(&tokens, &[0, 1], None).by_token;
        let st = stage(&tokens, &[0, 1], &marks, &BTreeSet::from([OwnerRule::BoughtTwice]));
        // The loosest passing level covers every owner trade: one row, core + side.
        assert_eq!(st.rows.len(), 1);
        let row = &st.rows[0];
        assert_eq!(row.level, 1);
        assert_eq!(row.row["labels"], json!(["Private: ix#01", "Pump.Fun: BUY"]));
        assert_eq!(row.row["side"], json!("buy"));
        assert!(row.owner_pct >= 99.9);
        // The public core never reaches the owner share; it is shown with its reason
        // only when owner trades used it, and here none did.
        assert!(st.failing_rows.iter().all(|r| r.row["labels"] != json!(["Pump.Fun: BUY"])));
        assert_eq!(st.reach[&OwnerRule::BoughtTwice].coins, 2);
    }

    #[test]
    fn nothing_toggled_stages_only_what_the_create_proves() {
        let tokens = vec![coin("a", 0)];
        let marks = group_marks(&tokens, &[0], None).by_token;
        let st = stage(&tokens, &[0], &marks, &BTreeSet::new());
        assert_eq!(st.owner_trades, 1);
        // One create trade is under the trade floor.
        assert!(st.rows.is_empty());
    }

    #[test]
    fn a_wallet_is_staged_only_when_it_comes_back_inside_the_window() {
        // `w` sells tokens it never bought on two coins; `once` on one. Coin `ci`
        // trades `i` days before the newest.
        let short = |w: &str, slot: u64| tr(w, PUBLIC, false, 1.0, slot, 0);
        let mut coins: Vec<CorpusToken> = Vec::new();
        for i in 0..6 {
            let mut c = coin(&format!("c{i}"), i * 86_400);
            let extra = match i {
                0 => vec![short("w", 50)],
                1 => vec![short("w", 50), short("once", 51)],
                5 => vec![short("old", 50)],
                4 => vec![short("old", 49)],
                _ => vec![],
            };
            let trades = Arc::make_mut(&mut c.trades);
            trades.extend(extra);
            for t in trades.iter_mut() {
                t.block_time -= Duration::days(i);
            }
            coins.push(c);
        }
        let idxs: Vec<usize> = (0..coins.len()).collect();
        let marks = group_marks(&coins, &idxs, None).by_token;
        let st = stage(&coins, &idxs, &marks, &BTreeSet::from([OwnerRule::BelowZero]));
        let w = st.wallets.iter().find(|x| x.wallet == "w").expect("w comes back");
        assert!(w.reason.is_none());
        assert_eq!(st.one_coin_wallets, 1);
        // `old` comes back on two coins, but its latest trade is 4 days and a bit
        // before the group's last: outside the 4-day window.
        assert_eq!(st.recent_window_days, 4);
        let old = st.wallets.iter().find(|x| x.wallet == "old").expect("old is listed");
        assert!(old.reason.is_some() && old.days_since > 4.0);
    }
}
