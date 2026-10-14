//! Wallets that come back, for one fingerprint's volume tag.
//!
//! The live print appends an instruction list. It does not save a wallet. This
//! read replaces `tags.volume.match.wallet` with the addresses that still pass,
//! and only while their latest trade on the launch group sits inside the recent
//! window. The window is this group's own coins: the gap within which 90% of
//! wallets that stay in the group do come back.
//!
//! The coin set is the SQL mirror of `hunter_engine::fingerprint::matches`
//! ([`fingerprint_scope_clauses`]). A wildcard matches every token, so it is
//! skipped rather than scanned. When no wallet has come back twice, the window
//! is not measured and the stored list is left as it is.

use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use hunter_engine::fingerprint::{AxisId, AxisPredicate};

use crate::models::Fingerprint;
use crate::storage::ix_labels_sql::ix_labels_ordered_eq_sql;
use crate::storage::repositories::creation_stats_repo::fingerprint_scope_clauses;

/// One saved address set, and the window that kept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeWallets {
    pub wallets: Vec<String>,
    /// Launch-group coins, newest first, that still count.
    pub window: i64,
    pub coins: i64,
}

#[derive(Debug, Clone)]
struct Use {
    ords: BTreeSet<i64>,
    buys: i64,
    sells: i64,
}

#[derive(Debug, Clone, Default)]
struct Pair {
    ords: BTreeSet<i64>,
    partners: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy)]
struct Market {
    coins: i64,
    buys: i64,
    sells: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct VolumeFingerprint {
    pub id: Uuid,
    criteria: sqlx::types::Json<hunter_engine::fingerprint::Criteria>,
    wildcard: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct LegRow {
    ord: i64,
    wallet: String,
    trade_type: String,
    token_amount: i64,
    amount_lamports: i64,
    slot: i64,
    ms: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct MarketRow {
    address: String,
    coins: i64,
    buys: i64,
    sells: i64,
}

struct MintLeg {
    wallet: String,
    buy: bool,
    tok: i64,
    lamports: i64,
    slot: i64,
    ms: i64,
}

/// Fingerprints that carry a volume tag.
pub async fn volume_fingerprints(pool: &PgPool) -> anyhow::Result<Vec<VolumeFingerprint>> {
    Ok(sqlx::query_as::<_, VolumeFingerprint>(
        "SELECT id, criteria, wildcard FROM fingerprints WHERE tags ? 'volume' ORDER BY id",
    )
    .fetch_all(pool)
    .await?)
}

/// The addresses to store, or `None` when the window cannot be measured.
/// `None` leaves the stored list alone.
pub async fn coming_back(
    pool: &PgPool,
    row: &VolumeFingerprint,
) -> anyhow::Result<Option<VolumeWallets>> {
    if row.wildcard {
        return Ok(None);
    }
    let mut fp = Fingerprint::empty(row.id, Utc::now());
    fp.criteria = row.criteria.0.clone();
    fp.wildcard = false;
    let (where_sql, labels) = scope_where(&fp);
    let n: i64 = count_coins(pool, &where_sql, labels.as_deref()).await?;
    if n == 0 {
        return Ok(None);
    }
    let legs = load_legs(pool, &where_sql, labels.as_deref()).await?;
    let (uses, below, pairs) = fold_legs(&legs);
    let mut need: Vec<String> = Vec::new();
    for (w, u) in &uses {
        let pair_n = pairs.get(w).map(|p| p.ords.len()).unwrap_or(0);
        let below_n = below.get(w).map(|s| s.len()).unwrap_or(0);
        if u.ords.len() >= 3 || below_n >= 2 || pair_n >= 20 || u.sells == 0 {
            need.push(w.clone());
        }
    }
    let market = load_market(pool, &need).await?;
    Ok(select_wallets(n, &uses, &below, &pairs, &market))
}

fn scope_where(fp: &Fingerprint) -> (String, Option<Vec<String>>) {
    let mut clauses = fingerprint_scope_clauses(fp, "ti");
    let labels = match fp.criteria.get(AxisId::IxLabels) {
        Some(AxisPredicate::Sequence { labels }) => Some(labels.clone()),
        _ => None,
    };
    if labels.is_some() {
        clauses.push(ix_labels_ordered_eq_sql("t.ix_labels", "$1"));
    }
    let body = if clauses.is_empty() {
        "TRUE".to_string()
    } else {
        clauses.join(" AND ")
    };
    (body, labels)
}

async fn count_coins(
    pool: &PgPool,
    where_sql: &str,
    labels: Option<&[String]>,
) -> anyhow::Result<i64> {
    let sql = format!(
        "SELECT COUNT(*)::bigint FROM tokens t \
         LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address \
         WHERE {where_sql}"
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    q = match labels {
        Some(labels) => q.bind(labels),
        None => q,
    };
    Ok(q.fetch_one(pool).await?)
}

async fn load_legs(
    pool: &PgPool,
    where_sql: &str,
    labels: Option<&[String]>,
) -> anyhow::Result<Vec<LegRow>> {
    let sql = format!(
        r#"
        WITH grp AS (
            SELECT t.mint_address,
                   ROW_NUMBER() OVER (ORDER BY t.creation_slot, t.mint_address) AS ord
            FROM tokens t
            LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
            WHERE {where_sql}
        )
        SELECT g.ord,
               w.address AS wallet,
               t.trade_type,
               t.token_amount,
               t.amount_lamports,
               t.slot,
               (EXTRACT(EPOCH FROM t.block_time) * 1000)::bigint AS ms
        FROM trades t
        JOIN grp g ON g.mint_address = t.mint_address
        JOIN wallet_dict w ON w.id = t.wallet_id
        WHERE w.address <> ''
        ORDER BY g.ord, t.slot, t.tx_index, t.leg_index
        "#
    );
    let mut q = sqlx::query_as::<_, LegRow>(&sql);
    q = match labels {
        Some(labels) => q.bind(labels),
        None => q,
    };
    Ok(q.fetch_all(pool).await?)
}

async fn load_market(pool: &PgPool, wallets: &[String]) -> anyhow::Result<HashMap<String, Market>> {
    let mut out = HashMap::new();
    for chunk in wallets.chunks(500) {
        let rows = sqlx::query_as::<_, MarketRow>(
            r#"
            SELECT w.address,
                   COUNT(DISTINCT t.mint_address)::bigint AS coins,
                   COUNT(*) FILTER (WHERE t.trade_type = 'buy')::bigint AS buys,
                   COUNT(*) FILTER (WHERE t.trade_type = 'sell')::bigint AS sells
            FROM trades t
            JOIN wallet_dict w ON w.id = t.wallet_id
            WHERE w.address = ANY($1::text[])
            GROUP BY w.address
            "#,
        )
        .bind(chunk)
        .fetch_all(pool)
        .await?;
        for row in rows {
            out.insert(
                row.address,
                Market { coins: row.coins, buys: row.buys, sells: row.sells },
            );
        }
    }
    Ok(out)
}

fn fold_legs(legs: &[LegRow]) -> (HashMap<String, Use>, HashMap<String, BTreeSet<i64>>, HashMap<String, Pair>) {
    let mut uses: HashMap<String, Use> = HashMap::new();
    let mut below: HashMap<String, BTreeSet<i64>> = HashMap::new();
    let mut pairs: HashMap<String, Pair> = HashMap::new();
    let mut mint: Vec<MintLeg> = Vec::new();
    let mut ord: Option<i64> = None;
    for leg in legs {
        if ord != Some(leg.ord) {
            if let Some(prev) = ord {
                flush_mint(prev, &mint, &mut below, &mut pairs);
            }
            mint.clear();
            ord = Some(leg.ord);
        }
        let buy = leg.trade_type == "buy";
        let u = uses.entry(leg.wallet.clone()).or_insert_with(|| Use {
            ords: BTreeSet::new(),
            buys: 0,
            sells: 0,
        });
        u.ords.insert(leg.ord);
        if buy {
            u.buys += 1;
        } else if leg.trade_type == "sell" {
            u.sells += 1;
        }
        mint.push(MintLeg {
            wallet: leg.wallet.clone(),
            buy,
            tok: leg.token_amount,
            lamports: leg.amount_lamports,
            slot: leg.slot,
            ms: leg.ms,
        });
    }
    if let Some(prev) = ord {
        flush_mint(prev, &mint, &mut below, &mut pairs);
    }
    (uses, below, pairs)
}

fn flush_mint(
    ord: i64,
    legs: &[MintLeg],
    below: &mut HashMap<String, BTreeSet<i64>>,
    pairs: &mut HashMap<String, Pair>,
) {
    let mut bag: HashMap<&str, i128> = HashMap::new();
    for leg in legs {
        if leg.buy {
            *bag.entry(leg.wallet.as_str()).or_insert(0) += i128::from(leg.tok);
            continue;
        }
        let held = bag.get(leg.wallet.as_str()).copied().unwrap_or(0).max(0);
        let excess = i128::from(leg.tok) - held;
        if leg.tok > 0 && excess.saturating_mul(100) > i128::from(leg.tok) {
            below.entry(leg.wallet.clone()).or_default().insert(ord);
        }
        *bag.entry(leg.wallet.as_str()).or_insert(0) -= i128::from(leg.tok);
    }
    let buys: Vec<&MintLeg> = legs.iter().filter(|l| l.buy).collect();
    for a in &buys {
        if a.lamports <= 0 {
            continue;
        }
        let double = i128::from(a.lamports).saturating_mul(2);
        for b in &buys {
            if a.wallet == b.wallet || a.slot != b.slot || i128::from(b.lamports) != double {
                continue;
            }
            let hit = legs.iter().any(|s| {
                !s.buy
                    && s.wallet == a.wallet
                    && (50_000..=70_000).contains(&(s.ms - a.ms))
                    && near(i128::from(s.tok), i128::from(a.tok))
            });
            if hit {
                let p = pairs.entry(a.wallet.clone()).or_default();
                p.ords.insert(ord);
                p.partners.insert(b.wallet.clone());
            }
        }
    }
}

/// 0.5%: the same band the below-zero amount match uses.
fn near(a: i128, b: i128) -> bool {
    if a < 0 || b < 0 {
        return false;
    }
    (a - b).abs().saturating_mul(1_000_000) <= 5_000 * a.max(b)
}

fn select_wallets(
    n: i64,
    uses: &HashMap<String, Use>,
    below: &HashMap<String, BTreeSet<i64>>,
    pairs: &HashMap<String, Pair>,
    market: &HashMap<String, Market>,
) -> Option<VolumeWallets> {
    let mut gaps = Vec::new();
    for (w, u) in uses {
        if u.ords.len() < 3 {
            continue;
        }
        let Some(m) = market.get(w) else { continue };
        let g = u.ords.len() as i64;
        if m.coins > 0 && g * 10 >= m.coins * 8 {
            let ords: Vec<i64> = u.ords.iter().copied().collect();
            gaps.extend(ords.windows(2).map(|w| w[1] - w[0]));
        }
    }
    let window = p90(&gaps)?;
    let newest = n - window + 1;
    let recent = |w: &str| {
        uses.get(w).and_then(|u| u.ords.iter().next_back().copied()).is_some_and(|last| last >= newest)
    };
    let mut kept = BTreeSet::new();
    for (w, u) in uses {
        if !recent(w) {
            continue;
        }
        let g = u.ords.len() as i64;
        if g >= 3 {
            if let Some(m) = market.get(w) {
                if m.coins > 0 && g * 10 >= m.coins * 8 {
                    kept.insert(w.clone());
                }
            }
        }
        if below.get(w).is_some_and(|s| s.len() >= 2) {
            kept.insert(w.clone());
        }
        if u.sells == 0 {
            if let Some(m) = market.get(w) {
                if m.sells == 0 && m.coins >= 10 && m.buys >= 20 {
                    kept.insert(w.clone());
                }
            }
        }
    }
    for (small, pair) in pairs {
        if pair.ords.len() < 20 || !recent(small) {
            continue;
        }
        kept.insert(small.clone());
        for partner in &pair.partners {
            if recent(partner) {
                kept.insert(partner.clone());
            }
        }
    }
    Some(VolumeWallets {
        wallets: kept.into_iter().collect(),
        window,
        coins: n,
    })
}

/// Nearest rank of the 90% gap. Empty means the window is not measured.
fn p90(gaps: &[i64]) -> Option<i64> {
    if gaps.is_empty() {
        return None;
    }
    let mut xs = gaps.to_vec();
    xs.sort_unstable();
    let i = ((xs.len() - 1) as f64 * 0.9).round() as usize;
    xs.get(i).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg(ord: i64, wallet: &str, buy: bool, tok: i64, lamports: i64, slot: i64, ms: i64) -> LegRow {
        LegRow {
            ord,
            wallet: wallet.into(),
            trade_type: if buy { "buy" } else { "sell" }.into(),
            token_amount: tok,
            amount_lamports: lamports,
            slot,
            ms,
        }
    }

    fn use_of(ords: &[i64], buys: i64, sells: i64) -> Use {
        Use { ords: ords.iter().copied().collect(), buys, sells }
    }

    #[test]
    fn an_old_address_comes_off_and_a_recent_one_stays() {
        let mut uses = HashMap::new();
        // Coins 1,2,3 of 5. Gaps of 1. The 90% gap is 1, so only coin 5 is recent.
        uses.insert("old".into(), use_of(&[1, 2, 3], 3, 1));
        uses.insert("now".into(), use_of(&[3, 4, 5], 3, 1));
        let mut market = HashMap::new();
        market.insert("old".into(), Market { coins: 3, buys: 3, sells: 1 });
        market.insert("now".into(), Market { coins: 3, buys: 3, sells: 1 });
        let got = select_wallets(5, &uses, &HashMap::new(), &HashMap::new(), &market).unwrap();
        assert_eq!(got.window, 1);
        assert_eq!(got.wallets, vec!["now".to_string()]);
    }

    #[test]
    fn below_zero_on_two_coins_stays_only_while_recent() {
        let mut uses = HashMap::new();
        uses.insert("back".into(), use_of(&[1, 2, 3], 6, 0));
        uses.insert("sold".into(), use_of(&[2, 3], 0, 2));
        let mut market = HashMap::new();
        market.insert("back".into(), Market { coins: 3, buys: 6, sells: 0 });
        let mut below = HashMap::new();
        below.insert("sold".into(), BTreeSet::from([2, 3]));
        let got = select_wallets(3, &uses, &below, &HashMap::new(), &market).unwrap();
        assert!(got.wallets.contains(&"sold".to_string()));
    }

    #[test]
    fn never_sells_needs_the_market_and_a_recent_group_trade() {
        let mut uses = HashMap::new();
        uses.insert("quiet".into(), use_of(&[1], 1, 0));
        uses.insert("live".into(), use_of(&[1, 2, 5], 3, 0));
        let mut market = HashMap::new();
        market.insert("quiet".into(), Market { coins: 12, buys: 20, sells: 0 });
        market.insert("live".into(), Market { coins: 12, buys: 20, sells: 0 });
        // The stays wallet that measures the window.
        uses.insert("home".into(), use_of(&[3, 4, 5], 3, 1));
        market.insert("home".into(), Market { coins: 3, buys: 3, sells: 1 });
        let got = select_wallets(5, &uses, &HashMap::new(), &HashMap::new(), &market).unwrap();
        assert!(got.wallets.contains(&"live".to_string()));
        assert!(!got.wallets.contains(&"quiet".to_string()));
    }

    #[test]
    fn one_same_slot_coin_does_not_save_the_pair() {
        let legs = vec![
            leg(1, "small", true, 100, 1_000, 7, 0),
            leg(1, "big", true, 200, 2_000, 7, 0),
            leg(1, "small", false, 100, 900, 8, 60_000),
        ];
        let (_uses, _below, pairs) = fold_legs(&legs);
        assert_eq!(pairs.get("small").map(|p| p.ords.len()), Some(1));
        let (uses, below, pairs) = fold_legs(&legs);
        // No wallet has come back across coins, so the window is not measured.
        assert!(select_wallets(1, &uses, &below, &pairs, &HashMap::new()).is_none());
    }

    #[test]
    fn a_sell_of_tokens_never_bought_is_below_zero() {
        let legs = vec![leg(1, "b", false, 1_000, 1, 1, 0)];
        let (_u, below, _p) = fold_legs(&legs);
        assert!(below.get("b").is_some());
    }

    #[test]
    fn no_return_gap_does_not_invent_a_window() {
        let mut uses = HashMap::new();
        uses.insert("once".into(), use_of(&[1], 1, 0));
        assert!(select_wallets(3, &uses, &HashMap::new(), &HashMap::new(), &HashMap::new()).is_none());
    }
}
