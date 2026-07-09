use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::PgPool;

use crate::grouping::GroupField;

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

/// TZ-aware time-bucket SQL expression for a (whitelisted) bucket tag. `ts_expr`
/// is the wall-clock timestamp expression (e.g. `(t.created_at AT TIME ZONE $1)`).
///
/// Calendar-aligned units stay on `date_trunc` (so `week` keeps its Monday
/// alignment); the arbitrary sub-hour / multi-hour intervals use `date_bin` with
/// a midnight origin, so every interval that evenly divides an hour or a day
/// aligns to a clean local boundary. The tag is whitelisted by the handler
/// (`normalize_bucket`) — never user free-text — so interpolating it is
/// injection-safe.
fn bucket_expr(ts_expr: &str, bucket: &str) -> String {
    let bin = |iv: &str| format!("date_bin('{iv}', {ts_expr}, TIMESTAMP '2000-01-01 00:00:00')");
    match bucket {
        "hour" | "day" | "week" => format!("date_trunc('{bucket}', {ts_expr})"),
        "10m" => bin("10 minutes"),
        "30m" => bin("30 minutes"),
        "4h" => bin("4 hours"),
        "8h" => bin("8 hours"),
        "12h" => bin("12 hours"),
        // Defensive: normalize_bucket guarantees a known tag; fall back to day.
        _ => format!("date_trunc('day', {ts_expr})"),
    }
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
        // Bucket expression is interpolated (whitelisted tag), not bound — so the
        // `$2` slot the old `date_trunc($2, …)` used is gone and the rest shift up.
        let bkt = bucket_expr("(t.created_at AT TIME ZONE $1)", bucket_unit);
        let sql = format!(
            r#"
            SELECT
                {bkt} AS bucket,
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
            GROUP BY 1
            ORDER BY 1
            "#,
        );
        let rows = sqlx::query_as::<_, TrendPointRow>(&sql)
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

/// SQL bucket-label expression for a continuous SOL amount. Bins `sol_expr` (a
/// float8 **SOL** expression, or NULL) into [`SOL_BUCKET_WIDTH`]-wide `"lo–hi"` ranges,
/// `∅` when NULL. Kept byte-for-byte in lockstep with `grouping::bucket_sol_label`:
/// same `+ 1e-9` boundary epsilon (0.1 isn't f64-exact), same 1-decimal rounding
/// (`to_char … '0.0'` ⇔ Rust `{:.1}`), same en-dash separator — so the dashboard and
/// the sweep produce identical group keys. `sol_expr` is built from fixed field
/// literals (never user text), so interpolation is injection-safe.
fn sol_bucket_sql(sol_expr: &str) -> String {
    // width 0.1 / 1 decimal — mirror of `grouping::{SOL_BUCKET_WIDTH, SOL_BUCKET_DECIMALS}`.
    let lo = format!("floor(({sol_expr}) / 0.1 + 1e-9) * 0.1");
    format!(
        "CASE WHEN ({sol_expr}) IS NULL THEN '∅' \
         ELSE to_char({lo}, 'FM99999990.0') || '–' || to_char(({lo}) + 0.1, 'FM99999990.0') END"
    )
}

/// The per-field SQL value expression used to build the group key. Renders a TEXT
/// value so every field collapses to a hashable key, mirroring the sweep's
/// `render_field`: discrete fields render their exact value (`∅` sentinel for
/// missing, `" | "`-joined on-chain-order labels for `ix_labels`); the continuous
/// SOL-amount fields are **binned** via [`sol_bucket_sql`]. Fields come from the fixed
/// [`GroupField`] enum (never user free-text), so interpolating these is injection-safe.
fn group_field_sql(f: GroupField) -> String {
    match f {
        GroupField::TokenProgramId => "COALESCE(t.token_program_id, '∅')".to_string(),
        GroupField::CuLimit => "COALESCE(t.cu_limit::text, '∅')".to_string(),
        GroupField::CuPrice => "COALESCE(t.cu_price::text, '∅')".to_string(),
        GroupField::IsCashbackEnabled => "t.is_cashback_enabled::text".to_string(),
        // Continuous SOL amounts → binned SOL ranges. Lamports sources are ÷1e9 to
        // human SOL first so the label reads in SOL (matches the "SOL cost"/"SOL in"
        // display name + the sweep's `bucket_lamports_as_sol`).
        GroupField::MaxCostLamports => {
            sol_bucket_sql("(t.initial_buy_instruction->>'max_cost_lamports')::float8 / 1e9")
        }
        GroupField::SpendableLamportsIn => {
            sol_bucket_sql("(t.initial_buy_instruction->>'spendable_lamports_in')::float8 / 1e9")
        }
        GroupField::InitialBuySol => sol_bucket_sql("t.initial_buy_lamports::float8 / 1e9"),
        // First-slot buy/sell are trade-derived, sourced from `tokens_info` (the `ti`
        // alias the LEFT JOIN in `grouped()` adds) — the only non-`tokens` group fields.
        GroupField::FirstSlotBuySol => sol_bucket_sql("ti.first_slot_buy_lamports::float8 / 1e9"),
        GroupField::FirstSlotSellSol => sol_bucket_sql("ti.first_slot_sell_lamports::float8 / 1e9"),
        // Labels joined with " | " in on-chain order (NOT alphabetised) so the
        // displayed/copied set mirrors the real instruction sequence. Ordinality
        // preserves array position; duplicates are kept intentionally.
        GroupField::IxLabels => "COALESCE((SELECT string_agg(e.val, ' | ' ORDER BY e.ord) \
              FROM jsonb_array_elements_text(t.ix_labels) WITH ORDINALITY AS e(val, ord)), '∅')"
            .to_string(),
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
    /// day×hour fold (`cells`) and calendar trend (`points`). Count only (no
    /// outcome columns), but LEFT JOINs `tokens_info` so trade-derived group fields
    /// (`first_slot_buy_sol`/`first_slot_sell_sol`) can key off it — the join is
    /// one-to-one on `mint_address`, so it doesn't change group cardinality. Shares
    /// the same TZ-aware bucketing + segment filter as
    /// [`heatmap`]/[`trend`]; the window is caller-clamped so the scan is bounded.
    pub async fn grouped(
        &self,
        fields: &[GroupField],
        bucket_unit: &str,
        top: i64,
        field_filters: &[(GroupField, Vec<String>)],
        ix_labels_filter: Option<&[String]>,
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

        // Per-field value filters restrict the corpus *before* partitioning, so
        // only matching groups survive into the top-N. Predicates compare the same
        // TEXT expression the group key renders, against a bound `text[]` (one
        // param per field). `ix_labels` is a set-equality match (order-independent)
        // against the sorted-distinct label array. Binds start at `$7` (after
        // top=$6); both predicate index and bind order must stay in lockstep.
        let mut preds = String::new();
        let mut filter_binds: Vec<Vec<String>> = Vec::new();
        let mut idx = 7;
        for (fld, vals) in field_filters {
            preds.push_str(&format!("\n  AND {} = ANY(${idx})", group_field_sql(*fld)));
            filter_binds.push(vals.clone());
            idx += 1;
        }
        if let Some(labels) = ix_labels_filter {
            let mut sorted: Vec<String> = labels.to_vec();
            sorted.sort();
            sorted.dedup();
            preds.push_str(&format!(
                "\n  AND (SELECT array_agg(DISTINCT e.val ORDER BY e.val) \
                 FROM jsonb_array_elements_text(t.ix_labels) AS e(val)) = ${idx}"
            ));
            filter_binds.push(sorted);
            idx += 1;
        }
        let _ = idx;

        // Shared CTE: window+segment-filtered rows with their group key + time
        // dimensions, then the top-N groups ranked by volume (g = 0-based rank).
        // Bucket expression is interpolated (whitelisted tag), so the old `$2`
        // bucket slot is gone and the fixed binds shift up by one.
        let bkt = bucket_expr("(t.created_at AT TIME ZONE $1)", bucket_unit);
        let cte = format!(
            r#"
            WITH base AS (
                SELECT {gkey} AS gkey,
                       EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                       EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                       {bkt} AS bkt
                FROM tokens t
                LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
                WHERE t.created_at >= $2 AND t.created_at < $3
                  AND ($4::bool IS NULL OR t.is_mayhem_mode = $4)
                  AND ($5::bool IS NULL OR t.is_cashback_enabled = $5){preds}
            ),
            ranked AS (
                SELECT gkey, COUNT(*) AS total,
                       (row_number() OVER (ORDER BY COUNT(*) DESC, gkey::text) - 1) AS g
                FROM base
                GROUP BY gkey
                ORDER BY total DESC, gkey::text
                LIMIT $6
            )
            "#,
            gkey = gkey_sql,
        );

        // SQL strings bound to named locals so the queries (which borrow them)
        // outlive each statement. Bind the fixed params (renumbered) then the
        // per-field filter arrays; applied identically to all three sub-queries.
        let groups_sql = format!(
            "{cte} SELECT g::bigint AS g, gkey AS group_key, total::bigint AS total \
             FROM ranked ORDER BY g"
        );
        let cells_sql = format!(
            "{cte} SELECT r.g::bigint AS g, b.dow, b.hour, COUNT(*)::bigint AS count \
             FROM base b JOIN ranked r ON b.gkey = r.gkey \
             GROUP BY r.g, b.dow, b.hour"
        );
        let points_sql = format!(
            "{cte} SELECT r.g::bigint AS g, b.bkt AS bucket, COUNT(*)::bigint AS count \
             FROM base b JOIN ranked r ON b.gkey = r.gkey \
             GROUP BY r.g, b.bkt ORDER BY b.bkt"
        );

        macro_rules! run {
            ($sql:expr, $ty:ty) => {{
                let mut q = sqlx::query_as::<_, $ty>($sql)
                    .bind(f.tz)
                    .bind(f.from)
                    .bind(f.to)
                    .bind(f.mayhem)
                    .bind(f.cashback)
                    .bind(top);
                for fv in &filter_binds {
                    q = q.bind(fv.as_slice());
                }
                q.fetch_all(&self.pool).await?
            }};
        }

        let groups = run!(&groups_sql, GroupedGroupRow);
        let cells = run!(&cells_sql, GroupedHeatCellRow);
        let points = run!(&points_sql, GroupedTrendPointRow);

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
