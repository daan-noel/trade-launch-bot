//! `build_breadth_day_stats` - the daily build-breadth table.
//!
//! One row per (UTC day, build recipe): the recipe's app
//! ([`recipe_app`](hunter_engine::metrics::trade_keys::recipe_app); the recipe itself for
//! a direct pump.fun call), counted across every recipe it sent on the PREVIOUS day -
//! its distinct buying wallets and its buy transactions, on any token. The engine
//! classes each row with
//! [`is_public_app`](hunter_engine::metrics::holder_book::is_public_app) and stamps
//! every buy with its recipe's class while a rule reads `m_holder_book`.
//!
//! **One definition** ([`compute_day`](BuildBreadthRepo::compute_day)) fills the
//! table for live (the daily refresh) and for simulate (any day the table lacks), so
//! a past day is graded on the table the live engine would have had that morning.
//! The recipe is cut and hashed, and its app named, with the engine's own `flow_ix`
//! functions, never re-implemented in SQL.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;

use hunter_engine::event::BuildBreadth;
use hunter_engine::grouping::normalize_labels;
use hunter_engine::metrics::trade_keys::{build_hash, is_build_noise, recipe_app};

/// One stored row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct BuildBreadthDayStat {
    pub day: NaiveDate,
    /// The recipe: the ordered labels without setup, teardown and memos, as JSONB.
    pub recipe: Value,
    /// Distinct wallets that bought through the recipe's app.
    pub app_buyers: i32,
    /// Buy transactions sent through the recipe's app.
    pub app_buys: i32,
}

pub struct BuildBreadthRepo {
    pool: PgPool,
}

/// 00:00 UTC of `day`, as the instant the SQL compares against.
fn day_start_utc(day: NaiveDate) -> DateTime<Utc> {
    day.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
}

/// What a recipe's app is keyed by: the named program, or the recipe itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AppKey {
    Program(String),
    Recipe(Vec<String>),
}

/// One app's day: its wallets and its buy transactions.
#[derive(Default)]
struct AppDay {
    wallets: HashSet<i32>,
    buys: i64,
}

fn clamp_i32(n: impl TryInto<i32>) -> i32 {
    n.try_into().unwrap_or(i32::MAX)
}

impl BuildBreadthRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Compute `day`'s rows from the curve buys of `day - 1`: each distinct label
    /// sequence with its distinct wallets and buy transactions, cut to recipes and
    /// pooled per app here. One day of `trades`, grouped in SQL so only the sequences,
    /// their wallet ids and a count travel. Nothing is written. An error when `trades`
    /// holds no buy that day (pruned or not yet ingested): an empty table would class
    /// every holder as a private bot.
    pub async fn compute_day(&self, day: NaiveDate) -> anyhow::Result<Vec<BuildBreadthDayStat>> {
        let end = day_start_utc(day);
        let start = end - Duration::days(1);
        let rows: Vec<(Value, Vec<i32>, i64)> = sqlx::query_as(
            "SELECT ix_labels, array_agg(DISTINCT wallet_id), count(DISTINCT tx_signature) FROM trades \
             WHERE block_time >= $1 AND block_time < $2 AND trade_type = 'buy' \
               AND venue = 'curve' AND reserve_lamports > 0 AND ix_labels IS NOT NULL \
             GROUP BY ix_labels",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            anyhow::bail!(
                "no curve buys in trades on {} - cannot compute build breadth for {day}",
                start.date_naive()
            );
        }
        Ok(Self::fold(day, rows))
    }

    /// Label sequences with their wallets and buy transactions -> one row per recipe,
    /// carrying its app's wallets (unioned) and buys (summed; a transaction has one
    /// label sequence, so no buy counts twice). A sequence whose recipe is empty is
    /// dropped: the engine could never look it up.
    fn fold(day: NaiveDate, rows: Vec<(Value, Vec<i32>, i64)>) -> Vec<BuildBreadthDayStat> {
        let mut app_of: HashMap<Vec<String>, AppKey> = HashMap::new();
        let mut apps: HashMap<AppKey, AppDay> = HashMap::new();
        for (labels, wallets, buys) in rows {
            let recipe: Vec<String> =
                normalize_labels(&labels).into_iter().filter(|l| !is_build_noise(l)).collect();
            if recipe.is_empty() {
                continue;
            }
            let key = app_of
                .entry(recipe)
                .or_insert_with_key(|r| match recipe_app(r) {
                    Some(p) => AppKey::Program(p.to_string()),
                    None => AppKey::Recipe(r.clone()),
                })
                .clone();
            let app = apps.entry(key).or_default();
            app.wallets.extend(wallets);
            app.buys += buys;
        }
        let mut out: Vec<BuildBreadthDayStat> = app_of
            .into_iter()
            .map(|(recipe, key)| {
                let app = &apps[&key];
                BuildBreadthDayStat {
                    day,
                    recipe: Value::from(recipe),
                    app_buyers: clamp_i32(app.wallets.len()),
                    app_buys: clamp_i32(app.buys),
                }
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.app_buyers));
        out
    }

    /// Store a day's rows. `ON CONFLICT DO NOTHING`: a day is written once, so a
    /// restart (or a lab run) never rewrites the table a holder was classed under.
    /// Returns how many rows were new.
    pub async fn insert_day(&self, rows: &[BuildBreadthDayStat]) -> anyhow::Result<u64> {
        // 4 binds/row against the 65535-bind statement cap.
        const CHUNK: usize = 5000;
        let mut inserted = 0u64;
        for chunk in rows.chunks(CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO build_breadth_day_stats (day, recipe, app_buyers, app_buys) ",
            );
            qb.push_values(chunk, |mut b, r| {
                b.push_bind(r.day).push_bind(&r.recipe).push_bind(r.app_buyers).push_bind(r.app_buys);
            });
            qb.push(" ON CONFLICT (day, recipe) DO NOTHING");
            inserted += qb.build().execute(&self.pool).await?.rows_affected();
        }
        Ok(inserted)
    }

    /// The stored rows of one day (empty when the day was never computed).
    pub async fn load_day(&self, day: NaiveDate) -> anyhow::Result<Vec<BuildBreadthDayStat>> {
        let rows = sqlx::query_as::<_, BuildBreadthDayStat>(
            "SELECT day, recipe, app_buyers, app_buys FROM build_breadth_day_stats WHERE day = $1",
        )
        .bind(day)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The stored rows of `day`, computing and storing them first when absent - the
    /// one call both the live refresh and simulate make, so a day is defined once.
    pub async fn load_or_compute_day(&self, day: NaiveDate) -> anyhow::Result<Vec<BuildBreadthDayStat>> {
        let stored = self.load_day(day).await?;
        if !stored.is_empty() {
            return Ok(stored);
        }
        let computed = self.compute_day(day).await?;
        self.insert_day(&computed).await?;
        Ok(computed)
    }

    /// Rows -> the engine's table, hashed through the ONE recipe hasher.
    pub fn to_engine(rows: &[BuildBreadthDayStat]) -> Vec<BuildBreadth> {
        rows.iter()
            .filter_map(|r| {
                let build_hash = build_hash(&normalize_labels(&r.recipe))?;
                Some(BuildBreadth {
                    build_hash,
                    app_buyers: r.app_buyers.max(0) as u32,
                    app_buys: r.app_buys.max(0) as u32,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 11).unwrap()
    }

    fn row(out: &[BuildBreadthDayStat], recipe: Value) -> &BuildBreadthDayStat {
        out.iter().find(|r| r.recipe == recipe).expect("recipe stored")
    }

    /// Two sequences that differ only by account setup are one recipe, and a wallet
    /// that used both counts once.
    #[test]
    fn sequences_cut_to_one_recipe_union_their_wallets() {
        let rows = vec![
            (json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]), vec![1, 2], 5),
            (
                json!([
                    "Compute Budget: SetComputeUnitLimit",
                    "Associated Token: CreateIdempotent",
                    "Pump.Fun: Buy"
                ]),
                vec![2, 3],
                4,
            ),
            (json!(["Axiom Trade: ix#00"]), vec![4], 1),
        ];
        let out = BuildBreadthRepo::fold(day(), rows);
        assert_eq!(out.len(), 2);
        let direct = row(&out, json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]));
        assert_eq!((direct.app_buyers, direct.app_buys), (3, 9));
    }

    /// Every recipe of one app carries the app's wallets and buys; each direct pump.fun
    /// recipe is an app of its own.
    #[test]
    fn an_apps_recipes_share_its_wallets_and_buys() {
        let rows = vec![
            (json!(["Compute Budget: SetComputeUnitPrice", "Axiom Trade: ix#00"]), vec![1, 2], 6),
            (json!(["Axiom Trade: ix#00", "Compute Budget: SetComputeUnitPrice"]), vec![2, 3], 3),
            (json!(["Pump.Fun: Buy"]), vec![1], 2),
            (json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]), vec![2, 3], 2),
        ];
        let out = BuildBreadthRepo::fold(day(), rows);
        assert_eq!(out.len(), 4);
        for r in &out[..2] {
            assert_eq!((r.app_buyers, r.app_buys), (3, 9), "{:?}", r.recipe);
        }
        assert_eq!(row(&out, json!(["Pump.Fun: Buy"])).app_buys, 2);
        assert_eq!(row(&out, json!(["Pump.Fun: Buy"])).app_buyers, 1);
    }

    /// The stored recipe hashes to the value the engine stamps a trade with, whatever
    /// setup labels that trade carried.
    #[test]
    fn a_stored_recipe_hashes_like_the_trades_it_counts() {
        let trade = json!(["Compute Budget: SetComputeUnitLimit", "Memo Program: Memo", "Pump.Fun: Buy"]);
        let out = BuildBreadthRepo::fold(day(), vec![(trade.clone(), vec![9], 1)]);
        let engine = BuildBreadthRepo::to_engine(&out);
        assert_eq!(
            Some(engine[0].build_hash),
            hunter_engine::metrics::trade_keys::build_hash_from_labels_value(&trade)
        );
    }

    #[test]
    fn a_day_starts_at_utc_midnight() {
        assert_eq!(day_start_utc(day()).to_rfc3339(), "2026-09-11T00:00:00+00:00");
    }
}
