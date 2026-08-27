use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;

use hunter_engine::fingerprint::{AxisId, AxisPredicate};

use crate::config::constants::tuning::PRIOR_LAUNCH_WINDOW_DAYS;
use crate::grouping::{parse_filter, GroupField, GroupKey, GroupPlan, GroupValue, PartitionSpec};
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

/// The outcome-maturity censoring predicate, shared **verbatim** by the outcome
/// columns (`matured`/`known`/`migrated`/`dead`) and the trade-metric columns
/// (`trades`/`trades_per_day`/`trades_avg`) in both [`CreationStatsRepo::heatmap`]
/// and [`CreationStatsRepo::trend`] — a fresh token shouldn't read as inactive
/// any more than it should read as dead (trade-counts plan §2). Defined once so
/// a future edit to the censoring rule can't drift between the two column
/// families; bound `$2` is `f.maturity_secs`.
const MATURED_PRED: &str = "t.created_at < now() - make_interval(secs => $2)";

/// The four trade-metric SELECT columns (`trades`/`volume_sol`/`trades_per_day`/
/// `trades_avg`), byte-identical between [`CreationStatsRepo::heatmap`] and
/// [`CreationStatsRepo::trend`] (trade-counts plan §3) — factored into one
/// function so the two call sites can't drift, and so it's unit-testable
/// without a DB. Every `FILTER` reuses [`MATURED_PRED`] verbatim.
fn trade_metrics_sql() -> String {
    format!(
        r#"COALESCE(SUM(ti.trade_count) FILTER (WHERE {m}), 0)::bigint AS trades,
                COALESCE(SUM(ti.volume_sol) FILTER (WHERE {m}), 0)::float8 AS volume_sol,
                COALESCE(
                    SUM(ti.trade_count / GREATEST(EXTRACT(EPOCH FROM (now() - t.created_at)) / 86400.0, 1))
                        FILTER (WHERE {m}),
                    0
                )::float8 AS trades_per_day,
                SUM(ti.trade_count) FILTER (WHERE {m})::float8
                    / NULLIF(COUNT(*) FILTER (WHERE {m} AND ti.mint_address IS NOT NULL), 0) AS trades_avg"#,
        m = MATURED_PRED,
    )
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
    /// Lifetime-to-last-sync trade count, summed over matured+known tokens
    /// (`tokens_info.trade_count`). See the trade-counts plan §2 for the age-bias
    /// caveat — prefer `trades_per_day` when comparing cohorts of different ages.
    pub trades: i64,
    /// Lifetime-to-last-sync SOL volume, same censoring as `trades`.
    pub volume_sol: f64,
    /// Age-normalized `SUM(trade_count / age_days)` — composes across buckets
    /// (a plain `SUM`), unlike `trades_avg`. The metric that answers "is this
    /// cohort still actively traded, adjusted for how long it's had to trade".
    pub trades_per_day: f64,
    /// Mean trades per token (`SUM/COUNT`, not a median — see plan §1/§2).
    /// `NULL` when the cell has no matured+known token (`NULLIF` on the
    /// denominator), so the UI renders "no data" instead of a misleading 0.
    pub trades_avg: Option<f64>,
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
    /// See [`HeatCellRow::trades`].
    pub trades: i64,
    /// See [`HeatCellRow::volume_sol`].
    pub volume_sol: f64,
    /// See [`HeatCellRow::trades_per_day`].
    pub trades_per_day: f64,
    /// See [`HeatCellRow::trades_avg`].
    pub trades_avg: Option<f64>,
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

    /// 7×24 seasonality fold (counts + censored outcome + trade columns per cell).
    pub async fn heatmap(&self, f: StatsFilter<'_>) -> anyhow::Result<Vec<HeatCellRow>> {
        let sql = format!(
            r#"
            SELECT
                EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                COUNT(*)::bigint AS total,
                COUNT(*) FILTER (WHERE {m})::bigint AS matured,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.mint_address IS NOT NULL)::bigint AS known,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.is_migrated)::bigint AS migrated,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.is_dead)::bigint AS dead,
                {trade_metrics}
            FROM tokens t
            LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
            WHERE t.created_at >= $3 AND t.created_at < $4
              AND ($5::bool IS NULL OR t.is_mayhem_mode = $5)
              AND ($6::bool IS NULL OR t.is_cashback_enabled = $6)
            GROUP BY 1, 2
            "#,
            m = MATURED_PRED,
            trade_metrics = trade_metrics_sql(),
        );
        let rows = sqlx::query_as::<_, HeatCellRow>(&sql)
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
                COUNT(*) FILTER (WHERE {m})::bigint AS matured,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.mint_address IS NOT NULL)::bigint AS known,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.is_migrated)::bigint AS migrated,
                COUNT(*) FILTER (WHERE {m}
                                   AND ti.is_dead)::bigint AS dead,
                {trade_metrics}
            FROM tokens t
            LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
            WHERE t.created_at >= $3 AND t.created_at < $4
              AND ($5::bool IS NULL OR t.is_mayhem_mode = $5)
              AND ($6::bool IS NULL OR t.is_cashback_enabled = $6)
            GROUP BY 1
            ORDER BY 1
            "#,
            m = MATURED_PRED,
            trade_metrics = trade_metrics_sql(),
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
    /// Lifetime-to-last-sync trade count summed over the group (count-only —
    /// no maturity censoring, matching `total`'s own volume-view contract).
    pub trades: i64,
    /// `trades::float8 / total::float8` — the per-token figure `rank_by=
    /// trades_per_token` ranks on; `total` is never 0 for a returned group.
    pub trades_avg: f64,
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
/// Integer digit positions in every `to_char` group-key mask.
///
/// **Sized for the worst case, because `to_char` fails LOUDLY-but-silently:** a
/// value too wide for its mask renders as `########`, and since that is a perfectly
/// good TEXT group key it becomes a real (wrong) group instead of an error. The old
/// 8-position mask did exactly that to every token carrying pump.fun's
/// `max_cost_lamports = u64::MAX` "no slippage limit" sentinel (≈1.84e10 SOL) —
/// 11,250 of them in a 30-day window on 2026-08-04, all collapsed into one
/// `########.#` group, and silently disagreeing with `bucket_sol_label`, which the
/// mirror is supposed to match byte-for-byte.
///
/// 18 positions covers `u64::MAX` lamports as SOL (11 digits) with room to spare;
/// The token-side SQL expression for one **numeric** axis, as `numeric` — the
/// mirror of `AxisId::read_num`. Every numeric axis has one, so a scoped query can
/// reproduce the matcher exactly rather than quietly skipping an axis it cannot
/// express — skipping WIDENS the corpus, which is the direction that shows a
/// reassuring number instead of a wrong one.
///
/// `numeric`, never `float8`: a `max_sol_cost` ceiling is `u64::MAX`, and `float8`
/// stops being injective at 2^53, so two distinct amounts up there would compare
/// equal. `ti_alias` is the `tokens_info` LEFT JOIN alias the first-slot axes read
/// off — `"ti"` from [`CreationStatsRepo::grouped`], `"i"` from the drill-down's
/// `token_repo` query.
fn axis_num_sql(axis: AxisId, ti_alias: &str) -> Option<String> {
    Some(match axis {
        AxisId::CuLimit => "t.cu_limit::numeric".to_string(),
        AxisId::CuPrice => "t.cu_price::numeric".to_string(),
        AxisId::InitBuyLamports => "t.initial_buy_lamports::numeric".to_string(),
        AxisId::MaxCostLamports => {
            "(t.initial_buy_instruction->>'max_cost_lamports')::numeric".to_string()
        }
        AxisId::SpendableLamportsIn => {
            "(t.initial_buy_instruction->>'spendable_lamports_in')::numeric".to_string()
        }
        AxisId::FirstSlotBuyLamports => format!("{ti_alias}.first_slot_buy_lamports::numeric"),
        AxisId::FirstSlotSellLamports => format!("{ti_alias}.first_slot_sell_lamports::numeric"),
        // Derived from the labels, exactly as the engine derives it — never a
        // stored column, so the two can never disagree about one transaction.
        AxisId::IxCount => format!(
            "COALESCE(jsonb_array_length({}), 0)::numeric",
            crate::storage::ix_labels_sql::ix_labels_array_sql("t.ix_labels")
        ),
        // The engine counts this in memory over a trailing window; the mirror counts
        // the same window off `tokens`, so a scoped dashboard and the live gate agree
        // on what "prior" means. Bounded on BOTH ends deliberately — unbounded, the
        // same creator would read differently in a backtest than live, because the
        // answer would depend on how long the table has existed.
        AxisId::PriorLaunches => format!(
            "(SELECT count(*) FROM tokens p WHERE p.creator_wallet = t.creator_wallet \
              AND p.created_at < t.created_at \
              AND p.created_at >= t.created_at - INTERVAL '{PRIOR_LAUNCH_WINDOW_DAYS} days')::numeric"
        ),
        AxisId::IxLabels => return None,
    })
}

/// SQL producing one field's [`GroupValue`] as `jsonb` — the mirror of
/// `grouping::field_value`.
///
/// **The key carries the predicate, not a rendered label.** That is what removed the
/// byte-identical-label lockstep this file used to owe the engine: there is no
/// `to_char` mask to match, no `1e-9` boundary epsilon to reproduce, and no float
/// division whose rounding both sides had to get wrong in the same way. Two integers
/// either name the same window or they do not.
fn group_value_sql(f: GroupField, spec: &PartitionSpec, ti_alias: &str) -> String {
    let missing = "jsonb_build_object('kind', 'missing')";
    match f {
        GroupField::TokenProgramId => format!(
            "CASE WHEN t.token_program_id IS NULL THEN {missing} \
             ELSE jsonb_build_object('kind', 'text', 'value', t.token_program_id) END"
        ),
        GroupField::IsCashbackEnabled => {
            "jsonb_build_object('kind', 'flag', 'value', t.is_cashback_enabled)".to_string()
        }
        // An EMPTY label list is the same sentinel as absent — the matcher's own rule,
        // so the token side and the fingerprint side agree about what "unset" is.
        GroupField::IxLabels => {
            let agg = format!(
                "(SELECT jsonb_agg(e.val ORDER BY e.ord) FROM {} WITH ORDINALITY AS e(val, ord))",
                ix_labels_elements_sql("t.ix_labels")
            );
            format!(
                "CASE WHEN {agg} IS NULL THEN {missing} \
                 ELSE jsonb_build_object('kind', 'labels', 'labels', {agg}) END"
            )
        }
        other => {
            let axis = other.axis().expect("non-axis fields handled above");
            let expr = axis_num_sql(axis, ti_alias).expect("a numeric field has an expression");
            let window = match spec {
                // One group per distinct value: the degenerate window.
                PartitionSpec::Distinct => format!(
                    "jsonb_build_object('kind', 'window', 'min', trunc({expr})::text, \
                     'max', trunc({expr})::text)"
                ),
                // A CASE ladder over the explicit edges, mirroring
                // `PartitionSpec::window_for`. The edges are ascending integers held
                // in the run's own spec — never user free text — so interpolating them
                // is injection-safe by construction.
                PartitionSpec::Ranges { edges } => {
                    let mut arms = String::new();
                    for (i, e) in edges.iter().enumerate() {
                        let min = if i == 0 {
                            "'min', NULL".to_string()
                        } else {
                            format!("'min', '{}'", edges[i - 1])
                        };
                        arms.push_str(&format!(
                            " WHEN trunc({expr}) < {e} THEN jsonb_strip_nulls(\
                               jsonb_build_object('kind', 'window', {min}, 'max', '{}'))",
                            e - 1
                        ));
                    }
                    let last = edges.last().copied().unwrap_or(0);
                    format!(
                        "CASE{arms} ELSE jsonb_build_object('kind', 'window', 'min', '{last}') END"
                    )
                }
            };
            format!("CASE WHEN {expr} IS NULL THEN {missing} ELSE {window} END")
        }
    }
}

/// A saved fingerprint rendered as a [`GroupKey`](crate::grouping::GroupKey) JSON
/// object, so the scoped dashboard's single `g = 0` card displays the axes the
/// fingerprint actually matches instead of an empty "ALL" key.
///
/// A copy, not a re-derivation: a group key's window and a fingerprint's range are
/// the same type, so there is nothing to translate and nothing to get wrong.
pub fn group_key_from_fingerprint(fp: &Fingerprint) -> JsonValue {
    let mut map = serde_json::Map::new();
    for (axis, pred) in fp.criteria.iter() {
        let value = match pred {
            AxisPredicate::Range { min, max } => GroupValue::Window { min: *min, max: *max },
            AxisPredicate::Sequence { labels } => GroupValue::Labels { labels: labels.clone() },
        };
        map.insert(axis.key().to_string(), json!(value));
    }
    JsonValue::Object(map)
}

/// Every configured-axis clause for the "scope by saved fingerprint" path — the SQL
/// mirror of `hunter_engine::fingerprint::matches`. No leading `AND`; join the
/// clauses with `" AND "`.
///
/// Generated from the criteria map, so an axis added to the registry is mirrored
/// the moment [`axis_num_sql`] learns to read it. The retired form hand-listed five
/// columns and could silently omit one, which reads as a wider corpus, not an error.
///
/// `ix_labels` is excluded: it is the only *bound* (not literal) predicate, since
/// labels are arbitrary on-chain text rather than a trusted numeric column — callers
/// add it themselves via [`ix_labels_ordered_eq_sql`].
///
/// A criterion-less fingerprint mirrors the matcher's own guard (it never matches
/// "everything") with a single `FALSE`.
fn fingerprint_scope_clauses(fp: &Fingerprint, ti_alias: &str) -> Vec<String> {
    if !fp.has_any_criterion() {
        return vec!["FALSE".to_string()];
    }
    // A wildcard matches every token, so it constrains nothing — the mirror of the
    // matcher's own short-circuit, at the same position and for the same reason.
    // Spelled out rather than left to fall through the loop on the strength of the
    // `fingerprints_wildcard_excludes_axes` CHECK: the two readers of `wildcard` have
    // to reach this verdict independently, or an axis that slips past the CHECK
    // narrows the dashboard while the engine still arms on everything.
    if fp.wildcard {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (axis, pred) in fp.criteria.iter() {
        let AxisPredicate::Range { min, max } = pred else { continue };
        let Some(expr) = axis_num_sql(axis, ti_alias) else { continue };
        // A configured axis with no observed value FAILS, the same fail-closed
        // direction the matcher takes. `NULL BETWEEN a AND b` is NULL, which a
        // `WHERE` drops — the behaviour we want — but it is stated here rather than
        // relied on, because a caller that wraps these in a `NOT` would flip it.
        out.push(match (min, max) {
            (Some(a), Some(b)) if a == b => format!("({expr}) = {a}"),
            (Some(a), Some(b)) => format!("({expr}) BETWEEN {a} AND {b}"),
            (Some(a), None) => format!("({expr}) >= {a}"),
            (None, Some(b)) => format!("({expr}) <= {b}"),
            (None, None) => continue,
        });
    }
    out
}

/// Fold an ordered label sequence into a `group_key` object using the SAME
/// [`GroupValue::Labels`] shape [`group_value_sql`] emits when `ix_labels` is an
/// actual group field — so a filtered-but-ungrouped card reads identically to a
/// grouped one everywhere downstream (fingerprint identity, the "already a
/// fingerprint" badge, the card's own key display). A no-op when `ordered` is
/// `None`/empty, or `group_key` already has an `ix_labels` entry (the real grouped
/// case) — never overwrites.
fn fold_ordered_labels_into_group_key(group_key: &mut JsonValue, ordered: Option<Vec<String>>) {
    let Some(labels) = ordered else { return };
    if labels.is_empty() {
        return;
    }
    if let JsonValue::Object(map) = group_key {
        if !map.contains_key("ix_labels") {
            map.insert("ix_labels".to_string(), json!(GroupValue::Labels { labels }));
        }
    }
}

/// One per-field value filter, lowered to SQL. Returns the predicate plus the
/// `text[]` bind it needs (`None` ⇒ fully self-contained, bind nothing).
///
/// **Numeric fields** parse each typed value through
/// [`parse_filter`](crate::grouping::parse_filter) — the ONE filter parser every
/// surface shares — and compare against the axis's own numeric expression. Because
/// a parsed filter IS an [`AxisPredicate`], the filter box, the group key and the
/// live match all speak one vocabulary: a chip's own text pasted into the box
/// selects exactly that chip's tokens, by construction rather than by two
/// implementations agreeing.
///
/// **Discrete fields** (program id, cashback) compare the rendered group-key text
/// against a bound `text[]`; the typed value IS the key value there.
///
/// Bounds are parsed integers interpolated as literals — injection-safe by
/// construction (a parsed number, never user text), the same argument
/// [`fingerprint_scope_clauses`] makes. A value that doesn't parse is dropped; if
/// that leaves nothing, the filter becomes `FALSE` rather than silently passing
/// every row — a dropped filter reads as "no filter", which WIDENS the query.
///
/// `bind_placeholder` (e.g. `"$7"`) is consumed **only** when the returned bind is
/// `Some`, so callers must not advance their parameter counter otherwise.
fn field_filter_pred(
    field: GroupField,
    values: &[String],
    ti_alias: &str,
    bind_placeholder: &str,
) -> (String, Option<Vec<String>>) {
    let (Some(axis), Some(unit)) = (field.axis(), field.unit()) else {
        // Discrete: compare the group-key text against a bound array.
        let expr = match field {
            GroupField::TokenProgramId => "COALESCE(t.token_program_id, '∅')".to_string(),
            GroupField::IsCashbackEnabled => "t.is_cashback_enabled::text".to_string(),
            // `ix_labels` filters through `ix_labels_ordered_eq_sql`, never here.
            _ => return ("FALSE".to_string(), None),
        };
        return (format!("{expr} = ANY({bind_placeholder})"), Some(values.to_vec()));
    };
    let Some(expr) = axis_num_sql(axis, ti_alias) else { return ("FALSE".to_string(), None) };
    let mut terms: Vec<String> = Vec::new();
    for pred in values.iter().filter_map(|v| parse_filter(v, unit)) {
        let AxisPredicate::Range { min, max } = pred else { continue };
        terms.push(match (min, max) {
            (Some(a), Some(b)) if a == b => format!("({expr}) = {a}"),
            (Some(a), Some(b)) => format!("(({expr}) BETWEEN {a} AND {b})"),
            (Some(a), None) => format!("({expr}) >= {a}"),
            (None, Some(b)) => format!("({expr}) <= {b}"),
            (None, None) => continue,
        });
    }
    match terms.len() {
        0 => ("FALSE".to_string(), None),
        1 => (terms.pop().unwrap(), None),
        _ => (format!("({})", terms.join(" OR "))
            , None),
    }
}





/// Per-group time-series for the grouped dashboard section.
pub struct GroupedCreation {
    pub groups: Vec<GroupedGroupRow>,
    pub cells: Vec<GroupedHeatCellRow>,
    pub points: Vec<GroupedTrendPointRow>,
}

/// SQL `ORDER BY` fragment for the grouped-ranking tag — whitelisted by the
/// handler's `normalize_rank_by`, never user free-text (same discipline as
/// [`bucket_expr`]). `trade_count` is `base`'s own column (`ti.trade_count`,
/// carried through the LEFT JOIN already in `base`'s SELECT), so ranking by
/// trades costs nothing beyond the existing per-group fold — no new join/scan.
/// `trades_per_token` is the one that actually fixes the grouped section's
/// motivating example (a big group of mediocre launches out-ranking a small
/// elite one) — raw `trades` still scales with group size exactly like
/// `COUNT(*)` does (trade-counts plan §5).
fn rank_by_order_sql(rank_by: &str) -> &'static str {
    match rank_by {
        "trades" => "COALESCE(SUM(trade_count), 0) DESC, gkey::text",
        "trades_per_token" => {
            "(COALESCE(SUM(trade_count), 0)::float8 / COUNT(*)::float8) DESC, gkey::text"
        }
        // Defensive: normalize_rank_by guarantees a known tag; fall back to count
        // (the "default does not change" rule — trade-counts plan §5/§7).
        _ => "COUNT(*) DESC, gkey::text",
    }
}

impl CreationStatsRepo {
    /// Partition tokens by a compound fingerprint key (`fields`, in order), keep
    /// the top-`top` groups by volume (or by `rank_by`) over the window, and
    /// return each group's day×hour fold (`cells`) and calendar trend (`points`).
    /// LEFT JOINs `tokens_info` for both the trade-derived group fields
    /// (`first_slot_buy_sol`/`first_slot_sell_sol`) and the per-group `trades`/
    /// `trades_avg` totals — the join is one-to-one on `mint_address`, so it
    /// doesn't change group cardinality. Count-only outcome-wise (no
    /// migrated/dead columns); shares the same TZ-aware bucketing + segment
    /// filter as [`heatmap`]/[`trend`]; the window is caller-clamped so the scan
    /// is bounded.
    // Each arg is an independent query dimension the handler already carries;
    // bundling them into a struct would only add indirection for one call site.
    #[allow(clippy::too_many_arguments)]
    pub async fn grouped(
        &self,
        // How each field is partitioned — the same plan the grouped sweep runs, so
        // the dashboard's groups ARE a sweep's groups ("swept = run").
        plan: &GroupPlan,
        bucket_unit: &str,
        top: i64,
        field_filters: &[(GroupField, Vec<String>)],
        ix_labels_filter: Option<&[String]>,
        // Ranking criterion: "count" (default) | "trades" | "trades_per_token" —
        // whitelisted by the handler's `normalize_rank_by`, never free text.
        rank_by: &str,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<GroupedCreation> {
        // Build the group-key JSON object expression from the selected fields.
        // Empty selection ⇒ a single "ALL" group (`{}`), like the sweep's ALL group.
        let gkey_sql = if plan.is_empty() {
            "'{}'::jsonb".to_string()
        } else {
            let pairs: Vec<String> = plan
                .0
                .iter()
                .map(|(fld, spec)| {
                    format!("'{}', {}", fld.as_str(), group_value_sql(*fld, spec, "ti"))
                })
                .collect();
            format!("jsonb_build_object({})", pairs.join(", "))
        };

        // Per-field value filters restrict the corpus *before* partitioning, so
        // only matching groups survive into the top-N. `field_filter_pred` is the
        // SSOT for how one lowers: discrete fields compare the rendered group-key
        // TEXT against a bound `text[]`, the bucketed SOL fields pin an exact
        // amount on their raw lamports column (they have no typeable group-key
        // form). `ix_labels` is an **exact ordered sequence** match through the
        // `ix_labels_ordered_eq_sql` SSOT — same semantics as the engine matcher
        // and the scoped path. Binds start at `$7` (after top=$6); only a
        // predicate that returned a bind advances `idx`, so predicate index and
        // bind order stay in lockstep.
        let mut preds = String::new();
        let mut filter_binds: Vec<Vec<String>> = Vec::new();
        let mut idx = 7;
        for (fld, vals) in field_filters {
            let (pred, bind) = field_filter_pred(*fld, vals, "ti", &format!("${idx}"));
            preds.push_str(&format!("\n  AND {pred}"));
            if let Some(b) = bind {
                filter_binds.push(b);
                idx += 1;
            }
        }
        if let Some(labels) = ix_labels_filter {
            preds.push_str(&format!(
                "\n  AND {}",
                ix_labels_ordered_eq_sql("t.ix_labels", &format!("${idx}"))
            ));
            filter_binds.push(labels.to_vec());
            idx += 1;
        }
        let _ = idx;

        // Shared CTE: window+segment-filtered rows with their group key + time
        // dimensions, then the top-N groups ranked by volume (g = 0-based rank).
        // Bucket expression is interpolated (whitelisted tag), so the old `$2`
        // bucket slot is gone and the fixed binds shift up by one.
        let bkt = bucket_expr("(t.created_at AT TIME ZONE $1)", bucket_unit);
        let order = rank_by_order_sql(rank_by);
        let cte = format!(
            r#"
            WITH base AS (
                SELECT {gkey} AS gkey,
                       EXTRACT(DOW  FROM (t.created_at AT TIME ZONE $1))::int AS dow,
                       EXTRACT(HOUR FROM (t.created_at AT TIME ZONE $1))::int AS hour,
                       {bkt} AS bkt,
                       ti.trade_count AS trade_count
                FROM tokens t
                LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
                WHERE t.created_at >= $2 AND t.created_at < $3
                  AND ($4::bool IS NULL OR t.is_mayhem_mode = $4)
                  AND ($5::bool IS NULL OR t.is_cashback_enabled = $5){preds}
            ),
            ranked AS (
                SELECT gkey, COUNT(*) AS total,
                       COALESCE(SUM(trade_count), 0)::bigint AS trades,
                       (row_number() OVER (ORDER BY {order}) - 1) AS g
                FROM base
                GROUP BY gkey
                ORDER BY {order}
                LIMIT $6
            )
            "#,
            gkey = gkey_sql,
        );

        // SQL strings bound to named locals so the queries (which borrow them)
        // outlive each statement. Bind the fixed params (renumbered) then the
        // per-field filter arrays; applied identically to all three sub-queries.
        let groups_sql = format!(
            "{cte} SELECT g::bigint AS g, gkey AS group_key, total::bigint AS total, \
             trades::bigint AS trades, (trades::float8 / total::float8) AS trades_avg \
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

        let mut groups = run!(&groups_sql, GroupedGroupRow);
        let cells = run!(&cells_sql, GroupedHeatCellRow);
        let points = run!(&points_sql, GroupedTrendPointRow);

        // The filter is an exact ordered sequence, so every row in every group
        // carries EXACTLY these labels — fold them straight into the key so a
        // filtered-but-ungrouped card seeds a fingerprint with the real axis
        // instead of silently dropping what the user pinned on the corpus.
        if let Some(labels) = ix_labels_filter {
            for g in &mut groups {
                fold_ordered_labels_into_group_key(&mut g.group_key, Some(labels.to_vec()));
            }
        }

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
        // (handles bare-array + `{instructions:[…]}`) — the same
        // `ix_labels_ordered_eq_sql` SSOT `grouped()`'s `ix_labels_filter` uses.
        // Bound as `text[]`. Owned so it can be re-bound across all three queries.
        let ix_bind: Option<Vec<String>> = match fp.criteria.get(AxisId::IxLabels) {
            Some(AxisPredicate::Sequence { labels }) => Some(labels.clone()),
            _ => None,
        };
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
                       {bkt} AS bkt,
                       ti.trade_count AS trade_count
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
        // `trades`/`trades_avg` mirror `grouped()`'s ranked-group output — there's
        // only ever one group here, so no ranking, just the same two columns.
        let groups_sql = format!(
            "{cte} SELECT 0::bigint AS g, '{{}}'::jsonb AS group_key, COUNT(*)::bigint AS total, \
             COALESCE(SUM(trade_count), 0)::bigint AS trades, \
             (COALESCE(SUM(trade_count), 0)::float8 / COUNT(*)::float8) AS trades_avg \
             FROM base HAVING COUNT(*) > 0"
        );
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
    plan: &GroupPlan,
    group_key: &GroupKey,
    field_filters: &[(GroupField, Vec<String>)],
    ix_labels_filter: Option<&[String]>,
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
    for (field, spec) in &plan.0 {
        let Some((_, val)) = group_key.0.iter().find(|(f2, _)| f2 == field) else {
            continue;
        };
        // Compare the whole `jsonb` value, not a rendered string: the key IS the
        // predicate, so this equality is the same one `grouped()`'s `ranked` CTE
        // applies per-`gkey`, just pinned to one rank instead of top-N'd.
        args.push(SqlArg::Str(json!(val).to_string()));
        // Runs against `token_repo`'s query (`TokenRepo::LIST_FROM`), which joins
        // `tokens_info` as `i` (NOT the `ti` `grouped()` uses) — see `group_value_sql`.
        clauses.push(format!("{} = ${}::jsonb", group_value_sql(*field, spec, "i"), args.len()));
    }

    // Corpus-level filters applied before the groups were ranked — through the
    // SAME `field_filter_pred` SSOT `grouped()` uses, so the drill-down's row set
    // and the card's count can't diverge. A bucketed-SOL predicate is
    // self-contained (no bind), so only the discrete arm pushes an arg.
    for (field, vals) in field_filters {
        let (pred, bind) = field_filter_pred(*field, vals, "i", &format!("${}", args.len() + 1));
        if let Some(b) = bind {
            args.push(SqlArg::StrArray(b));
        }
        clauses.push(pred);
    }
    // Exact ordered sequence — the same `ix_labels_ordered_eq_sql` SSOT
    // `grouped()` filters the corpus with, so the drill-down's rows are exactly
    // the rows the card counted.
    if let Some(labels) = ix_labels_filter {
        args.push(SqlArg::StrArray(labels.to_vec()));
        clauses.push(ix_labels_ordered_eq_sql("t.ix_labels", &format!("${}", args.len())));
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
    // `tokens_info` as `i` (NOT `grouped_scoped`'s `ti`; see `group_value_sql`).
    clauses.extend(fingerprint_scope_clauses(fp, "i"));
    if let Some(AxisPredicate::Sequence { labels }) = fp.criteria.get(AxisId::IxLabels) {
        args.push(SqlArg::StrArray(labels.clone()));
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
    use hunter_engine::fingerprint::Criteria;
    use uuid::Uuid;

    const SOL: u128 = 1_000_000_000;

    fn filter(from: DateTime<Utc>, to: DateTime<Utc>) -> StatsFilter<'static> {
        StatsFilter { tz: "UTC", maturity_secs: 0.0, from, to, mayhem: None, cashback: None }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-16T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn fp_with(criteria: Criteria) -> Fingerprint {
        Fingerprint { criteria, ..Fingerprint::empty(Uuid::nil(), now()) }
    }

    // ── The mirror ──────────────────────────────────────────────────────────

    /// Every axis the matcher can read, the mirror must be able to read too. An axis
    /// with no expression here is silently DROPPED from a scope clause, which widens
    /// the corpus — the dashboard then shows a comforting number for a rule that
    /// arms on far fewer tokens.
    #[test]
    fn every_numeric_axis_has_a_sql_expression() {
        for axis in AxisId::ALL {
            let sql = axis_num_sql(axis, "ti");
            match axis.def().kind {
                hunter_engine::fingerprint::AxisKind::Numeric => {
                    let sql = sql.unwrap_or_else(|| panic!("{axis:?} has no SQL expression"));
                    assert!(
                        sql.contains("numeric"),
                        "{axis:?} must compare as numeric, never float8 (a ceiling \
                         exceeds 2^53 and float8 stops being injective there): {sql}"
                    );
                }
                hunter_engine::fingerprint::AxisKind::Sequence => {
                    assert!(sql.is_none(), "{axis:?} is a sequence, not a number");
                }
            }
        }
    }

    /// Each predicate shape lowers to the comparison that names the same integers.
    #[test]
    fn a_scope_clause_lowers_each_predicate_shape() {
        let clause = |pred| {
            let fp = fp_with(Criteria::new().with(AxisId::MaxCostLamports, pred));
            fingerprint_scope_clauses(&fp, "ti").join(" AND ")
        };
        assert!(clause(AxisPredicate::exact(SOL)).contains("= 1000000000"));
        assert!(clause(AxisPredicate::range(Some(SOL), Some(2 * SOL)))
            .contains("BETWEEN 1000000000 AND 2000000000"));
        assert!(clause(AxisPredicate::range(Some(SOL), None)).contains(">= 1000000000"));
        assert!(clause(AxisPredicate::range(None, Some(SOL))).contains("<= 1000000000"));
    }

    /// The `u64::MAX` ceiling must survive into the SQL as itself. Under the retired
    /// BIGINT axes it could not even be named, and the two readers of one row
    /// disagreed by 1.84e19.
    #[test]
    fn a_ceiling_is_nameable_in_the_mirror() {
        let fp = fp_with(
            Criteria::new()
                .with(AxisId::MaxCostLamports, AxisPredicate::exact(u128::from(u64::MAX))),
        );
        let sql = fingerprint_scope_clauses(&fp, "ti").join(" AND ");
        assert!(sql.contains("18446744073709551615"), "{sql}");
    }

    /// A criterion-less row matches nothing and a wildcard constrains nothing —
    /// the mirror has to reach both verdicts independently of the matcher.
    #[test]
    fn the_two_degenerate_rows_mirror_the_matchers_own_short_circuits() {
        let empty = fp_with(Criteria::new());
        assert_eq!(fingerprint_scope_clauses(&empty, "ti"), vec!["FALSE".to_string()]);

        let mut any = fp_with(Criteria::new());
        any.wildcard = true;
        assert!(fingerprint_scope_clauses(&any, "ti").is_empty());
    }

    /// `ix_labels` is the one bound predicate (arbitrary on-chain text), so it must
    /// never be interpolated into a scope clause — the caller binds it.
    #[test]
    fn labels_are_never_interpolated_into_a_scope_clause() {
        let fp = fp_with(Criteria::new().with(
            AxisId::IxLabels,
            AxisPredicate::Sequence { labels: vec!["Pump.Fun: Create".into()] },
        ));
        let sql = fingerprint_scope_clauses(&fp, "ti").join(" AND ");
        assert!(!sql.contains("Pump.Fun"), "labels must be bound, not inlined: {sql}");
    }

    /// The engine tallies prior launches over a bounded trailing window; the mirror
    /// must count the SAME window, or a scoped dashboard and the live gate disagree
    /// about what "prior" means.
    #[test]
    fn prior_launches_mirrors_the_engines_trailing_window() {
        let sql = axis_num_sql(AxisId::PriorLaunches, "ti").unwrap();
        assert!(sql.contains(&format!("INTERVAL '{PRIOR_LAUNCH_WINDOW_DAYS} days'")), "{sql}");
        assert!(sql.contains("p.created_at < t.created_at"), "strictly prior: {sql}");
    }

    /// `ix_count` is derived from the labels on both sides, never stored beside them.
    #[test]
    fn ix_count_is_derived_from_the_labels_in_sql_too() {
        let sql = axis_num_sql(AxisId::IxCount, "ti").unwrap();
        assert!(sql.contains("jsonb_array_length"), "{sql}");
        assert!(sql.contains("COALESCE"), "an unknown label list is 0 instructions: {sql}");
    }

    // ── Group keys ──────────────────────────────────────────────────────────

    /// A distinct partition pins the value as a degenerate window, so the key is
    /// directly promotable to a fingerprint axis.
    #[test]
    fn a_distinct_key_is_a_degenerate_window() {
        let sql = group_value_sql(GroupField::CuLimit, &PartitionSpec::Distinct, "ti");
        assert!(sql.contains("'kind', 'window'"), "{sql}");
        assert!(sql.contains("'min'") && sql.contains("'max'"), "{sql}");
        assert!(sql.contains("'kind', 'missing'"), "absent must not collide with 0: {sql}");
    }

    /// The edge ladder must tile the domain the same way `PartitionSpec::window_for`
    /// does: open below the first edge, open above the last, `max = next - 1`.
    #[test]
    fn a_range_ladder_tiles_the_domain_like_the_engine() {
        let spec = PartitionSpec::Ranges { edges: vec![10, 20] };
        let sql = group_value_sql(GroupField::IxCount, &spec, "ti");
        // First window: no min, max = 9.
        assert!(sql.contains("'min', NULL") && sql.contains("'max', '9'"), "{sql}");
        // Middle window: min = 10, max = 19.
        assert!(sql.contains("'min', '10'") && sql.contains("'max', '19'"), "{sql}");
        // Last window: min = 20, open top.
        assert!(sql.contains("'kind', 'window', 'min', '20'"), "{sql}");
        // An open bound is ABSENT, not null — one spelling of "unbounded", matching
        // the `skip_serializing_if` on the Rust side.
        assert!(sql.contains("jsonb_strip_nulls"), "{sql}");
    }

    /// A fingerprint renders as a group key by copying its predicates — no
    /// translation step, so there is nothing to get wrong.
    #[test]
    fn a_fingerprint_group_key_copies_its_predicates() {
        let fp = fp_with(
            Criteria::new()
                .with(AxisId::MaxCostLamports, AxisPredicate::range(Some(SOL), Some(2 * SOL - 1)))
                .with(AxisId::IxLabels, AxisPredicate::Sequence {
                    labels: vec!["Pump.Fun: Create".into(), "Pump.Fun: Buy".into()],
                }),
        );
        let key = group_key_from_fingerprint(&fp);
        assert_eq!(
            key["max_cost_lamports"],
            json!({ "kind": "window", "min": "1000000000", "max": "1999999999" })
        );
        assert_eq!(
            key["ix_labels"],
            json!({ "kind": "labels", "labels": ["Pump.Fun: Create", "Pump.Fun: Buy"] })
        );
        // And it round-trips back into the same predicates.
        let back = crate::grouping::GroupKey::from_json(&key);
        for (field, value) in &back.0 {
            let axis = field.axis().unwrap();
            assert_eq!(value.to_predicate().as_ref(), fp.criteria.get(axis), "{field:?}");
        }
    }

    #[test]
    fn fold_ordered_labels_fills_missing_ix_labels_only() {
        let mut key = json!({ "cu_limit": { "kind": "window", "min": "1", "max": "1" } });
        fold_ordered_labels_into_group_key(&mut key, Some(vec!["A".into(), "B".into()]));
        assert_eq!(key["ix_labels"], json!({ "kind": "labels", "labels": ["A", "B"] }));

        // Never overwrites a real grouped value.
        let mut grouped = json!({ "ix_labels": { "kind": "labels", "labels": ["X"] } });
        fold_ordered_labels_into_group_key(&mut grouped, Some(vec!["A".into()]));
        assert_eq!(grouped["ix_labels"], json!({ "kind": "labels", "labels": ["X"] }));

        // Empty ≡ absent.
        let mut empty = json!({});
        fold_ordered_labels_into_group_key(&mut empty, Some(vec![]));
        assert_eq!(empty, json!({}));
    }

    // ── Filters ─────────────────────────────────────────────────────────────

    /// The filter box, the group key and the live match share one parser, so a chip's
    /// own text selects that chip's tokens rather than relying on two spellings
    /// agreeing.
    #[test]
    fn a_numeric_filter_lowers_through_the_shared_parser() {
        let (pred, bind) =
            field_filter_pred(GroupField::InitBuyLamports, &["1.515".into()], "ti", "$7");
        assert!(bind.is_none(), "a numeric filter is self-contained");
        assert!(pred.contains("= 1515000000"), "{pred}");

        let (range, _) =
            field_filter_pred(GroupField::InitBuyLamports, &["1.5-1.6".into()], "ti", "$7");
        assert!(
            range.contains("BETWEEN 1500000000 AND 1599999999"),
            "a half-open chip is the inclusive range one lamport short: {range}"
        );

        // Several values OR together.
        let (multi, _) = field_filter_pred(
            GroupField::InitBuyLamports,
            &["1.515".into(), "2-2.1".into()],
            "ti",
            "$7",
        );
        assert!(multi.contains(" OR "), "{multi}");
    }

    /// A count axis reads its integer, not SOL — the unit comes off the registry.
    #[test]
    fn a_count_filter_reads_integers_not_sol() {
        let (pred, _) = field_filter_pred(GroupField::IxCount, &["5".into()], "ti", "$7");
        assert!(pred.contains("= 5"), "{pred}");
    }

    /// An unparseable filter becomes `FALSE`, never "no filter" — dropping it widens
    /// the query, which is the direction that looks like success.
    #[test]
    fn an_unparseable_filter_fails_closed() {
        let (pred, bind) = field_filter_pred(GroupField::InitBuyLamports, &["junk".into()], "ti", "$7");
        assert_eq!(pred, "FALSE");
        assert!(bind.is_none());
    }

    #[test]
    fn a_discrete_filter_still_binds_the_group_key_text() {
        let (pred, bind) =
            field_filter_pred(GroupField::TokenProgramId, &["Tokenkeg".into()], "ti", "$7");
        assert!(pred.contains("= ANY($7)"), "{pred}");
        assert_eq!(bind.as_deref(), Some(&["Tokenkeg".to_string()][..]));
    }

    /// The SOL↔lamports conversion the filter parser uses must be the same one the
    /// repo boundary uses, or a typed amount stops recovering its stored integer.
    #[test]
    fn engine_sol_to_lamports_matches_the_repo_boundary_conversion() {
        for sol in [0.0, 0.108, 1.0, 1.515, 15.15, 1234.5] {
            assert_eq!(
                u128::from(crate::grouping::sol_to_lamports(sol)),
                crate::config::constants::sol_to_lamports(sol) as u128,
                "{sol} SOL converts differently in the engine than at the repo boundary"
            );
        }
    }

    // ── Drill-down builders ─────────────────────────────────────────────────

    #[test]
    fn drilldown_binds_stay_in_lockstep_with_placeholders() {
        let (from, to) = (now(), now());
        let plan = GroupPlan::distinct(&[GroupField::CuLimit]);
        let key = crate::grouping::GroupKey(vec![(
            GroupField::CuLimit,
            GroupValue::Window { min: Some(200_000), max: Some(200_000) },
        )]);
        let (sql, args) = build_grouped_tokens_where(
            &plan,
            &key,
            &[(GroupField::TokenProgramId, vec!["Tokenkeg".into()])],
            Some(&["A".to_string()]),
            None,
            None,
            "abc",
            filter(from, to),
        );
        // Every `$n` referenced must exist in `args`.
        let mut n = 0;
        for tok in sql.split('$').skip(1) {
            let digits: String = tok.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(i) = digits.parse::<usize>() {
                n = n.max(i);
            }
        }
        assert_eq!(n, args.len(), "placeholder {n} vs {} binds:\n{sql}", args.len());
        assert!(matches!(args.last(), Some(SqlArg::Str(_))), "search is last");
    }

    /// The group key is compared as `jsonb`, not as a rendered string — that
    /// equality IS the predicate, so the drill-down cannot select a different set
    /// than the card counted.
    #[test]
    fn a_group_key_lowers_to_a_jsonb_equality() {
        let (from, to) = (now(), now());
        let plan = GroupPlan::distinct(&[GroupField::CuLimit]);
        let key = crate::grouping::GroupKey(vec![(
            GroupField::CuLimit,
            GroupValue::Window { min: Some(200_000), max: Some(200_000) },
        )]);
        let (sql, args) =
            build_grouped_tokens_where(&plan, &key, &[], None, None, None, "", filter(from, to));
        assert!(sql.contains("::jsonb"), "{sql}");
        let SqlArg::Str(bound) = &args[args.len() - 1] else { panic!("expected the key bind") };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(bound).unwrap(),
            json!({ "kind": "window", "min": "200000", "max": "200000" })
        );
    }

    /// The drill-down runs against `token_repo`'s query, which joins `tokens_info`
    /// as `i` — passing `ti` is a "missing FROM-clause entry" only at query time.
    #[test]
    fn first_slot_axes_use_the_callers_tokens_info_alias() {
        for axis in [AxisId::FirstSlotBuyLamports, AxisId::FirstSlotSellLamports] {
            assert!(axis_num_sql(axis, "i").unwrap().starts_with("i."));
            assert!(axis_num_sql(axis, "ti").unwrap().starts_with("ti."));
        }
    }

    #[test]
    fn a_group_key_entry_the_plan_does_not_name_is_skipped_not_faked() {
        let (from, to) = (now(), now());
        let plan = GroupPlan::distinct(&[GroupField::CuLimit, GroupField::CuPrice]);
        // Key carries only one of the two grouped fields.
        let key = crate::grouping::GroupKey(vec![(
            GroupField::CuLimit,
            GroupValue::Window { min: Some(1), max: Some(1) },
        )]);
        let (sql, _) =
            build_grouped_tokens_where(&plan, &key, &[], None, None, None, "", filter(from, to));
        assert_eq!(sql.matches("::jsonb").count(), 1, "{sql}");
    }

    #[test]
    fn all_group_with_no_extra_filters_is_window_only() {
        let (from, to) = (now(), now());
        let (sql, args) = build_grouped_tokens_where(
            &GroupPlan::default(),
            &crate::grouping::GroupKey(Vec::new()),
            &[],
            None,
            None,
            None,
            "",
            filter(from, to),
        );
        assert_eq!(args.len(), 2, "just the two window bounds");
        assert!(sql.contains("t.created_at >= $1") && sql.contains("t.created_at < $2"));
    }

    #[test]
    fn dow_hour_binds_tz_once_and_reuses_the_placeholder() {
        let (from, to) = (now(), now());
        let (sql, _) = build_grouped_tokens_where(
            &GroupPlan::default(),
            &crate::grouping::GroupKey(Vec::new()),
            &[],
            None,
            Some(3),
            Some(14),
            "",
            filter(from, to),
        );
        assert_eq!(sql.matches("AT TIME ZONE $3").count(), 2, "{sql}");
    }

    #[test]
    fn search_matches_mint_or_symbol_lowercased() {
        let (from, to) = (now(), now());
        let (sql, args) = build_grouped_tokens_where(
            &GroupPlan::default(),
            &crate::grouping::GroupKey(Vec::new()),
            &[],
            None,
            None,
            None,
            "AbC%",
            filter(from, to),
        );
        assert!(sql.contains("LOWER(t.mint_address)") && sql.contains("LOWER(t.symbol)"));
        assert!(matches!(args.last(), Some(SqlArg::Str(v)) if v == "abc\\%"));
    }

    /// An unknown sort column must fall back to the implicit ordering, never emit a
    /// column name it was handed — the sort registry is the whitelist.
    #[test]
    fn order_defaults_to_newest_first_and_ignores_unknown_columns() {
        let default = "t.created_at DESC, t.mint_address DESC";
        assert_eq!(build_grouped_tokens_order(&[]), default);
        assert_eq!(build_grouped_tokens_order(&[("not_a_column".into(), true)]), default);
    }

    #[test]
    fn rank_by_order_sql_whitelists_and_defaults_to_count() {
        assert_eq!(rank_by_order_sql("nope"), rank_by_order_sql("count"));
        assert_ne!(rank_by_order_sql("trades"), rank_by_order_sql("count"));
    }

    #[test]
    fn trades_avg_divides_by_nullif_zero() {
        assert!(trade_metrics_sql().contains("NULLIF"));
    }
}
