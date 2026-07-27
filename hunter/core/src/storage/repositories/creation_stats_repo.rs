use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;

use hunter_engine::fingerprint::configured_labels;

use crate::config::constants::lamports_to_sol;
use crate::grouping::{bucket_sol_label, decimals_for, GroupField};
use crate::models::Fingerprint;
use crate::storage::ix_labels_sql::{ix_labels_elements_sql, ix_labels_ordered_eq_sql};

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
        // preserves array position; duplicates are kept intentionally. Unwraps
        // both bare-array and `{instructions:[…]}` shapes (see ix_labels_sql).
        GroupField::IxLabels => format!(
            "COALESCE((SELECT string_agg(e.val, ' | ' ORDER BY e.ord) \
              FROM {} WITH ORDINALITY AS e(val, ord)), '∅')",
            ix_labels_elements_sql("t.ix_labels")
        ),
    }
}

/// Render a saved fingerprint as a `group_key` JSON object (same `" | "`-joined
/// `ix_labels` + bucketed SOL labels the manual `grouped()` path emits). Used by
/// the scoped dashboard so the single `g = 0` card displays the fingerprint's
/// axes as-is — including `ix_labels` — instead of an empty `{}` "ALL" key.
pub fn group_key_from_fingerprint(fp: &Fingerprint) -> JsonValue {
    let width = fp.bucket_size_amount;
    let decimals = decimals_for(width);
    let mut map = serde_json::Map::new();
    if let Some(v) = fp.cu_limit {
        map.insert("cu_limit".into(), json!(v.to_string()));
    }
    if let Some(v) = fp.cu_price {
        map.insert("cu_price".into(), json!(v.to_string()));
    }
    if let Some(l) = fp.init_buy_lamports {
        map.insert(
            "initial_buy_sol".into(),
            json!(bucket_sol_label(lamports_to_sol(l), width, decimals)),
        );
    }
    if let Some(l) = fp.max_cost_lamports {
        map.insert(
            "max_cost_lamports".into(),
            json!(bucket_sol_label(lamports_to_sol(l), width, decimals)),
        );
    }
    if let Some(l) = fp.spendable_lamports_in {
        map.insert(
            "spendable_lamports_in".into(),
            json!(bucket_sol_label(lamports_to_sol(l), width, decimals)),
        );
    }
    if let Some(l) = fp.first_slot_buy_lamports {
        map.insert(
            "first_slot_buy_sol".into(),
            json!(bucket_sol_label(lamports_to_sol(l), width, decimals)),
        );
    }
    if let Some(l) = fp.first_slot_sell_lamports {
        map.insert(
            "first_slot_sell_sol".into(),
            json!(bucket_sol_label(lamports_to_sol(l), width, decimals)),
        );
    }
    if let Some(labels) = configured_labels(fp.ix_labels.as_deref()) {
        map.insert("ix_labels".into(), json!(labels.join(" | ")));
    }
    JsonValue::Object(map)
}

/// SQL predicate pinning a continuous SOL token expression to the SAME bucket
/// as a fingerprint's own axis value, at `width` — the SQL mirror of
/// `hunter_engine::grouping::same_bucket` (kept in lockstep: identical
/// `+ BUCKET_EPS` boundary epsilon). `fp_value_sol` is computed from a trusted
/// numeric field on the `Fingerprint` DB row (never user free-text), so
/// literal-embedding it is injection-safe — same convention `sol_bucket_sql`
/// already uses for `width`. No leading `AND`; join clauses with `" AND "`.
fn bucket_eq_clause(sol_expr: &str, fp_value_sol: f64, width: f64) -> String {
    let idx = crate::grouping::bucket_index(fp_value_sol, width);
    format!(
        "floor(({sol_expr}) / {width} + {eps}) = {idx}",
        eps = crate::grouping::BUCKET_EPS,
    )
}

/// Every configured-axis clause for the "scope by saved fingerprint" path
/// (exact `cu_limit`/`cu_price`, bucketed SOL axes) — the SQL mirror of
/// `hunter_engine::fingerprint::matches`. `ix_labels` is excluded: it's the only
/// *bound* (not literal) predicate, since labels are arbitrary on-chain text
/// rather than a trusted numeric column — callers add it themselves via a
/// `t.ix_labels = $n` bind (see [`CreationStatsRepo::grouped_scoped`] /
/// [`build_grouped_tokens_where_scoped`]). `ti_alias` is the `tokens_info` LEFT
/// JOIN alias the first-slot buy/sell axes read off of — `"ti"` from
/// `grouped_scoped`, `"i"` from the drill-down's `token_repo` query (see
/// [`group_field_sql`]'s doc for why the two differ).
///
/// An all-`None` fingerprint mirrors the engine matcher's guard (a fingerprint
/// with no criteria never matches "everything") with a single `FALSE` clause.
///
/// The bucket width is read off `fp` and is **deliberately not a parameter**:
/// this is a second implementation of `hunter_engine::fingerprint::matches`, so
/// any width substituted on this side (an "unset ⇒ default" fallback, a caller's
/// own width) silently makes the dashboard's matched-token count disagree with
/// what the live engine actually arms — the reassuring number wins and the
/// divergence goes unnoticed. `Fingerprint::validate` + the `0014` CHECK
/// guarantee the stored width is usable, so there is nothing left to substitute.
/// `fingerprint_scope_sql_buckets_at_the_engine_width` locks the placement.
fn fingerprint_scope_clauses(fp: &Fingerprint, ti_alias: &str) -> Vec<String> {
    if !fp.has_any_criterion() {
        return vec!["FALSE".to_string()];
    }
    let width = fp.bucket_size_amount;
    let mut out = Vec::new();
    if let Some(v) = fp.cu_limit {
        out.push(format!("t.cu_limit = {v}"));
    }
    if let Some(v) = fp.cu_price {
        out.push(format!("t.cu_price = {v}"));
    }
    if let Some(l) = fp.init_buy_lamports {
        out.push(bucket_eq_clause("t.initial_buy_lamports::float8 / 1e9", lamports_to_sol(l), width));
    }
    if let Some(l) = fp.max_cost_lamports {
        out.push(bucket_eq_clause(
            "(t.initial_buy_instruction->>'max_cost_lamports')::float8 / 1e9",
            lamports_to_sol(l),
            width,
        ));
    }
    if let Some(l) = fp.spendable_lamports_in {
        out.push(bucket_eq_clause(
            "(t.initial_buy_instruction->>'spendable_lamports_in')::float8 / 1e9",
            lamports_to_sol(l),
            width,
        ));
    }
    if let Some(l) = fp.first_slot_buy_lamports {
        out.push(bucket_eq_clause(
            &format!("{ti_alias}.first_slot_buy_lamports::float8 / 1e9"),
            lamports_to_sol(l),
            width,
        ));
    }
    if let Some(l) = fp.first_slot_sell_lamports {
        out.push(bucket_eq_clause(
            &format!("{ti_alias}.first_slot_sell_lamports::float8 / 1e9"),
            lamports_to_sol(l),
            width,
        ));
    }
    out
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
            let elems = ix_labels_elements_sql("t.ix_labels");
            preds.push_str(&format!(
                "\n  AND (SELECT array_agg(DISTINCT e.val ORDER BY e.val) \
                 FROM {elems} AS e(val)) = ${idx}"
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

    /// The "scope by saved fingerprint" path: a single "ALL" group (`g = 0`) over
    /// tokens the fingerprint's own axes select — the SQL mirror of the sweep's
    /// and flow discovery's `fp_to_engine` + `hunter_engine::fingerprint::matches`
    /// scoping (exact `cu_limit`/`cu_price`/`ix_labels`, SOL axes by the
    /// fingerprint's own bucket width), so a scoped dashboard reads the same
    /// corpus a scoped sweep/discovery run would. Manual `group_by` /
    /// `field_filters` / `ix_labels_filter` don't apply here (same contract).
    pub async fn grouped_scoped(
        &self,
        fp: &Fingerprint,
        bucket_unit: &str,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<GroupedCreation> {
        let mut preds = String::new();
        for clause in fingerprint_scope_clauses(fp, "ti") {
            preds.push_str(&format!("\n  AND {clause}"));
        }
        // ix_labels: ordered exact match over the unwrapped label sequence
        // (handles bare-array + `{instructions:[…]}`) — NOT `grouped()`'s
        // sorted-set-equality `ix_labels_filter`. Bound as `text[]`. Owned so
        // it can be re-bound across all three queries.
        let ix_bind: Option<Vec<String>> =
            configured_labels(fp.ix_labels.as_deref()).map(<[String]>::to_vec);
        if ix_bind.is_some() {
            preds.push_str(&format!(
                "\n  AND {}",
                ix_labels_ordered_eq_sql("t.ix_labels", "$6")
            ));
        }

        let bkt = bucket_expr("(t.created_at AT TIME ZONE $1)", bucket_unit);
        let cte = format!(
            r#"
            WITH base AS (
                SELECT EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                       EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                       {bkt} AS bkt
                FROM tokens t
                LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
                WHERE t.created_at >= $2 AND t.created_at < $3
                  AND ($4::bool IS NULL OR t.is_mayhem_mode = $4)
                  AND ($5::bool IS NULL OR t.is_cashback_enabled = $5){preds}
            )
            "#,
        );
        // Placeholder `group_key` (`{}`) is overwritten below with
        // [`group_key_from_fingerprint`] so the card shows the fingerprint axes
        // (incl. `ix_labels`) as-is. Ungrouped `HAVING` collapses to zero rows
        // when the corpus is empty (rather than one row reading `total = 0`).
        let groups_sql =
            format!("{cte} SELECT 0::bigint AS g, '{{}}'::jsonb AS group_key, COUNT(*)::bigint AS total FROM base HAVING COUNT(*) > 0");
        let cells_sql = format!(
            "{cte} SELECT 0::bigint AS g, dow, hour, COUNT(*)::bigint AS count FROM base GROUP BY dow, hour"
        );
        let points_sql =
            format!("{cte} SELECT 0::bigint AS g, bkt AS bucket, COUNT(*)::bigint AS count FROM base GROUP BY bkt ORDER BY bkt");

        macro_rules! run {
            ($sql:expr, $ty:ty) => {{
                let mut q = sqlx::query_as::<_, $ty>($sql)
                    .bind(f.tz)
                    .bind(f.from)
                    .bind(f.to)
                    .bind(f.mayhem)
                    .bind(f.cashback);
                if let Some(labels) = &ix_bind {
                    q = q.bind(labels.as_slice());
                }
                q.fetch_all(&self.pool).await?
            }};
        }

        let mut groups = run!(&groups_sql, GroupedGroupRow);
        let cells = run!(&cells_sql, GroupedHeatCellRow);
        let points = run!(&points_sql, GroupedTrendPointRow);

        // Scoped path always emits one logical group — stamp its key from the
        // fingerprint so the card shows cu_*/ix_labels/… as-is (create-from-card
        // + fp badge matching reuse the same identity).
        let gk = group_key_from_fingerprint(fp);
        for g in &mut groups {
            g.group_key = gk.clone();
        }

        Ok(GroupedCreation { groups, cells, points })
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
        let elems = ix_labels_elements_sql("t.ix_labels");
        clauses.push(format!(
            "(SELECT array_agg(DISTINCT e.val ORDER BY e.val) \
              FROM {elems} AS e(val)) = ${}",
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

/// Same contract as [`build_grouped_tokens_where`], but for the "scope by saved
/// fingerprint" path (`fingerprint_id` set on the request): pins the corpus to
/// the tokens [`CreationStatsRepo::grouped_scoped`] selected instead of a manual
/// `group_by`/`field_filters`/`group_key` — there's only ever one group (`g = 0`),
/// so no `group_key` disambiguation is needed.
pub fn build_grouped_tokens_where_scoped(
    fp: &Fingerprint,
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

    // Fingerprint scope — runs against `token_repo`'s query, which joins
    // `tokens_info` as `i` (NOT `grouped_scoped`'s `ti`; see `group_field_sql`).
    clauses.extend(fingerprint_scope_clauses(fp, "i"));
    if let Some(labels) = configured_labels(fp.ix_labels.as_deref()) {
        args.push(SqlArg::StrArray(labels.to_vec()));
        let ph = format!("${}", args.len());
        clauses.push(ix_labels_ordered_eq_sql("t.ix_labels", &ph));
    }

    // Recurring weekly slot (a heatmap tile) — identical to `build_grouped_tokens_where`.
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

    // `clauses` always has the two window bounds, so this never falls back to
    // the `build_grouped_tokens_where` "TRUE" empty case.
    (clauses.join(" AND "), args)
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
    fn group_key_from_fingerprint_includes_ix_labels_structure() {
        let now = Utc::now();
        let fp = Fingerprint {
            id: uuid::Uuid::nil(),
            name: "t".into(),
            cu_limit: Some(200_000),
            cu_price: Some(1_000),
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: 0.1,
            ix_labels: Some(vec!["Pump.Fun: Create".into(), "Pump.Fun: Buy".into()]),
            metric_config: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        let gk = group_key_from_fingerprint(&fp);
        assert_eq!(gk["cu_limit"], "200000");
        assert_eq!(gk["cu_price"], "1000");
        assert_eq!(gk["ix_labels"], "Pump.Fun: Create | Pump.Fun: Buy");
    }

    /// A fingerprint with no axis set, at `width`.
    fn blank_fp(width: f64) -> Fingerprint {
        let now = Utc::now();
        Fingerprint {
            id: uuid::Uuid::nil(),
            name: "t".into(),
            cu_limit: None,
            cu_price: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: width,
            ix_labels: None,
            metric_config: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        }
    }

    /// Every bucket-matched SOL axis, as (setter, the token-side SQL expression the
    /// clause builder buckets). All five share the row's ONE `bucket_size_amount`,
    /// so the guard below has to walk all five — a per-axis width would be a bug in
    /// itself. `ti_alias` is `"ti"` throughout (`grouped_scoped`'s join alias).
    #[allow(clippy::type_complexity)]
    const SOL_AXES: &[(fn(&mut Fingerprint, Option<i64>), &str)] = &[
        (|fp, v| fp.init_buy_lamports = v, "t.initial_buy_lamports::float8 / 1e9"),
        (
            |fp, v| fp.max_cost_lamports = v,
            "(t.initial_buy_instruction->>'max_cost_lamports')::float8 / 1e9",
        ),
        (
            |fp, v| fp.spendable_lamports_in = v,
            "(t.initial_buy_instruction->>'spendable_lamports_in')::float8 / 1e9",
        ),
        (|fp, v| fp.first_slot_buy_lamports = v, "ti.first_slot_buy_lamports::float8 / 1e9"),
        (|fp, v| fp.first_slot_sell_lamports = v, "ti.first_slot_sell_lamports::float8 / 1e9"),
    ];

    /// One-axis fingerprint at `width`, for the `axis`-th entry of [`SOL_AXES`].
    fn one_axis_fp(axis: usize, lamports: i64, width: f64) -> Fingerprint {
        let mut fp = blank_fp(width);
        SOL_AXES[axis].0(&mut fp, Some(lamports));
        fp
    }

    /// SSOT guard (no DB): the "scope by saved fingerprint" SQL is a second
    /// implementation of `hunter_engine::fingerprint::matches`, so **every** one of
    /// the five bucket-matched SOL axes must bucket at the fingerprint's OWN
    /// `bucket_size_amount` and place every value exactly where the engine does.
    ///
    /// This fails on a revert to the old `fingerprint_bucket_width` fallback
    /// (`0 ⇒ 0.1`), on any drift in the `floor(v / w + eps)` form or the epsilon,
    /// and on any axis quietly acquiring a width of its own — the two surfaces
    /// would then disagree about which tokens a fingerprint matches, with the
    /// dashboard showing the reassuring number.
    #[test]
    fn fingerprint_scope_sql_buckets_every_sol_axis_at_the_engine_width() {
        use crate::grouping::{bucket_index, same_bucket, BUCKET_EPS};

        // A 0-SOL axis is a REAL value (bucket `[0, width)`), not "unset" — it is
        // in the matrix on purpose.
        for (axis, (_, sol_expr)) in SOL_AXES.iter().enumerate() {
            for width in [1e-6f64, 0.05, 0.1, 0.25, 1.0, 5.0] {
                for fp_sol in [0.0f64, 0.1, 0.5, 1.0, 2.34, 8.0] {
                    let lamports = (fp_sol * 1e9).round() as i64;
                    let fp = one_axis_fp(axis, lamports, width);
                    let clauses = fingerprint_scope_clauses(&fp, "ti");
                    assert_eq!(clauses.len(), 1, "one configured axis ⇒ one clause");

                    // The emitted literal must be the engine's own bucket index at
                    // the fingerprint's own width — no substituted default. Compare
                    // through the same lamports→SOL conversion the clause builder
                    // uses so this tests the bucketing, not f64 round-tripping.
                    let fp_sol = lamports_to_sol(lamports);
                    let idx = bucket_index(fp_sol, width);
                    let expected =
                        format!("floor(({sol_expr}) / {width} + {BUCKET_EPS}) = {idx}");
                    assert_eq!(clauses[0], expected, "axis={axis} width={width} fp={fp_sol}");

                    // …and the SQL's arithmetic must agree with `same_bucket` on
                    // which token values land in it (the predicate Postgres will
                    // evaluate, replicated here in f64 exactly as the text reads).
                    for tok_sol in [0.0, 0.05, 0.1, 0.3, 0.5, 1.0, 1.05, 2.34, 8.0] {
                        let sql_hit = ((tok_sol / width) + BUCKET_EPS).floor() as i64 == idx;
                        assert_eq!(
                            sql_hit,
                            same_bucket(tok_sol, fp_sol, width),
                            "axis={axis} width={width} fp={fp_sol} tok={tok_sol}",
                        );
                    }
                }
            }
        }
    }

    /// A zero-SOL axis must reach the SQL as a real `= 0` bucket on every axis,
    /// never be skipped as "unset" — the mirror of the `Option` semantics the
    /// engine matcher uses. `None` remains the only way to say "not configured".
    #[test]
    fn zero_sol_axis_emits_its_own_bucket_clause() {
        for axis in 0..SOL_AXES.len() {
            let fp = one_axis_fp(axis, 0, 1.0);
            let clauses = fingerprint_scope_clauses(&fp, "ti");
            assert_eq!(clauses.len(), 1, "axis {axis}: a 0-lamport axis must emit a clause");
            assert!(clauses[0].ends_with("= 0"), "axis {axis}: expected bucket 0: {}", clauses[0]);
        }
        // …while an all-absent fingerprint is fenced off entirely rather than
        // matching every token.
        assert_eq!(
            fingerprint_scope_clauses(&blank_fp(1.0), "ti"),
            vec!["FALSE".to_string()],
        );

        // `Some([])` is the SAME state as `None` (see `configured_labels`), so it
        // must fence too. It previously satisfied `has_any_criterion`, skipped the
        // FALSE guard, and then emitted NO predicates — leaving the scoped
        // dashboard matching every token in the window while the engine matcher
        // (which does not count empty labels) matched none.
        let mut empty_labels = blank_fp(1.0);
        empty_labels.ix_labels = Some(vec![]);
        assert_eq!(
            fingerprint_scope_clauses(&empty_labels, "ti"),
            vec!["FALSE".to_string()],
            "an empty label list must never widen the scope to every token",
        );
    }

    /// The `g = 0` scoped card labels its axes at the same width the clauses bucket
    /// at, so the card a user reads describes the rows they were actually served.
    #[test]
    fn scoped_group_key_labels_at_the_fingerprint_width() {
        let mut fp = blank_fp(1.0);
        fp.spendable_lamports_in = Some(0);
        fp.init_buy_lamports = Some(2_500_000_000); // 2.5 SOL @ 1.0 ⇒ [2, 3)
        let gk = group_key_from_fingerprint(&fp);
        assert_eq!(gk["spendable_lamports_in"], "0–1", "a 0 axis labels as its own bucket");
        assert_eq!(gk["initial_buy_sol"], "2–3");

        let mut fp = blank_fp(0.1);
        fp.spendable_lamports_in = Some(1_050_000_000); // 1.05 SOL @ 0.1 ⇒ [1.0, 1.1)
        let gk = group_key_from_fingerprint(&fp);
        assert_eq!(gk["spendable_lamports_in"], "1.0–1.1");
    }

    #[test]
    fn ix_labels_group_field_sql_unwraps_object_shape() {
        let sql = group_field_sql(GroupField::IxLabels, SOL_BUCKET_WIDTH, "ti");
        assert!(sql.contains("t.ix_labels->'instructions'"), "dual-shape unwrap missing: {sql}");
        assert!(sql.contains("string_agg"), "ordered join missing: {sql}");
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
