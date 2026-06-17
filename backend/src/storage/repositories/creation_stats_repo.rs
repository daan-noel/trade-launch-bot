use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::PgPool;

/// Token-creation-time bias aggregates. Reads `tokens` (creation time + segment
/// flags) LEFT JOINed to `tokens_info` (outcome: migrated / dead), grouped
/// server-side so the handler never pulls raw rows (data-scale guardrail).
///
/// All time bucketing is TZ-aware **in SQL**: `created_at AT TIME ZONE $tz`
/// converts the stored UTC instant to the requested zone's wall-clock before
/// `EXTRACT` / `date_trunc`, so the buckets line up with how a human in that
/// zone perceives "when" a token launched (see the dashboard plan, trap #4).
pub struct CreationStatsRepo {
    pool: PgPool,
}

/// Shared window + segment filter for both aggregates. `mayhem`/`cashback` are
/// `None` = no filter on that flag (the `$::bool IS NULL` short-circuit).
#[derive(Debug, Clone, Copy)]
pub struct StatsFilter<'a> {
    pub tz: &'a str,
    /// Outcome-maturity window (secs): outcome counts exclude tokens younger
    /// than this so a fresh bucket doesn't read artificially dead (trap #1).
    pub maturity_secs: f64,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub mayhem: Option<bool>,
    pub cashback: Option<bool>,
}

/// One day-of-week × hour-of-day cell, folded over the whole window.
/// `dow`: 0 = Sunday … 6 = Saturday (Postgres `EXTRACT(DOW)`). `hour`: 0..23.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct HeatCellRow {
    pub dow: i32,
    pub hour: i32,
    /// Tokens created in this cell (volume view — no maturity censoring).
    pub total: i64,
    /// Tokens old enough for their outcome to be settled (maturity window).
    pub matured: i64,
    /// Matured tokens that also have a `tokens_info` row (outcome coverage base).
    pub known: i64,
    pub migrated: i64,
    pub dead: i64,
}

/// One calendar bucket in absolute time. `bucket` is the **local** wall-clock
/// bucket start (a naive timestamp = the UTC instant already shifted into `$tz`);
/// the frontend renders it as-is on the chart's (UTC-formatting) time axis.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct TrendPointRow {
    pub bucket: NaiveDateTime,
    pub total: i64,
    pub matured: i64,
    pub known: i64,
    pub migrated: i64,
    pub dead: i64,
}

impl CreationStatsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 7×24 seasonality fold (counts + censored outcome columns per cell).
    pub async fn heatmap(&self, f: StatsFilter<'_>) -> anyhow::Result<Vec<HeatCellRow>> {
        let rows = sqlx::query_as::<_, HeatCellRow>(
            r#"
            SELECT
                EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                COUNT(*)::bigint AS total,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $2))::bigint AS matured,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $2)
                                   AND ti.mint_address IS NOT NULL)::bigint AS known,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $2)
                                   AND ti.is_migrated)::bigint AS migrated,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $2)
                                   AND ti.is_dead)::bigint AS dead
            FROM tokens t
            LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
            WHERE t.created_at >= $3 AND t.created_at < $4
              AND ($5::bool IS NULL OR t.is_mayhem_mode = $5)
              AND ($6::bool IS NULL OR t.is_cashback_enabled = $6)
            GROUP BY 1, 2
            "#,
        )
        .bind(f.tz)
        .bind(f.maturity_secs)
        .bind(f.from)
        .bind(f.to)
        .bind(f.mayhem)
        .bind(f.cashback)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Absolute-calendar trend. `bucket_unit` is a `date_trunc` field
    /// (`hour`/`day`/`week`) — validated by the caller, bound as text. Same
    /// maturity censoring + segment filter as [`heatmap`].
    pub async fn trend(
        &self,
        bucket_unit: &str,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<Vec<TrendPointRow>> {
        let rows = sqlx::query_as::<_, TrendPointRow>(
            r#"
            SELECT
                date_trunc($2, (t.created_at AT TIME ZONE $1)) AS bucket,
                COUNT(*)::bigint AS total,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $3))::bigint AS matured,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $3)
                                   AND ti.mint_address IS NOT NULL)::bigint AS known,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $3)
                                   AND ti.is_migrated)::bigint AS migrated,
                COUNT(*) FILTER (WHERE t.created_at < now() - make_interval(secs => $3)
                                   AND ti.is_dead)::bigint AS dead
            FROM tokens t
            LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
            WHERE t.created_at >= $4 AND t.created_at < $5
              AND ($6::bool IS NULL OR t.is_mayhem_mode = $6)
              AND ($7::bool IS NULL OR t.is_cashback_enabled = $7)
            GROUP BY 1
            ORDER BY 1
            "#,
        )
        .bind(f.tz)
        .bind(bucket_unit)
        .bind(f.maturity_secs)
        .bind(f.from)
        .bind(f.to)
        .bind(f.mayhem)
        .bind(f.cashback)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

impl Clone for CreationStatsRepo {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}
