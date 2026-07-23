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

/// SQL bucket-label expression for a continuous SOL amount at bucket `width`. Bins
/// `sol_expr` (a float8 **SOL** expression, or NULL) into `width`-wide `"lo–hi"`
/// ranges, `∅` when NULL. Kept byte-for-byte in lockstep with
/// `grouping::bucket_sol_label` at the SAME width: same `+ 1e-9` boundary epsilon
/// (0.1 isn't f64-exact), same `decimals_for(width)` fractional digits, same en-dash
/// separator — so the dashboard and the sweep produce identical group keys.
///
/// **Rounding safety:** the bucket edge is always a multiple of `width`, so rendering
/// it at exactly `decimals_for(width)` places is lossless — Postgres `to_char`
/// (half-away-from-zero) and Rust `{:.n}` (half-to-even) can never disagree on a
/// value that has no `(decimals+1)`-th digit. This holds ONLY while the `to_char`
/// mask carries exactly `decimals_for(width)` trailing zeros; keep the two in sync.
///
/// `sol_expr` is built from fixed field literals (never user text) and `width` is a
/// server-clamped float, so interpolation is injection-safe.
fn sol_bucket_sql(sol_expr: &str, width: f64) -> String {
    let decimals = crate::grouping::decimals_for(width);
    // `FM` strips padding; the trailing `0`s force exactly `decimals` fractional
    // digits (mirroring Rust `{:.decimals$}`). No dot at all for whole-SOL widths.
    let mask = if decimals == 0 {
        "FM99999990".to_string()
    } else {
        format!("FM99999990.{}", "0".repeat(decimals))
    };
    // `{width}` prints f64 as its shortest round-tripping decimal, which Postgres
    // parses back to the identical float8 — so `/ width` matches Rust's `/ width`.
    let lo = format!("floor(({sol_expr}) / {width} + 1e-9) * {width}");
    format!(
        "CASE WHEN ({sol_expr}) IS NULL THEN '∅' \
         ELSE to_char({lo}, '{mask}') || '–' || to_char(({lo}) + {width}, '{mask}') END"
    )
}

/// The per-field SQL value expression used to build the group key. Renders a TEXT
/// value so every field collapses to a hashable key, mirroring the sweep's
/// `render_field`: discrete fields render their exact value (`∅` sentinel for
/// missing, `" | "`-joined on-chain-order labels for `ix_labels`); the continuous
/// SOL-amount fields are **binned** via [`sol_bucket_sql`]. Fields come from the fixed
/// [`GroupField`] enum (never user free-text), so interpolating these is injection-safe.
///
/// `ti_alias` is the `tokens_info` LEFT JOIN alias the first-slot buy/sell fields
/// read off of — it varies by caller: [`grouped`](CreationStatsRepo::grouped) joins
/// it as `ti`, while the drill-down builders below run against
/// `token_repo`'s own query (`TokenRepo::LIST_FROM`), which joins it as `i`. Passing
/// the wrong one is a silent SQL error ("missing FROM-clause entry") only surfaced
/// at query time — every call site is listed here so a rename can't miss one.
fn group_field_sql(f: GroupField, width: f64, ti_alias: &str) -> String {
    match f {
        GroupField::TokenProgramId => "COALESCE(t.token_program_id, '∅')".to_string(),
        GroupField::CuLimit => "COALESCE(t.cu_limit::text, '∅')".to_string(),
        GroupField::CuPrice => "COALESCE(t.cu_price::text, '∅')".to_string(),
        GroupField::IsCashbackEnabled => "t.is_cashback_enabled::text".to_string(),
        // Continuous SOL amounts → binned SOL ranges. Lamports sources are ÷1e9 to
        // human SOL first so the label reads in SOL (matches the "SOL cost"/"SOL in"
        // display name + the sweep's `bucket_lamports_as_sol`).
        GroupField::MaxCostLamports => {
            sol_bucket_sql("(t.initial_buy_instruction->>'max_cost_lamports')::float8 / 1e9", width)
        }
        GroupField::SpendableLamportsIn => {
            sol_bucket_sql("(t.initial_buy_instruction->>'spendable_lamports_in')::float8 / 1e9", width)
        }
        GroupField::InitialBuySol => sol_bucket_sql("t.initial_buy_lamports::float8 / 1e9", width),
        // First-slot buy/sell are trade-derived, sourced from the caller's
        // `tokens_info` LEFT JOIN — the only non-`tokens` group fields.
        GroupField::FirstSlotBuySol => {
            sol_bucket_sql(&format!("{ti_alias}.first_slot_buy_lamports::float8 / 1e9"), width)
        }
        GroupField::FirstSlotSellSol => {
            sol_bucket_sql(&format!("{ti_alias}.first_slot_sell_lamports::float8 / 1e9"), width)
        }
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
    // Each arg is an independent query dimension the handler already carries;
    // bundling them into a struct would only add indirection for one call site.
    #[allow(clippy::too_many_arguments)]
    pub async fn grouped(
        &self,
        fields: &[GroupField],
        bucket_unit: &str,
        top: i64,
        field_filters: &[(GroupField, Vec<String>)],
        ix_labels_filter: Option<&[String]>,
        // Bucket width (SOL) for the continuous SOL group fields — the same knob the
        // grouped sweep uses, so the dashboard's group labels match a sweep at this
        // width ("swept = run"). Discrete fields ignore it.
        bucket_width: f64,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<GroupedCreation> {
        // Build the group-key JSON object expression from the selected fields.
        // Empty selection ⇒ a single "ALL" group (`{}`), like the sweep's ALL group.
        let gkey_sql = if fields.is_empty() {
            "'{}'::jsonb".to_string()
        } else {
            let pairs: Vec<String> = fields
                .iter()
                .map(|fld| format!("'{}', {}", fld.as_str(), group_field_sql(*fld, bucket_width, "ti")))
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
            preds.push_str(&format!("\n  AND {} = ANY(${idx})", group_field_sql(*fld, bucket_width, "ti")));
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

// ---------------------------------------------------------------------------
// Grouped-creation "drill-down" token list — pure WHERE/ORDER builders (no DB).
//
// Backs the dashboard's "view tokens" action: identify every token belonging
// to ONE specific fingerprint group `grouped()` already ranked (an exact
// `group_key` match on each `fields` entry), optionally narrowed to one
// recurring weekly day-of-week+hour-of-day slot (a heatmap tile click). The
// actual row fetch/pagination reuses `token_repo::{find_list_page,count_list}`
// verbatim (same `TokenListRow` projection the live Tokens list serves) — this
// module only builds the extra `WHERE`/`ORDER BY` fragment identifying the
// group/cell, numbered from `$1` (the caller — `token_repo`— appends its own
// trailing `LIMIT`/`OFFSET` binds after these).
// ---------------------------------------------------------------------------

/// Build the `WHERE` body + positional binds selecting every token in one exact
/// fingerprint group (optionally narrowed to one recurring day-of-week+hour
/// slot), reusing the SAME window/segment/corpus filters `grouped()` used when
/// it computed the group. `search` is an optional mint/symbol substring
/// (mirrors the Tokens page's global search box); blank ⇒ no restriction.
#[allow(clippy::too_many_arguments)]
pub fn build_grouped_tokens_where(
    fields: &[GroupField],
    group_key: &[(GroupField, String)],
    field_filters: &[(GroupField, Vec<String>)],
    ix_labels_filter: Option<&[String]>,
    bucket_width: f64,
    dow: Option<i32>,
    hour: Option<i32>,
    search: &str,
    f: StatsFilter<'_>,
) -> (String, Vec<crate::api::handlers::tokens::SqlArg>) {
    use crate::api::handlers::tokens::SqlArg;

    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<SqlArg> = Vec::new();

    args.push(SqlArg::Ts(f.from));
    clauses.push(format!("t.created_at >= ${}", args.len()));
    args.push(SqlArg::Ts(f.to));
    clauses.push(format!("t.created_at < ${}", args.len()));

    if let Some(m) = f.mayhem {
        args.push(SqlArg::Bool(m));
        clauses.push(format!("t.is_mayhem_mode = ${}", args.len()));
    }
    if let Some(c) = f.cashback {
        args.push(SqlArg::Bool(c));
        clauses.push(format!("t.is_cashback_enabled = ${}", args.len()));
    }

    // The exact group: every selected `fields` entry must render to its
    // `group_key` value — the same equality `grouped()`'s `ranked` CTE applies
    // per-`gkey`, just pinned to this one rank instead of top-N'd.
    for field in fields {
        let Some((_, val)) = group_key.iter().find(|(f2, _)| f2 == field) else {
            continue;
        };
        args.push(SqlArg::Str(val.clone()));
        // Runs against `token_repo`'s query (`TokenRepo::LIST_FROM`), which joins
        // `tokens_info` as `i` (NOT the `ti` `grouped()` uses) — see `group_field_sql`.
        clauses.push(format!("{} = ${}", group_field_sql(*field, bucket_width, "i"), args.len()));
    }

    // Corpus-level filters applied before the groups were ranked (same as `grouped()`).
    for (field, vals) in field_filters {
        args.push(SqlArg::StrArray(vals.clone()));
        clauses.push(format!("{} = ANY(${})", group_field_sql(*field, bucket_width, "i"), args.len()));
    }
    if let Some(labels) = ix_labels_filter {
        let mut sorted: Vec<String> = labels.to_vec();
        sorted.sort();
        sorted.dedup();
        args.push(SqlArg::StrArray(sorted));
        clauses.push(format!(
            "(SELECT array_agg(DISTINCT e.val ORDER BY e.val) \
              FROM jsonb_array_elements_text(t.ix_labels) AS e(val)) = ${}",
            args.len()
        ));
    }

    // Recurring weekly slot (a heatmap tile): every occurrence of this
    // day-of-week + hour-of-day across the whole window, in the requested tz —
    // mirrors exactly how `heatmap()`/`grouped()` fold their cells.
    if let (Some(dow), Some(hour)) = (dow, hour) {
        args.push(SqlArg::Str(f.tz.to_string()));
        let tz_ph = args.len();
        args.push(SqlArg::I64(dow as i64));
        let dow_ph = args.len();
        args.push(SqlArg::I64(hour as i64));
        let hour_ph = args.len();
        clauses.push(format!(
            "EXTRACT(DOW FROM (t.created_at AT TIME ZONE ${tz_ph}))::int = ${dow_ph} \
             AND EXTRACT(HOUR FROM (t.created_at AT TIME ZONE ${tz_ph}))::int = ${hour_ph}"
        ));
    }

    // Mint/symbol substring search (mirrors the Tokens page's global search;
    // `sql.rs::search_clause` is the SSOT for the live list — narrowed here to
    // avoid pulling in its `SqlArgs` counter type for one extra clause).
    let needle = search.trim();
    if !needle.is_empty() {
        let esc = needle
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        args.push(SqlArg::Str(esc));
        let ph = args.len();
        clauses.push(format!(
            "(LOWER(t.mint_address) LIKE '%' || ${ph} || '%' ESCAPE '\\' \
              OR LOWER(t.symbol) LIKE '%' || ${ph} || '%' ESCAPE '\\')"
        ));
    }

    let where_sql = if clauses.is_empty() {
        "TRUE".to_string()
    } else {
        clauses.join(" AND ")
    };
    (where_sql, args)
}

/// `ORDER BY` body for the drill-down list. Reuses the SAME per-column sort
/// registry the live Tokens list reads (`sort_sql_expr`), so a column-header
/// sort click behaves identically here. Unknown/empty sort ⇒ newest-first
/// (matches the heatmap/trend's implicit ordering).
pub fn build_grouped_tokens_order(sorting: &[(String, bool)]) -> String {
    let mut terms: Vec<String> = Vec::new();
    for (col, desc) in sorting {
        if let Some((expr, is_text)) = crate::api::handlers::tokens::sort_sql_expr(col) {
            let dir = if *desc { "DESC" } else { "ASC" };
            let keyed = if is_text { format!("LOWER({expr})") } else { expr };
            terms.push(format!("{keyed} {dir} NULLS LAST"));
        }
    }
    if terms.is_empty() {
        return "t.created_at DESC, t.mint_address DESC".to_string();
    }
    terms.push("t.mint_address ASC".to_string());
    terms.join(", ")
}

#[cfg(test)]
mod grouped_tokens_tests {
    use super::*;
    use crate::api::handlers::tokens::SqlArg;
    use crate::grouping::SOL_BUCKET_WIDTH;

    fn filter(from: DateTime<Utc>, to: DateTime<Utc>) -> StatsFilter<'static> {
        StatsFilter { tz: "UTC", maturity_secs: 0.0, from, to, mayhem: None, cashback: None }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-16T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn all_group_with_no_extra_filters_is_window_only() {
        let (where_sql, args) = build_grouped_tokens_where(
            &[], &[], &[], None, SOL_BUCKET_WIDTH, None, None, "", filter(now(), now()),
        );
        assert_eq!(where_sql, "t.created_at >= $1 AND t.created_at < $2");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn group_key_lowers_to_equality_per_field() {
        let (where_sql, args) = build_grouped_tokens_where(
            &[GroupField::CuLimit],
            &[(GroupField::CuLimit, "200000".to_string())],
            &[],
            None,
            SOL_BUCKET_WIDTH,
            None,
            None,
            "",
            filter(now(), now()),
        );
        assert!(where_sql.contains("COALESCE(t.cu_limit::text, '∅') = $3"));
        match &args[2] {
            SqlArg::Str(s) => assert_eq!(s, "200000"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn first_slot_sol_fields_use_token_repos_tokens_info_alias() {
        // Regression: `grouped()` joins `tokens_info` as `ti`, but the drill-down
        // WHERE runs against `token_repo`'s own query (`TokenRepo::LIST_FROM`),
        // which joins it as `i`. Emitting `ti.` here produced a live Postgres
        // "missing FROM-clause entry for table \"ti\"" (surfaced to the client as
        // the generic "failed to compute creation stats" 500).
        let (where_sql, _args) = build_grouped_tokens_where(
            &[GroupField::FirstSlotBuySol],
            &[(GroupField::FirstSlotBuySol, "0.0–0.1".to_string())],
            &[(GroupField::FirstSlotSellSol, vec!["0.0–0.1".to_string()])],
            None,
            SOL_BUCKET_WIDTH,
            None,
            None,
            "",
            filter(now(), now()),
        );
        assert!(where_sql.contains("i.first_slot_buy_lamports"));
        assert!(where_sql.contains("i.first_slot_sell_lamports"));
        assert!(!where_sql.contains("ti."), "must not reference the grouped()-only `ti` alias: {where_sql}");
    }

    #[test]
    fn missing_group_key_entry_is_skipped_not_faked() {
        // A `fields` entry with no matching `group_key` value emits no clause for
        // it (defensive — the handler validates completeness before calling in).
        let (where_sql, _args) = build_grouped_tokens_where(
            &[GroupField::CuLimit],
            &[],
            &[],
            None,
            SOL_BUCKET_WIDTH,
            None,
            None,
            "",
            filter(now(), now()),
        );
        assert!(!where_sql.contains("cu_limit"));
    }

    #[test]
    fn dow_hour_binds_tz_once_and_reuses_the_placeholder() {
        let (where_sql, args) = build_grouped_tokens_where(
            &[], &[], &[], None, SOL_BUCKET_WIDTH, Some(1), Some(15), "", filter(now(), now()),
        );
        assert_eq!(where_sql.matches("$3").count(), 2, "tz placeholder reused for both EXTRACTs");
        assert!(where_sql.contains("EXTRACT(DOW"));
        assert!(where_sql.contains("EXTRACT(HOUR"));
        assert_eq!(args.len(), 5); // from, to, tz, dow, hour
    }

    #[test]
    fn search_matches_mint_or_symbol_lowercased() {
        let (where_sql, args) = build_grouped_tokens_where(
            &[], &[], &[], None, SOL_BUCKET_WIDTH, None, None, "  BONK  ", filter(now(), now()),
        );
        assert!(where_sql.contains("LOWER(t.mint_address) LIKE"));
        assert!(where_sql.contains("LOWER(t.symbol) LIKE"));
        match args.last() {
            Some(SqlArg::Str(s)) => assert_eq!(s, "bonk"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn order_defaults_to_newest_first() {
        assert_eq!(build_grouped_tokens_order(&[]), "t.created_at DESC, t.mint_address DESC");
        // Unknown column ⇒ dropped, same fallback.
        assert_eq!(
            build_grouped_tokens_order(&[("bogus".to_string(), true)]),
            "t.created_at DESC, t.mint_address DESC"
        );
    }

    #[test]
    fn order_maps_known_column_with_tiebreak() {
        let sql = build_grouped_tokens_order(&[("cu_limit".to_string(), true)]);
        assert!(sql.contains("t.cu_limit DESC NULLS LAST"));
        assert!(sql.ends_with("t.mint_address ASC"));
    }
}
