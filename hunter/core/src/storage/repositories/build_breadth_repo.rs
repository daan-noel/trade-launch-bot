//! `build_breadth_day_stats` - the daily build-breadth table.
//!
//! One row per (UTC day, build recipe): how many distinct wallets bought with the
//! recipe, on any token, on the PREVIOUS day. The engine stamps every buy with its
//! recipe's count while a rule reads `m_holder_book`, and `public_app_share` classes
//! a holder's first buy as a public app above
//! [`PUBLIC_MIN_BUYERS`](hunter_engine::metrics::holder_book::PUBLIC_MIN_BUYERS).
//!
//! **One definition** ([`compute_day`](BuildBreadthRepo::compute_day)) fills the
//! table for live (the daily refresh) and for simulate (any day the table lacks), so
//! a past day is graded on the table the live engine would have had that morning.
//! The recipe is cut and hashed with the engine's own `flow_ix` functions, never
//! re-implemented in SQL.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;

use hunter_engine::event::BuildBreadth;
use hunter_engine::grouping::normalize_labels;
use hunter_engine::metrics::flow_ix::{build_hash, is_build_noise};

/// One stored row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct BuildBreadthDayStat {
    pub day: NaiveDate,
    /// The recipe: the ordered labels without setup, teardown and memos, as JSONB.
    pub recipe: Value,
    pub buyers: i32,
}

pub struct BuildBreadthRepo {
    pool: PgPool,
}

/// 00:00 UTC of `day`, as the instant the SQL compares against.
fn day_start_utc(day: NaiveDate) -> DateTime<Utc> {
    day.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
}

impl BuildBreadthRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Compute `day`'s rows from the curve buys of `day - 1`: each distinct label
    /// sequence with its distinct wallets, cut to recipes here, wallets unioned per
    /// recipe. One day of `trades`, grouped in SQL so only the sequences and their
    /// wallet ids travel. Nothing is written. An error when `trades` holds no buy
    /// that day (pruned or not yet ingested): an empty table would class every holder
    /// as a private bot.
    pub async fn compute_day(&self, day: NaiveDate) -> anyhow::Result<Vec<BuildBreadthDayStat>> {
        let end = day_start_utc(day);
        let start = end - Duration::days(1);
        let rows: Vec<(Value, Vec<i32>)> = sqlx::query_as(
            "SELECT ix_labels, array_agg(DISTINCT wallet_id) FROM trades \
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

    /// Label sequences with their wallets -> one row per recipe. A sequence whose
    /// recipe is empty is dropped: the engine could never look it up.
    fn fold(day: NaiveDate, rows: Vec<(Value, Vec<i32>)>) -> Vec<BuildBreadthDayStat> {
        let mut by_recipe: HashMap<Vec<String>, HashSet<i32>> = HashMap::new();
        for (labels, wallets) in rows {
            let recipe: Vec<String> =
                normalize_labels(&labels).into_iter().filter(|l| !is_build_noise(l)).collect();
            if recipe.is_empty() {
                continue;
            }
            by_recipe.entry(recipe).or_default().extend(wallets);
        }
        let mut out: Vec<BuildBreadthDayStat> = by_recipe
            .into_iter()
            .map(|(recipe, wallets)| BuildBreadthDayStat {
                day,
                recipe: Value::from(recipe),
                buyers: wallets.len().min(i32::MAX as usize) as i32,
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.buyers));
        out
    }

    /// Store a day's rows. `ON CONFLICT DO NOTHING`: a day is written once, so a
    /// restart (or a lab run) never rewrites the table a holder was classed under.
    /// Returns how many rows were new.
    pub async fn insert_day(&self, rows: &[BuildBreadthDayStat]) -> anyhow::Result<u64> {
        // 3 binds/row against the 65535-bind statement cap.
        const CHUNK: usize = 5000;
        let mut inserted = 0u64;
        for chunk in rows.chunks(CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> =
                sqlx::QueryBuilder::new("INSERT INTO build_breadth_day_stats (day, recipe, buyers) ");
            qb.push_values(chunk, |mut b, r| {
                b.push_bind(r.day).push_bind(&r.recipe).push_bind(r.buyers);
            });
            qb.push(" ON CONFLICT (day, recipe) DO NOTHING");
            inserted += qb.build().execute(&self.pool).await?.rows_affected();
        }
        Ok(inserted)
    }

    /// The stored rows of one day (empty when the day was never computed).
    pub async fn load_day(&self, day: NaiveDate) -> anyhow::Result<Vec<BuildBreadthDayStat>> {
        let rows = sqlx::query_as::<_, BuildBreadthDayStat>(
            "SELECT day, recipe, buyers FROM build_breadth_day_stats WHERE day = $1",
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
                Some(BuildBreadth { build_hash, buyers: r.buyers.max(0) as u32 })
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

    /// Two sequences that differ only by account setup are one recipe, and a wallet
    /// that used both counts once.
    #[test]
    fn sequences_cut_to_one_recipe_union_their_wallets() {
        let rows = vec![
            (json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]), vec![1, 2]),
            (
                json!([
                    "Compute Budget: SetComputeUnitLimit",
                    "Associated Token: CreateIdempotent",
                    "Pump.Fun: Buy"
                ]),
                vec![2, 3],
            ),
            (json!(["Axiom Trade: ix#00"]), vec![4]),
        ];
        let out = BuildBreadthRepo::fold(day(), rows);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].buyers, 3);
        assert_eq!(out[0].recipe, json!(["Compute Budget: SetComputeUnitLimit", "Pump.Fun: Buy"]));
    }

    /// The stored recipe hashes to the value the engine stamps a trade with, whatever
    /// setup labels that trade carried.
    #[test]
    fn a_stored_recipe_hashes_like_the_trades_it_counts() {
        let trade = json!(["Compute Budget: SetComputeUnitLimit", "Memo Program: Memo", "Pump.Fun: Buy"]);
        let out = BuildBreadthRepo::fold(day(), vec![(trade.clone(), vec![9])]);
        let engine = BuildBreadthRepo::to_engine(&out);
        assert_eq!(
            Some(engine[0].build_hash),
            hunter_engine::metrics::flow_ix::build_hash_from_labels_value(&trade)
        );
    }

    #[test]
    fn a_day_starts_at_utc_midnight() {
        assert_eq!(day_start_utc(day()).to_rfc3339(), "2026-09-11T00:00:00+00:00");
    }
}
