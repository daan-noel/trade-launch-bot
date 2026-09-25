//! `launch_build_day_stats` - the launch-build door feed.
//!
//! One row per (UTC day, creation build): how many tokens the build launched on the
//! PREVIOUS day and how many of those became runners. The engine stamps the two
//! `build_prev_day_*` fingerprint axes from a day's rows at `TokenCreated`, so a
//! rule can arm on "a build whose launches keep running".
//!
//! **One SQL definition** ([`compute_day`](LaunchBuildRepo::compute_day)) fills the
//! table for live (the daily refresh) and for simulate (any day the table lacks),
//! so a past day is graded on the door the live engine would have had that morning.
//! The build key is hashed at load with the engine's own
//! [`ix_hash_from_labels_value`], never re-implemented in SQL.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;

use hunter_engine::event::LaunchBuildStat;
use hunter_engine::metrics::trade_keys::ix_hash_from_labels_value;

use crate::config::constants::{RUNNER_MIN_PEAK_AGE_SECS, RUNNER_PEAK_RESERVE_SOL};
use crate::storage::ix_labels_sql::ix_labels_array_sql;

/// One stored row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LaunchBuildDayStat {
    pub day: NaiveDate,
    /// The build: the exact ordered creation labels, as a JSONB array.
    pub ix_labels: Value,
    pub launches: i32,
    pub runners: i32,
}

pub struct LaunchBuildRepo {
    pool: PgPool,
}

/// 00:00 UTC of `day`, as the instant the SQL compares against.
fn day_start_utc(day: NaiveDate) -> DateTime<Utc> {
    day.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
}

impl LaunchBuildRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Compute `day`'s rows from `tokens` x `tokens_info`: the tokens created on
    /// `day - 1`, grouped by creation build, with the runner count. About one day of
    /// `tokens` (~25k rows), never `trades`. Nothing is written.
    pub async fn compute_day(&self, day: NaiveDate) -> anyhow::Result<Vec<LaunchBuildDayStat>> {
        let arr = ix_labels_array_sql("t.ix_labels");
        let sql = format!(
            "SELECT $1::date AS day, {arr} AS ix_labels, count(*)::int AS launches, \
                    count(*) FILTER (WHERE i.curve_peak_reserve_sol >= $4 \
                        AND i.curve_peak_at >= t.created_at + make_interval(secs => $5) \
                        AND i.curve_peak_at < $3)::int AS runners \
             FROM tokens t LEFT JOIN tokens_info i ON i.mint_address = t.mint_address \
             WHERE t.created_at >= $2 AND t.created_at < $3 AND jsonb_array_length({arr}) > 0 \
             GROUP BY 2 ORDER BY 3 DESC"
        );
        let end = day_start_utc(day);
        let start = end - Duration::days(1);
        let rows = sqlx::query_as::<_, LaunchBuildDayStat>(&sql)
            .bind(day)
            .bind(start)
            .bind(end)
            .bind(RUNNER_PEAK_RESERVE_SOL)
            .bind(RUNNER_MIN_PEAK_AGE_SECS)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    /// Store a day's rows. `ON CONFLICT DO NOTHING`: a day is written once, so a
    /// restart (or a lab run) never rewrites the door a token was already born under.
    /// Returns how many rows were new.
    pub async fn insert_day(&self, rows: &[LaunchBuildDayStat]) -> anyhow::Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        // 4 binds/row against the 65535-bind statement cap.
        const CHUNK: usize = 5000;
        let mut inserted = 0u64;
        for chunk in rows.chunks(CHUNK) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO launch_build_day_stats (day, ix_labels, launches, runners) ",
            );
            qb.push_values(chunk, |mut b, r| {
                b.push_bind(r.day).push_bind(&r.ix_labels).push_bind(r.launches).push_bind(r.runners);
            });
            qb.push(" ON CONFLICT (day, ix_labels) DO NOTHING");
            inserted += qb.build().execute(&self.pool).await?.rows_affected();
        }
        Ok(inserted)
    }

    /// The stored rows of one day (empty when the day was never computed).
    pub async fn load_day(&self, day: NaiveDate) -> anyhow::Result<Vec<LaunchBuildDayStat>> {
        let rows = sqlx::query_as::<_, LaunchBuildDayStat>(
            "SELECT day, ix_labels, launches, runners FROM launch_build_day_stats WHERE day = $1",
        )
        .bind(day)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Every day the table already holds, ascending — what an open-ended run reads
    /// instead of asking for a day range nobody bounded.
    pub async fn stored_days(&self) -> anyhow::Result<Vec<NaiveDate>> {
        let rows: Vec<(NaiveDate,)> =
            sqlx::query_as("SELECT DISTINCT day FROM launch_build_day_stats ORDER BY 1")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    /// The stored rows of `day`, computing and storing them first when absent - the
    /// one call both the live refresh and simulate make, so a day is defined once.
    pub async fn load_or_compute_day(&self, day: NaiveDate) -> anyhow::Result<Vec<LaunchBuildDayStat>> {
        let stored = self.load_day(day).await?;
        if !stored.is_empty() {
            return Ok(stored);
        }
        let computed = self.compute_day(day).await?;
        self.insert_day(&computed).await?;
        Ok(computed)
    }

    /// Rows -> the engine's map input, hashed through the ONE label hasher. A row
    /// whose labels hash to nothing (empty) is dropped: the engine could never stamp
    /// it.
    pub fn to_engine(rows: &[LaunchBuildDayStat]) -> Vec<LaunchBuildStat> {
        rows.iter()
            .filter_map(|r| {
                let build_hash = ix_hash_from_labels_value(&r.ix_labels)?;
                Some(LaunchBuildStat {
                    build_hash,
                    launches: r.launches.max(0) as u32,
                    runners: r.runners.max(0) as u32,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_hash_through_the_engine_hasher_and_drop_empty_builds() {
        let rows = vec![
            LaunchBuildDayStat {
                day: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
                ix_labels: json!(["Pump.Fun: Create_v2", "Pump.Fun: Buy"]),
                launches: 24,
                runners: 3,
            },
            LaunchBuildDayStat {
                day: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
                ix_labels: json!([]),
                launches: 5,
                runners: 0,
            },
        ];
        let out = LaunchBuildRepo::to_engine(&rows);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].build_hash,
            hunter_engine::metrics::trade_keys::ix_hash(&["Pump.Fun: Create_v2", "Pump.Fun: Buy"])
        );
        assert_eq!(out[0].runner_bps(), 1250);
    }

    #[test]
    fn a_day_starts_at_utc_midnight() {
        let d = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        assert_eq!(day_start_utc(d).to_rfc3339(), "2026-09-06T00:00:00+00:00");
    }
}
