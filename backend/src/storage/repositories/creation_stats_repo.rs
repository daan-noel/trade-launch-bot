use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::PgPool;

use crate::sweep::grouping::GroupField;

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

// ---------------------------------------------------------------------------
// Grouped (per-fingerprint) creation activity — count only, no outcome join.
// ---------------------------------------------------------------------------

/// One ranked fingerprint group: its rank index `g` (0 = largest), the
/// `group_key` JSON object (`{"cu_limit":"200000","ix_labels":"A | B"}`, matching
/// the sweep's `GroupKey::to_json` shape), and its total token count in the window.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct GroupedGroupRow {
    pub g: i64,
    pub group_key: serde_json::Value,
    pub total: i64,
}

/// One day-of-week × hour-of-day cell for group `g` (count only).
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct GroupedHeatCellRow {
    pub g: i64,
    pub dow: i32,
    pub hour: i32,
    pub count: i64,
}

/// One calendar bucket for group `g`. `bucket` is local wall-clock (see [`TrendPointRow`]).
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct GroupedTrendPointRow {
    pub g: i64,
    pub bucket: NaiveDateTime,
    pub count: i64,
}

/// The per-field SQL value expression used to build the group key. Renders a TEXT
/// value so every field collapses to a hashable key, mirroring the sweep's
/// `render_field`: `∅` sentinel for missing values, `" | "`-joined sorted-distinct
/// labels for `ix_labels`. Fields come from the fixed [`GroupField`] enum (never
/// user free-text), so interpolating these literals is injection-safe.
fn group_field_sql(f: GroupField) -> &'static str {
    match f {
        GroupField::CreatorWallet => "COALESCE(t.creator_wallet, '∅')",
        GroupField::TokenProgramId => "COALESCE(t.token_program_id, '∅')",
        GroupField::CuLimit => "COALESCE(t.cu_limit::text, '∅')",
        GroupField::CuPrice => "COALESCE(t.cu_price::text, '∅')",
        GroupField::IsCashbackEnabled => "t.is_cashback_enabled::text",
        GroupField::MaxSolCost => "COALESCE(t.initial_buy_instruction->>'max_sol_cost', '∅')",
        GroupField::SpendableSolIn => {
            "COALESCE(t.initial_buy_instruction->>'spendable_sol_in', '∅')"
        }
        GroupField::InitialBuySol => "COALESCE(t.initial_buy_sol::text, '∅')",
        // Sorted-distinct labels joined with " | ". Close-but-not-identical to the
        // sweep's `normalize_label_vec` (consecutive-dedup, on-chain order) — a
        // stable key for this discovery view (see @docs/architecture.md caveat).
        GroupField::IxLabels => {
            "COALESCE((SELECT string_agg(DISTINCT e, ' | ' ORDER BY e) \
              FROM jsonb_array_elements_text(t.ix_labels) AS e), '∅')"
        }
    }
}

/// Per-group time-series for the grouped dashboard section.
pub struct GroupedCreation {
    pub groups: Vec<GroupedGroupRow>,
    pub cells: Vec<GroupedHeatCellRow>,
    pub points: Vec<GroupedTrendPointRow>,
}

impl CreationStatsRepo {
    /// Partition tokens by a compound fingerprint key (`fields`, in order), keep
    /// the top-`top` groups by volume over the window, and return each group's
    /// day×hour fold (`cells`) and calendar trend (`points`). Count only — no
    /// `tokens_info` join. Shares the same TZ-aware bucketing + segment filter as
    /// [`heatmap`]/[`trend`]; the window is caller-clamped so the scan is bounded.
    pub async fn grouped(
        &self,
        fields: &[GroupField],
        bucket_unit: &str,
        top: i64,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<GroupedCreation> {
        // Build the group-key JSON object expression from the selected fields.
        // Empty selection ⇒ a single "ALL" group (`{}`), like the sweep's ALL group.
        let gkey_sql = if fields.is_empty() {
            "'{}'::jsonb".to_string()
        } else {
            let pairs: Vec<String> = fields
                .iter()
                .map(|fld| format!("'{}', {}", fld.as_str(), group_field_sql(*fld)))
                .collect();
            format!("jsonb_build_object({})", pairs.join(", "))
        };

        // Shared CTE: window+segment-filtered rows with their group key + time
        // dimensions, then the top-N groups ranked by volume (g = 0-based rank).
        let cte = format!(
            r#"
            WITH base AS (
                SELECT {gkey} AS gkey,
                       EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                       EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                       date_trunc($2, (t.created_at AT TIME ZONE $1)) AS bkt
                FROM tokens t
                WHERE t.created_at >= $3 AND t.created_at < $4
                  AND ($5::bool IS NULL OR t.is_mayhem_mode = $5)
                  AND ($6::bool IS NULL OR t.is_cashback_enabled = $6)
            ),
            ranked AS (
                SELECT gkey, COUNT(*) AS total,
                       (row_number() OVER (ORDER BY COUNT(*) DESC, gkey::text) - 1) AS g
                FROM base
                GROUP BY gkey
                ORDER BY total DESC, gkey::text
                LIMIT $7
            )
            "#,
            gkey = gkey_sql,
        );

        let groups = sqlx::query_as::<_, GroupedGroupRow>(&format!(
            "{cte} SELECT g::bigint AS g, gkey AS group_key, total::bigint AS total \
             FROM ranked ORDER BY g"
        ))
        .bind(f.tz)
        .bind(bucket_unit)
        .bind(f.from)
        .bind(f.to)
        .bind(f.mayhem)
        .bind(f.cashback)
        .bind(top)
        .fetch_all(&self.pool)
        .await?;

        let cells = sqlx::query_as::<_, GroupedHeatCellRow>(&format!(
            "{cte} SELECT r.g::bigint AS g, b.dow, b.hour, COUNT(*)::bigint AS count \
             FROM base b JOIN ranked r ON b.gkey = r.gkey \
             GROUP BY r.g, b.dow, b.hour"
        ))
        .bind(f.tz)
        .bind(bucket_unit)
        .bind(f.from)
        .bind(f.to)
        .bind(f.mayhem)
        .bind(f.cashback)
        .bind(top)
        .fetch_all(&self.pool)
        .await?;

        let points = sqlx::query_as::<_, GroupedTrendPointRow>(&format!(
            "{cte} SELECT r.g::bigint AS g, b.bkt AS bucket, COUNT(*)::bigint AS count \
             FROM base b JOIN ranked r ON b.gkey = r.gkey \
             GROUP BY r.g, b.bkt ORDER BY b.bkt"
        ))
        .bind(f.tz)
        .bind(bucket_unit)
        .bind(f.from)
        .bind(f.to)
        .bind(f.mayhem)
        .bind(f.cashback)
        .bind(top)
        .fetch_all(&self.pool)
        .await?;

        Ok(GroupedCreation {
            groups,
            cells,
            points,
        })
    }
}

impl Clone for CreationStatsRepo {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}
