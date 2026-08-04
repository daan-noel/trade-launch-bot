use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;

use hunter_engine::fingerprint::configured_labels;

use crate::config::constants::lamports_to_sol;
use crate::grouping::{bucket_sol_label, decimals_for, exact_sol_label, GroupField, SolFilter, SolPrecision};
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
    /// Set when `ix_labels` is **not** in `group_by` but an `ix_labels_filter`
    /// is active: the **mode** (most common) on-chain-ordered label sequence
    /// across the group's rows (see `grouped()`'s `MODE() WITHIN GROUP`). Folded
    /// into `group_key["ix_labels"]` before returning so a filtered-but-ungrouped
    /// card still seeds a fingerprint with a real ordered axis. The set-equality
    /// filter is order-blind, so rows can disagree on order — mode picks the
    /// majority rather than requiring unanimity (which left most cards empty).
    /// `None` ⇒ fall back to the filter's own label list in `grouped()`.
    pub ix_labels_ordered: Option<Vec<String>>,
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
/// `FM` suppresses the leading blanks, so a wide mask costs nothing in output.
const SQL_MASK_INT_DIGITS: usize = 18;

/// The `to_char` numeric mask for a group-key label at `decimals` fractional
/// places. Shared by [`sol_bucket_sql`] and [`sol_exact_sql`] so the two can never
/// drift on integer width — the drift that produced the `########` groups above.
fn sql_num_mask(decimals: usize) -> String {
    // `FM` strips padding; a trailing `0` (rather than `9`) in each position forces
    // the digit to render, mirroring Rust's `{:.decimals$}`.
    let int_part = format!("{}0", "9".repeat(SQL_MASK_INT_DIGITS - 1));
    if decimals == 0 {
        format!("FM{int_part}")
    } else {
        format!("FM{int_part}.{}", "0".repeat(decimals))
    }
}

fn sol_bucket_sql(sol_expr: &str, width: f64) -> String {
    let decimals = crate::grouping::decimals_for(width);
    let mask = sql_num_mask(decimals);
    // `{width}` prints f64 as its shortest round-tripping decimal, which Postgres
    // parses back to the identical float8 — so `/ width` matches Rust's `/ width`.
    let lo = format!("floor(({sol_expr}) / {width} + 1e-9) * {width}");
    format!(
        "CASE WHEN ({sol_expr}) IS NULL THEN '∅' \
         ELSE to_char({lo}, '{mask}') || '–' || to_char(({lo}) + {width}, '{mask}') END"
    )
}

/// SQL group-key expression for a continuous SOL amount in [`SolPrecision::Exact`]
/// mode — the mirror of `grouping::exact_sol_label_u64`, byte-for-byte. Takes the
/// **raw lamports** expression, not a pre-divided SOL one.
///
/// Nine decimals is lossless for lamports; the double `rtrim` strips trailing
/// zeros then the bare `.`, which is exactly Rust's trailing-zero trim. The `.`
/// pass is load-bearing: without it a whole-SOL amount would keep eating digits
/// (`10.000000000` → `1`). Verified equal on `0`, `1` lamport, `1.515`, `10`,
/// `100`, `15.15`, `123.456789012`.
///
/// **Multiply by `0.000000001`; never divide by `1e9`.** The exact form has to be
/// exact for the whole `u64` domain (see `grouping::MAX_BUCKETABLE_LAMPORTS`), and
/// the two obvious spellings both lose digits there: `::float8 / 1e9` (what this
/// used to be) collapses to 15 significant digits, printing `u64::MAX` as
/// `18446744073.7096`, and even `::numeric / 1e9` loses them because Postgres picks
/// the quotient's scale from `select_div_scale` — 16 significant digits, i.e. only
/// 8 decimals on an 11-digit result. Numeric *multiplication* takes the scale of
/// the operands (`0 + 9 = 9`), so this is exact by construction.
///
/// Do **not** substitute `'FM…999999999'` here: `FM` leaves a trailing `.` on
/// whole amounts (`1.`), which Rust never produces, and a group key that differs
/// by one byte is a different group.
fn sol_exact_sql(lamports_expr: &str) -> String {
    // 9 decimals = one lamport, the finest the source can express.
    let mask = sql_num_mask(9);
    format!(
        "CASE WHEN ({lamports_expr}) IS NULL THEN '∅' \
         ELSE rtrim(rtrim(to_char(({lamports_expr})::numeric * 0.000000001, '{mask}'), '0'), '.') \
         END"
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
fn group_field_sql(f: GroupField, precision: impl Into<SolPrecision>, ti_alias: &str) -> String {
    let precision = precision.into();
    // One seam for the precision policy, so no field can render in a mode the
    // others don't (mirrors `grouping::render_field`'s `key_lamports`/`key_sol`).
    // Takes the **raw lamports** expression: the bucket form divides in `float8`
    // because the engine bins in `f64` and the two must agree bit-for-bit, while
    // the exact form stays in `numeric` because it must be lossless (see
    // [`sol_exact_sql`]).
    let sol_key = |lamports_expr: &str| match precision {
        SolPrecision::Bucket(w) => sol_bucket_sql(&format!("({lamports_expr})::float8 / 1e9"), w),
        SolPrecision::Exact => sol_exact_sql(lamports_expr),
    };
    // The two `u64`-domain axes read off the creation-instruction JSONB. A value
    // past `i64` is a "no limit" ceiling, not an amount, so it short-circuits to
    // its exact label instead of being binned — the mirror of `render_field`'s
    // `key_lamports_u64`. Splitting at the identical threshold is what keeps the
    // dashboard's group key byte-identical to the sweep's.
    let sol_key_u64 = |lamports_expr: &str| {
        format!(
            "CASE WHEN ({lamports_expr}) IS NULL THEN '∅' \
             WHEN ({lamports_expr})::numeric > {max} THEN {exact} ELSE {inner} END",
            max = crate::grouping::MAX_BUCKETABLE_LAMPORTS,
            exact = sol_exact_sql(lamports_expr),
            inner = sol_key(lamports_expr),
        )
    };
    match f {
        GroupField::TokenProgramId => "COALESCE(t.token_program_id, '∅')".to_string(),
        GroupField::CuLimit => "COALESCE(t.cu_limit::text, '∅')".to_string(),
        GroupField::CuPrice => "COALESCE(t.cu_price::text, '∅')".to_string(),
        GroupField::IsCashbackEnabled => "t.is_cashback_enabled::text".to_string(),
        // Continuous SOL amounts → binned SOL ranges. Lamports sources are ÷1e9 to
        // human SOL first so the label reads in SOL (matches the "SOL cost"/"SOL in"
        // display name + the sweep's `bucket_lamports_as_sol`).
        GroupField::MaxCostLamports => {
            sol_key_u64("t.initial_buy_instruction->>'max_cost_lamports'")
        }
        GroupField::SpendableLamportsIn => {
            sol_key_u64("t.initial_buy_instruction->>'spendable_lamports_in'")
        }
        GroupField::InitialBuySol => sol_key("t.initial_buy_lamports"),
        // First-slot buy/sell are trade-derived, sourced from the caller's
        // `tokens_info` LEFT JOIN — the only non-`tokens` group fields.
        GroupField::FirstSlotBuySol => sol_key(&format!("{ti_alias}.first_slot_buy_lamports")),
        GroupField::FirstSlotSellSol => sol_key(&format!("{ti_alias}.first_slot_sell_lamports")),
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

/// The **raw lamports** SQL expression behind a bucketed SOL group field, or
/// `None` for a discrete field. This is the value [`group_field_sql`] bins into a
/// `"lo–hi"` label — exposed unbinned so a value filter can pin an exact amount.
///
/// All five bucketed fields are lamports-backed: two read off the
/// `initial_buy_instruction` JSONB (cast to `numeric`, not `bigint`, so a value
/// persisted as `1515000000.0` can't raise a cast error mid-query and a value past
/// `i64` — pump.fun's `u64::MAX` ceiling — neither errors nor rounds; **not**
/// `float8`, which stops being injective at 2^53), three are `BIGINT` columns.
/// `ti_alias` is the `tokens_info` join alias, same contract as [`group_field_sql`].
fn sol_field_lamports_sql(f: GroupField, ti_alias: &str) -> Option<String> {
    Some(match f {
        GroupField::MaxCostLamports => {
            "(t.initial_buy_instruction->>'max_cost_lamports')::numeric".to_string()
        }
        GroupField::SpendableLamportsIn => {
            "(t.initial_buy_instruction->>'spendable_lamports_in')::numeric".to_string()
        }
        GroupField::InitialBuySol => "t.initial_buy_lamports".to_string(),
        GroupField::FirstSlotBuySol => format!("{ti_alias}.first_slot_buy_lamports"),
        GroupField::FirstSlotSellSol => format!("{ti_alias}.first_slot_sell_lamports"),
        _ => return None,
    })
}

/// One per-field value filter, lowered to SQL. Returns the predicate plus the
/// `text[]` bind it needs (`None` ⇒ fully self-contained, bind nothing).
///
/// **Two shapes, one decider — `GroupField::is_bucketed`.**
///
/// * **Discrete** fields (`cu_limit`, `cu_price`, `is_cashback_enabled`,
///   `token_program_id`) compare the rendered group key against a bound `text[]`.
///   The typed value IS the group-key value, so the filter reads exactly as the
///   card does (`200000` pins `cu_limit=200000`).
/// * **Bucketed** SOL fields cannot do that: their group key is a *range label*
///   (`"1.5–1.6"`), and no typed amount ever equals one — the frontend's
///   `parseNumbers` doesn't even admit the range syntax, so this predicate was
///   unsatisfiable for all five fields. They instead pin the **exact amount** on
///   the raw lamports column ([`sol_field_lamports_sql`]), matching what the
///   grouped sweep's `matches_field_filter` does over the same wire field.
///
/// **Unit + syntax are `hunter_engine::grouping::SolFilter`'s**, the one parser
/// every surface shares: human SOL, either an exact amount (`1.515`) or a
/// half-open bucket range (`1.5–1.6`, the text a group chip displays). Grouping is
/// untouched either way — a filtered run still renders `group_key` as the bucket
/// label, so fingerprint identity / create-from-card are unaffected.
///
/// Bounds are parsed to `i64` lamports and interpolated as integer literals —
/// injection-safe by construction (a parsed number, never user text), the same
/// argument [`fingerprint_scope_clauses`] makes. A value that doesn't parse is
/// dropped; if that leaves nothing, the filter becomes `FALSE` rather than
/// silently passing every row (the handler rejects unparseable values up front,
/// so this is only a defensive floor).
///
/// `bind_placeholder` (e.g. `"$7"`) is consumed **only** when the returned bind is
/// `Some` — the bucketed arm is self-contained, so callers must not advance their
/// parameter counter for it.
fn field_filter_pred(
    field: GroupField,
    values: &[String],
    precision: impl Into<SolPrecision>,
    ti_alias: &str,
    bind_placeholder: &str,
) -> (String, Option<Vec<String>>) {
    let precision = precision.into();
    let Some(lamports_expr) = sol_field_lamports_sql(field, ti_alias) else {
        // Discrete: compare the group-key text against a bound array.
        return (
            format!("{} = ANY({bind_placeholder})", group_field_sql(field, precision, ti_alias)),
            Some(values.to_vec()),
        );
    };
    // Exact amounts collapse into one `IN (…)`; each range contributes its own
    // half-open pair. Both kinds OR together, so `1.515, 2.0–2.1` reads naturally.
    let mut exact: Vec<String> = Vec::new();
    let mut terms: Vec<String> = Vec::new();
    for parsed in values.iter().filter_map(|v| SolFilter::parse(v)) {
        match parsed {
            SolFilter::Exact(l) => exact.push(l.to_string()),
            SolFilter::Range(lo, hi) => {
                terms.push(format!("({lamports_expr} >= {lo} AND {lamports_expr} < {hi})"))
            }
        }
    }
    if !exact.is_empty() {
        terms.push(format!("{lamports_expr} IN ({})", exact.join(", ")));
    }
    match terms.len() {
        0 => ("FALSE".to_string(), None),
        1 => (terms.pop().unwrap(), None),
        _ => (format!("({})", terms.join(" OR ")), None),
    }
}

/// SQL expression for `base`'s per-row `ordered_labels` column (see `grouped()`).
/// Only worth computing when an `ix_labels_filter` is active AND `ix_labels`
/// isn't itself a group field — otherwise `group_key` either has no ix_labels
/// axis to safely fill in, or already carries the real (grouped) one, so the
/// column is a constant `NULL` and costs nothing extra per row.
fn ordered_labels_group_expr(ix_labels_filter: Option<&[String]>, fields: &[GroupField]) -> String {
    let include = ix_labels_filter.is_some() && !fields.contains(&GroupField::IxLabels);
    if include {
        format!(
            "ARRAY(SELECT e FROM {} WITH ORDINALITY AS x(e, ord) ORDER BY ord)",
            ix_labels_elements_sql("t.ix_labels")
        )
    } else {
        "NULL::text[]".to_string()
    }
}

/// Fold an ordered label sequence into a `group_key` object using the SAME
/// `" | "`-joined shape `group_field_sql(IxLabels, ..)` renders when `ix_labels`
/// is an actual group field — so a filtered-but-ungrouped card reads identically
/// to a grouped one everywhere downstream (fingerprint identity, the "already a
/// fingerprint" badge, the card's own key display). A no-op when `ordered` is
/// `None`/empty, or `group_key` already has an `ix_labels` entry (the real
/// grouped case, or a prior MODE fold) — never overwrites.
fn fold_ordered_labels_into_group_key(group_key: &mut JsonValue, ordered: Option<Vec<String>>) {
    let Some(labels) = ordered else { return };
    if labels.is_empty() {
        return;
    }
    if let JsonValue::Object(map) = group_key {
        if !map.contains_key("ix_labels") {
            map.insert("ix_labels".to_string(), json!(labels.join(" | ")));
        }
    }
}

/// Render a saved fingerprint as a `group_key` JSON object (same `" | "`-joined
/// `ix_labels` + bucketed SOL labels the manual `grouped()` path emits). Used by
/// the scoped dashboard so the single `g = 0` card displays the fingerprint's
/// axes as-is — including `ix_labels` — instead of an empty `{}` "ALL" key.
pub fn group_key_from_fingerprint(fp: &Fingerprint) -> JsonValue {
    // Same precision policy as `render_field` / `group_field_sql` — a scoped card
    // must read exactly as the manual path would render the same token.
    let precision = fp.precision();
    let decimals = precision.width().map(decimals_for).unwrap_or(0);
    let sol_key = |l: i64| match precision {
        SolPrecision::Bucket(w) => bucket_sol_label(lamports_to_sol(l), w, decimals),
        SolPrecision::Exact => exact_sol_label(l),
    };
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
            json!(sol_key(l)),
        );
    }
    if let Some(l) = fp.max_cost_lamports {
        map.insert(
            "max_cost_lamports".into(),
            json!(sol_key(l)),
        );
    }
    if let Some(l) = fp.spendable_lamports_in {
        map.insert(
            "spendable_lamports_in".into(),
            json!(sol_key(l)),
        );
    }
    if let Some(l) = fp.first_slot_buy_lamports {
        map.insert(
            "first_slot_buy_sol".into(),
            json!(sol_key(l)),
        );
    }
    if let Some(l) = fp.first_slot_sell_lamports {
        map.insert(
            "first_slot_sell_sol".into(),
            json!(sol_key(l)),
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
fn sol_axis_clause(lamports_expr: &str, fp_lamports: i64, precision: SolPrecision) -> String {
    // A stored axis is a `BIGINT`, so a token value past `i64` can never satisfy
    // one — the mirror of `hunter_engine::fingerprint::sol_axis_u64`'s
    // `bucketable_lamports` guard. Stated explicitly rather than left to the
    // arithmetic so the two sides are provably identical, and so nothing here
    // depends on `float8` behaviour above 2^53. Always true for the three
    // `BIGINT`-sourced axes; load-bearing for the two JSONB `u64` ones.
    let in_range = format!(
        "({lamports_expr})::numeric <= {max}",
        max = crate::grouping::MAX_BUCKETABLE_LAMPORTS,
    );
    match precision {
        SolPrecision::Bucket(width) => {
            let idx = crate::grouping::bucket_index(lamports_to_sol(fp_lamports), width);
            // `float8` here on purpose: the engine bins in `f64`, so the mirror has
            // to reproduce the same rounding, not a more exact one.
            format!(
                "{in_range} AND floor(((({lamports_expr})::float8) / 1e9) / {width} + {eps}) = {idx}",
                eps = crate::grouping::BUCKET_EPS,
            )
        }
        // Exact compares the raw lamports, mirroring the engine's `sol_axis`,
        // which is an `i64 ==`. In `numeric`, not `float8`: comparing as `float8`
        // stops being injective past 2^53 lamports, so two distinct amounts up
        // there would compare equal.
        SolPrecision::Exact => format!("{in_range} AND (({lamports_expr})::numeric) = {fp_lamports}"),
    }
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
    let precision = fp.precision();
    let mut out = Vec::new();
    if let Some(v) = fp.cu_limit {
        out.push(format!("t.cu_limit = {v}"));
    }
    if let Some(v) = fp.cu_price {
        out.push(format!("t.cu_price = {v}"));
    }
    // Each SOL axis pairs its `GroupField` with the fingerprint's value, so the
    // axis→column mapping is read from the ONE `sol_field_lamports_sql` table that
    // the value filters also use — no second hand-written copy of the five column
    // expressions to drift (or to miss an alias fix).
    for (field, fp_lamports) in [
        (GroupField::InitialBuySol, fp.init_buy_lamports),
        (GroupField::MaxCostLamports, fp.max_cost_lamports),
        (GroupField::SpendableLamportsIn, fp.spendable_lamports_in),
        (GroupField::FirstSlotBuySol, fp.first_slot_buy_lamports),
        (GroupField::FirstSlotSellSol, fp.first_slot_sell_lamports),
    ] {
        let (Some(l), Some(expr)) = (fp_lamports, sol_field_lamports_sql(field, ti_alias)) else {
            continue;
        };
        out.push(sol_axis_clause(&expr, l, precision));
    }
    out
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
        fields: &[GroupField],
        bucket_unit: &str,
        top: i64,
        field_filters: &[(GroupField, Vec<String>)],
        ix_labels_filter: Option<&[String]>,
        // Bucket width (SOL) for the continuous SOL group fields — the same knob the
        // grouped sweep uses, so the dashboard's group labels match a sweep at this
        // width ("swept = run"). Discrete fields ignore it.
        precision: SolPrecision,
        // Ranking criterion: "count" (default) | "trades" | "trades_per_token" —
        // whitelisted by the handler's `normalize_rank_by`, never free text.
        rank_by: &str,
        f: StatsFilter<'_>,
    ) -> anyhow::Result<GroupedCreation> {
        // Build the group-key JSON object expression from the selected fields.
        // Empty selection ⇒ a single "ALL" group (`{}`), like the sweep's ALL group.
        let gkey_sql = if fields.is_empty() {
            "'{}'::jsonb".to_string()
        } else {
            let pairs: Vec<String> = fields
                .iter()
                .map(|fld| format!("'{}', {}", fld.as_str(), group_field_sql(*fld, precision, "ti")))
                .collect();
            format!("jsonb_build_object({})", pairs.join(", "))
        };

        // Per-field value filters restrict the corpus *before* partitioning, so
        // only matching groups survive into the top-N. `field_filter_pred` is the
        // SSOT for how one lowers: discrete fields compare the rendered group-key
        // TEXT against a bound `text[]`, the bucketed SOL fields pin an exact
        // amount on their raw lamports column (they have no typeable group-key
        // form). `ix_labels` is a set-equality match (order-independent) against
        // the sorted-distinct label array. Binds start at `$7` (after top=$6);
        // only a predicate that returned a bind advances `idx`, so predicate index
        // and bind order stay in lockstep.
        let mut preds = String::new();
        let mut filter_binds: Vec<Vec<String>> = Vec::new();
        let mut idx = 7;
        for (fld, vals) in field_filters {
            let (pred, bind) = field_filter_pred(*fld, vals, precision, "ti", &format!("${idx}"));
            preds.push_str(&format!("\n  AND {pred}"));
            if let Some(b) = bind {
                filter_binds.push(b);
                idx += 1;
            }
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

        // When `ix_labels` isn't itself a group field but a filter narrowed the
        // corpus by *sorted, de-duplicated* label set (order-blind), a group's
        // rows can still disagree on the real on-chain-ordered sequence. Compute
        // each row's exact ordered sequence so `ranked` can take the MODE (most
        // common) — a fingerprint's `ix_labels` match is exact-ordered (see
        // `hunter_engine::fingerprint::matches`), so we need a concrete sequence,
        // not the filter's set. Unanimity was too strict and left most filtered
        // cards without `ix_labels`. Skipped (constant `NULL`) when there's no
        // filter or ix_labels is already grouped.
        let ordered_labels_expr = ordered_labels_group_expr(ix_labels_filter, fields);

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
                       ti.trade_count AS trade_count,
                       {ordered_labels} AS ordered_labels
                FROM tokens t
                LEFT JOIN tokens_info ti ON ti.mint_address = t.mint_address
                WHERE t.created_at >= $2 AND t.created_at < $3
                  AND ($4::bool IS NULL OR t.is_mayhem_mode = $4)
                  AND ($5::bool IS NULL OR t.is_cashback_enabled = $5){preds}
            ),
            ranked AS (
                SELECT gkey, COUNT(*) AS total,
                       COALESCE(SUM(trade_count), 0)::bigint AS trades,
                       (row_number() OVER (ORDER BY {order}) - 1) AS g,
                       mode() WITHIN GROUP (ORDER BY ordered_labels) AS ix_labels_ordered
                FROM base
                GROUP BY gkey
                ORDER BY {order}
                LIMIT $6
            )
            "#,
            gkey = gkey_sql,
            ordered_labels = ordered_labels_expr,
        );

        // SQL strings bound to named locals so the queries (which borrow them)
        // outlive each statement. Bind the fixed params (renumbered) then the
        // per-field filter arrays; applied identically to all three sub-queries.
        let groups_sql = format!(
            "{cte} SELECT g::bigint AS g, gkey AS group_key, total::bigint AS total, \
             trades::bigint AS trades, (trades::float8 / total::float8) AS trades_avg, \
             ix_labels_ordered \
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

        // Prefer the MODE on-chain sequence; if SQL returned nothing (e.g. all
        // NULL), fall back to the filter's own label list so create-from-card
        // never silently drops the axis the user already pinned on the corpus.
        for g in &mut groups {
            let ordered = g.ix_labels_ordered.take();
            fold_ordered_labels_into_group_key(&mut g.group_key, ordered);
            if let Some(labels) = ix_labels_filter {
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
             (COALESCE(SUM(trade_count), 0)::float8 / COUNT(*)::float8) AS trades_avg, \
             NULL::text[] AS ix_labels_ordered \
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
    fields: &[GroupField],
    group_key: &[(GroupField, String)],
    field_filters: &[(GroupField, Vec<String>)],
    ix_labels_filter: Option<&[String]>,
    precision: impl Into<SolPrecision>,
    dow: Option<i32>,
    hour: Option<i32>,
    search: &str,
    f: StatsFilter<'_>,
) -> (String, Vec<crate::api::handlers::tokens::SqlArg>) {
    let precision = precision.into();
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
        clauses.push(format!("{} = ${}", group_field_sql(*field, precision, "i"), args.len()));
    }

    // Corpus-level filters applied before the groups were ranked — through the
    // SAME `field_filter_pred` SSOT `grouped()` uses, so the drill-down's row set
    // and the card's count can't diverge. A bucketed-SOL predicate is
    // self-contained (no bind), so only the discrete arm pushes an arg.
    for (field, vals) in field_filters {
        let (pred, bind) = field_filter_pred(*field, vals, precision, "i", &format!("${}", args.len() + 1));
        if let Some(b) = bind {
            args.push(SqlArg::StrArray(b));
        }
        clauses.push(pred);
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

    /// The bug this locks: a value filter on a bucketed SOL field used to compare
    /// the typed amount against the **bucket label** the group key renders
    /// (`'15.1–15.2' = '15.15'`), so it matched nothing at any width — and the
    /// frontend's `parseNumbers` wouldn't even admit the range syntax that could
    /// have matched. It must pin the exact amount on the raw lamports column.
    #[test]
    fn bucketed_sol_filter_pins_exact_lamports_not_the_bucket_label() {
        let (pred, bind) = field_filter_pred(
            GroupField::MaxCostLamports,
            &["1.515".to_string()],
            SOL_BUCKET_WIDTH,
            "ti",
            "$7",
        );
        // Exact lamports, self-contained (no bind consumed → callers must not
        // advance their parameter counter for this arm).
        assert!(pred.contains("1515000000"), "expected exact lamports pin, got: {pred}");
        assert!(bind.is_none(), "bucketed arm must not consume a bind slot");
        // Must NOT reach for the label machinery.
        assert!(!pred.contains('–'), "predicate compares a bucket label: {pred}");
        assert!(!pred.contains("to_char"), "predicate compares a bucket label: {pred}");

        // The width is irrelevant to the filter — only to the group key. A run at
        // any width pins the same amount (this is what "exact match" buys you).
        let (wide, _) = field_filter_pred(
            GroupField::MaxCostLamports,
            &["1.515".to_string()],
            5.0,
            "ti",
            "$7",
        );
        assert_eq!(pred, wide, "bucket width must not change an exact value filter");
    }

    /// Every bucketed field pins its own lamports source, at the caller's
    /// `tokens_info` alias — a missed arm silently falls back to the label compare
    /// (and a wrong alias is a run-time "missing FROM-clause entry").
    #[test]
    fn every_bucketed_field_has_a_lamports_source() {
        for field in GroupField::ALL {
            let src = sol_field_lamports_sql(field, "ti");
            assert_eq!(
                src.is_some(),
                field.is_bucketed(),
                "{field:?}: is_bucketed()={} but lamports source is {src:?}",
                field.is_bucketed(),
            );
        }
        // Alias-dependent arms actually use the alias they were handed.
        assert!(sol_field_lamports_sql(GroupField::FirstSlotBuySol, "i")
            .unwrap()
            .starts_with("i."));
        assert!(sol_field_lamports_sql(GroupField::FirstSlotSellSol, "ti")
            .unwrap()
            .starts_with("ti."));
    }

    /// `hunter-engine` is pure by design, so it carries its own SOL→lamports
    /// conversion rather than importing `config::constants`. That's a sanctioned
    /// duplication (crate decoupling), so it needs the guard the monorepo rules
    /// require: the two must agree on every value, or a filter typed against a
    /// displayed amount would resolve to a different integer than the one the
    /// repo boundary stored.
    #[test]
    fn engine_sol_to_lamports_matches_the_repo_boundary_conversion() {
        for sol in [0.0, 1e-9, 0.05, 0.1, 1.515, 2.34, 15.15, 99.999, 1234.56789] {
            assert_eq!(
                crate::grouping::sol_to_lamports(sol),
                crate::config::constants::sol_to_lamports(sol),
                "engine/config disagree on {sol} SOL"
            );
        }
    }

    /// A group chip's text, pasted into the filter box, must select that chip's
    /// tokens — the half-open `[lo, hi)` window, never an exact-value compare.
    #[test]
    fn a_bucket_range_filter_lowers_to_a_half_open_window() {
        let (pred, bind) = field_filter_pred(
            GroupField::MaxCostLamports,
            &["1.5–1.6".to_string()],
            SOL_BUCKET_WIDTH,
            "ti",
            "$7",
        );
        assert!(bind.is_none());
        assert!(pred.contains(">= 1500000000"), "expected lower bound, got: {pred}");
        assert!(pred.contains("< 1600000000"), "expected exclusive upper bound, got: {pred}");

        // Mixed list: exact amounts collapse into one IN(), ranges OR alongside.
        let (mixed, _) = field_filter_pred(
            GroupField::MaxCostLamports,
            &["1.515".to_string(), "2.0–2.1".to_string(), "15.15".to_string()],
            SOL_BUCKET_WIDTH,
            "ti",
            "$7",
        );
        assert!(mixed.contains(" OR "), "expected a disjunction, got: {mixed}");
        assert!(mixed.contains("IN (1515000000, 15150000000)"), "got: {mixed}");
        assert!(mixed.contains(">= 2000000000"), "got: {mixed}");
    }

    /// Exact mode must render the amount, not a range — and must not reach for the
    /// bucket machinery, whose `to_char` mask would produce a different key.
    /// `sol_exact_sql` mirrors `grouping::exact_sol_label`; the two were checked
    /// equal against live Postgres on 0, 1 lamport, 0.05, 1.515, 10, 100, 15.15 and
    /// 123.456789012 — the `rtrim(rtrim(…,'0'),'.')` pair is what makes them agree
    /// (an `FM…999999999` mask leaves `1.` where Rust gives `1`).
    /// `to_char` renders `########` when a value overflows its mask, and that
    /// string is a perfectly good TEXT group key — so an overflow silently becomes
    /// a wrong group rather than an error. The mask must therefore be wide enough
    /// for the widest value the column can hold, including pump.fun's
    /// `max_cost_lamports = u64::MAX` "no slippage limit" sentinel.
    #[test]
    fn masks_are_wide_enough_for_the_u64_max_sentinel() {
        // u64::MAX lamports as SOL ≈ 1.8446744e10 — 11 integer digits.
        let needed = (u64::MAX as f64 / 1e9).log10().floor() as usize + 1;
        assert!(
            SQL_MASK_INT_DIGITS >= needed,
            "mask holds {SQL_MASK_INT_DIGITS} integer digits, sentinel needs {needed}"
        );
        for decimals in [0, 1, 2, 5, 9] {
            let mask = sql_num_mask(decimals);
            let int_digits = mask.trim_start_matches("FM").split('.').next().unwrap().len();
            assert_eq!(int_digits, SQL_MASK_INT_DIGITS, "mask {mask} lost integer width");
            assert_eq!(
                mask.split('.').nth(1).map(str::len).unwrap_or(0),
                decimals,
                "mask {mask} has the wrong fractional width"
            );
        }
        // Both renderers must go through the shared builder — a private mask is
        // how the two drifted apart in the first place.
        assert!(sol_bucket_sql("x", 0.1).contains(&sql_num_mask(1)));
        assert!(sol_exact_sql("x").contains(&sql_num_mask(9)));
    }

    /// The SQL mirror must split the `u64`-domain axes at **the engine's**
    /// threshold, and must not reach for `float8` anywhere the value can exceed
    /// 2^53. Both halves were live bugs: the group key cast
    /// `(…->>'max_cost_lamports')::float8 / 1e9`, which rendered pump.fun's
    /// `u64::MAX` ceiling as `18446744073.7096` (15 significant digits) and folded
    /// distinct ceilings into one group, while the engine read the same row as `-1`.
    #[test]
    fn the_u64_axes_split_at_the_engine_threshold_and_stay_exact() {
        let max = crate::grouping::MAX_BUCKETABLE_LAMPORTS.to_string();
        for field in [GroupField::MaxCostLamports, GroupField::SpendableLamportsIn] {
            for precision in [SolPrecision::Exact, SolPrecision::Bucket(0.1)] {
                let sql = group_field_sql(field, precision, "ti");
                assert!(
                    sql.contains(&max),
                    "{field:?}/{precision:?}: no out-of-i64 branch at the engine \
                     threshold {max}: {sql}"
                );
                // The exact renderer must multiply, never divide: Postgres picks a
                // quotient scale from `select_div_scale` (16 significant digits →
                // only 8 decimals on an 11-digit result), so `/ 1e9` drops the low
                // lamport digits even in `numeric`.
                assert!(
                    sql.contains("::numeric * 0.000000001"),
                    "{field:?}/{precision:?}: exact branch is not lossless: {sql}"
                );
                assert!(
                    !sql.contains("::float8 / 1e9 IS NULL"),
                    "{field:?}/{precision:?}: null-check went through float8: {sql}"
                );
            }
        }
        // The `BIGINT`-backed axes cannot exceed `i64`, so they keep the plain
        // renderer — no branch, no numeric cast, no change to their group keys.
        for field in [GroupField::InitialBuySol, GroupField::FirstSlotBuySol] {
            let sql = group_field_sql(field, SolPrecision::Bucket(0.1), "ti");
            assert!(!sql.contains(&max), "{field:?}: grew a branch it does not need: {sql}");
        }
    }

    /// The fingerprint-scope mirror carries the same guard as the engine matcher
    /// (`sol_axis_u64`'s `bucketable_lamports`), and compares exact amounts in
    /// `numeric` — `float8` equality stops being injective at 2^53, so two distinct
    /// lamport amounts up there would compare equal and widen the armed set.
    #[test]
    fn the_scope_mirror_guards_the_same_range_as_the_matcher() {
        let max = crate::grouping::MAX_BUCKETABLE_LAMPORTS.to_string();
        let expr = sol_field_lamports_sql(GroupField::MaxCostLamports, "ti").unwrap();
        assert!(expr.ends_with("::numeric"), "scope expr must be exact, got: {expr}");
        for precision in [SolPrecision::Exact, SolPrecision::Bucket(0.1)] {
            let clause = sol_axis_clause(&expr, 1_515_000_000, precision);
            assert!(clause.contains(&max), "{precision:?}: no range guard: {clause}");
        }
        // Bucket mode still bins in `float8` — the engine bins in `f64`, and the
        // mirror has to reproduce that rounding rather than a more exact one.
        let bucketed = sol_axis_clause(&expr, 1_515_000_000, SolPrecision::Bucket(0.1));
        assert!(bucketed.contains("::float8"), "bucket parity with the engine lost: {bucketed}");
        let exact = sol_axis_clause(&expr, 1_515_000_000, SolPrecision::Exact);
        assert!(
            exact.contains("::numeric) = 1515000000"),
            "exact compare must be numeric: {exact}"
        );
    }

    #[test]
    fn exact_precision_renders_amounts_not_ranges() {
        for field in GroupField::ALL.into_iter().filter(|f: &GroupField| f.is_bucketed()) {
            let sql = group_field_sql(field, SolPrecision::Exact, "ti");
            assert!(sql.contains("rtrim"), "{field:?}: not the exact renderer: {sql}");
            assert!(!sql.contains("'–'"), "{field:?}: still emits a range label: {sql}");
            assert!(sql.contains("'∅'"), "{field:?}: lost the missing-value sentinel");
            // Bucket mode is unchanged for the same field.
            let bucketed = group_field_sql(field, SolPrecision::Bucket(0.1), "ti");
            assert!(bucketed.contains("'–'"), "{field:?}: bucket mode regressed: {bucketed}");
        }
        // Discrete fields are identical in both modes — precision is a SOL-axis
        // concept only, so a mode switch must not perturb their keys.
        for field in GroupField::ALL.into_iter().filter(|f| !f.is_bucketed()) {
            assert_eq!(
                group_field_sql(field, SolPrecision::Exact, "ti"),
                group_field_sql(field, SolPrecision::Bucket(0.1), "ti"),
                "{field:?}: discrete field changed with precision",
            );
        }
    }

    /// Discrete fields keep comparing the rendered group key against a bound
    /// `text[]` — there the typed value IS the group-key value, so `cu_limit=200000`
    /// reads exactly as the card does. Changing this would break working filters.
    #[test]
    fn discrete_filter_still_binds_the_group_key_text() {
        let (pred, bind) = field_filter_pred(
            GroupField::CuLimit,
            &["200000".to_string()],
            SOL_BUCKET_WIDTH,
            "ti",
            "$7",
        );
        assert!(pred.contains("t.cu_limit"), "expected group-key compare, got: {pred}");
        assert!(pred.ends_with("= ANY($7)"), "expected bound array compare, got: {pred}");
        assert_eq!(bind.as_deref(), Some(["200000".to_string()].as_slice()));
    }

    /// Both surfaces number their parameters themselves, so a self-contained
    /// (bucketed) predicate must not leave a hole in the drill-down's bind list.
    #[test]
    fn drilldown_binds_stay_in_lockstep_with_placeholders() {
        let (sql, args) = build_grouped_tokens_where(
            &[GroupField::CuLimit],
            &[(GroupField::CuLimit, "200000".to_string())],
            // One bucketed (no bind) + one discrete (one bind), in that order —
            // the discrete one must still land on the placeholder it was given.
            &[
                (GroupField::MaxCostLamports, vec!["1.515".to_string()]),
                (GroupField::CuPrice, vec!["1000".to_string()]),
            ],
            None,
            SOL_BUCKET_WIDTH,
            None,
            None,
            "",
            filter(now() - chrono::Duration::days(30), now()),
        );
        // Highest placeholder referenced must equal the number of args pushed.
        let max_ph = (1..=args.len() + 2)
            .filter(|n| sql.contains(&format!("${n}")))
            .max()
            .unwrap_or(0);
        assert_eq!(max_ph, args.len(), "placeholder/bind drift in: {sql}");
        assert!(sql.contains("1515000000"), "bucketed filter not applied: {sql}");
        match args.last() {
            Some(SqlArg::StrArray(v)) => assert_eq!(v, &["1000".to_string()]),
            other => panic!("expected the cu_price array as the last bind, got {other:?}"),
        }
    }

    #[test]
    fn ordered_labels_expr_only_when_filtered_and_ungrouped() {
        let labels = vec!["Pump.Fun: Create".to_string(), "Pump.Fun: Buy".to_string()];

        // Filter active, grouping by something else ⇒ compute the real ordered
        // sequence so `ranked` can check unanimity.
        let sql = ordered_labels_group_expr(Some(&labels), &[GroupField::CuLimit]);
        assert!(sql.contains("WITH ORDINALITY"), "expected ordinality expr, got: {sql}");

        // No filter ⇒ nothing to verify, constant NULL (zero added cost).
        assert_eq!(ordered_labels_group_expr(None, &[GroupField::CuLimit]), "NULL::text[]");

        // Already grouped by ix_labels ⇒ group_key has the real thing already;
        // redundant to also compute it here.
        assert_eq!(
            ordered_labels_group_expr(Some(&labels), &[GroupField::IxLabels]),
            "NULL::text[]"
        );
    }

    #[test]
    fn fold_ordered_labels_fills_missing_ix_labels_only() {
        // Mode / verified sequence ⇒ folded in, on-chain order preserved
        // (not sorted alphabetically — "Buy" before "Create" here).
        let mut gk = serde_json::json!({"cu_limit": "200000"});
        fold_ordered_labels_into_group_key(
            &mut gk,
            Some(vec!["Pump.Fun: Buy".to_string(), "Pump.Fun: Create".to_string()]),
        );
        assert_eq!(gk["ix_labels"], "Pump.Fun: Buy | Pump.Fun: Create");
        assert_eq!(gk["cu_limit"], "200000");

        // Nothing from SQL ⇒ no-op; caller may then fall back to the filter list.
        let mut gk_none = serde_json::json!({"cu_limit": "200000"});
        fold_ordered_labels_into_group_key(&mut gk_none, None);
        assert!(gk_none.get("ix_labels").is_none());

        // Already grouped by ix_labels ⇒ never overwritten, even if (defensively)
        // called with some other ordered value.
        let mut gk_grouped = serde_json::json!({"ix_labels": "A | B"});
        fold_ordered_labels_into_group_key(&mut gk_grouped, Some(vec!["C".to_string()]));
        assert_eq!(gk_grouped["ix_labels"], "A | B");
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
            bucket_size_amount: Some(0.1),
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
    fn blank_fp(width: Option<f64>) -> Fingerprint {
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
    /// The five SOL axes paired with the `GroupField` naming their lamports column,
    /// so this guard reads the column expression from the same
    /// `sol_field_lamports_sql` table production uses — it can then only test the
    /// bucketing/precision, never accidentally re-assert a stale column name.
    const SOL_AXES: &[(fn(&mut Fingerprint, Option<i64>), GroupField)] = &[
        (|fp, v| fp.init_buy_lamports = v, GroupField::InitialBuySol),
        (|fp, v| fp.max_cost_lamports = v, GroupField::MaxCostLamports),
        (|fp, v| fp.spendable_lamports_in = v, GroupField::SpendableLamportsIn),
        (|fp, v| fp.first_slot_buy_lamports = v, GroupField::FirstSlotBuySol),
        (|fp, v| fp.first_slot_sell_lamports = v, GroupField::FirstSlotSellSol),
    ];

    /// One-axis fingerprint at `width` (`None` => exact match), for the `axis`-th
    /// entry of [`SOL_AXES`].
    fn one_axis_fp(axis: usize, lamports: i64, width: Option<f64>) -> Fingerprint {
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
        for (axis, (_, field)) in SOL_AXES.iter().enumerate() {
            let lam_expr = sol_field_lamports_sql(*field, "ti").expect("bucketed axis");
            let sol_expr = format!("((({lam_expr})::float8) / 1e9)");
            for width in [1e-6f64, 0.05, 0.1, 0.25, 1.0, 5.0] {
                for fp_sol in [0.0f64, 0.1, 0.5, 1.0, 2.34, 8.0] {
                    let lamports = (fp_sol * 1e9).round() as i64;
                    let fp = one_axis_fp(axis, lamports, Some(width));
                    let clauses = fingerprint_scope_clauses(&fp, "ti");
                    assert_eq!(clauses.len(), 1, "one configured axis ⇒ one clause");

                    // The emitted literal must be the engine's own bucket index at
                    // the fingerprint's own width — no substituted default. Compare
                    // through the same lamports→SOL conversion the clause builder
                    // uses so this tests the bucketing, not f64 round-tripping.
                    let fp_sol = lamports_to_sol(lamports);
                    let idx = bucket_index(fp_sol, width);
                    // The `<= i64::MAX` prefix mirrors the matcher's
                    // `bucketable_lamports` guard: a token value past `i64` (the
                    // `u64::MAX` "no cap" ceiling) can never satisfy a `BIGINT` axis
                    // on either side. Always true for the three `BIGINT` axes.
                    let in_range = format!(
                        "({lam_expr})::numeric <= {}",
                        crate::grouping::MAX_BUCKETABLE_LAMPORTS
                    );
                    let expected = format!(
                        "{in_range} AND floor({sol_expr} / {width} + {BUCKET_EPS}) = {idx}"
                    );
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
            let fp = one_axis_fp(axis, 0, Some(1.0));
            let clauses = fingerprint_scope_clauses(&fp, "ti");
            assert_eq!(clauses.len(), 1, "axis {axis}: a 0-lamport axis must emit a clause");
            assert!(clauses[0].ends_with("= 0"), "axis {axis}: expected bucket 0: {}", clauses[0]);
        }
        // …while an all-absent fingerprint is fenced off entirely rather than
        // matching every token.
        assert_eq!(
            fingerprint_scope_clauses(&blank_fp(Some(1.0)), "ti"),
            vec!["FALSE".to_string()],
        );

        // `Some([])` is the SAME state as `None` (see `configured_labels`), so it
        // must fence too. It previously satisfied `has_any_criterion`, skipped the
        // FALSE guard, and then emitted NO predicates — leaving the scoped
        // dashboard matching every token in the window while the engine matcher
        // (which does not count empty labels) matched none.
        let mut empty_labels = blank_fp(Some(1.0));
        empty_labels.ix_labels = Some(vec![]);
        assert_eq!(
            fingerprint_scope_clauses(&empty_labels, "ti"),
            vec!["FALSE".to_string()],
            "an empty label list must never widen the scope to every token",
        );
    }

    /// The exact-mode half of the same SSOT guard: with `bucket_size_amount = NULL`
    /// every SOL axis must pin the **raw lamports integer**, matching the engine's
    /// `sol_axis`, which is an `i64 ==`.
    ///
    /// Comparing in SOL would be the subtle wrong answer here: `lamports as f64 / 1e9`
    /// stops being injective past 2^53 lamports, and real data carries pump.fun's
    /// `max_cost_lamports = u64::MAX` "no limit" sentinel — well past that — so a
    /// float compare could call two different amounts equal and arm on the wrong
    /// token. This is the live entry gate, so that has to be impossible, not unlikely.
    /// For the same reason the comparison itself is `numeric`, not `float8`: casting
    /// the raw lamports to `float8` reintroduced exactly that collision one level
    /// down, above 2^53.
    #[test]
    fn exact_fingerprint_scope_pins_raw_lamports_on_every_axis() {
        for (axis, (_, field)) in SOL_AXES.iter().enumerate() {
            let lam_expr = sol_field_lamports_sql(*field, "ti").expect("bucketed axis");
            for lamports in [0_i64, 1, 50_000_000, 1_515_000_000, 15_150_000_000, i64::MAX] {
                let fp = one_axis_fp(axis, lamports, None);
                assert_eq!(fp.precision(), crate::grouping::SolPrecision::Exact);
                let clauses = fingerprint_scope_clauses(&fp, "ti");
                assert_eq!(clauses.len(), 1, "one configured axis ⇒ one clause");
                assert_eq!(
                    clauses[0],
                    format!(
                        "({lam_expr})::numeric <= {max} AND (({lam_expr})::numeric) = {lamports}",
                        max = crate::grouping::MAX_BUCKETABLE_LAMPORTS,
                    ),
                    "axis={axis} lamports={lamports}",
                );
                // No bucketing arithmetic may survive into the exact clause.
                assert!(!clauses[0].contains("floor"), "exact clause still buckets: {}", clauses[0]);
            }
        }
        // A NULL width must not weaken the never-match-everything guard.
        assert_eq!(
            fingerprint_scope_clauses(&blank_fp(None), "ti"),
            vec!["FALSE".to_string()],
        );
    }

    /// A scoped card under an exact fingerprint must read as the amount, not a
    /// range — otherwise the card describes a wider set than it was served.
    #[test]
    fn exact_scoped_group_key_labels_as_amounts() {
        let mut fp = blank_fp(None);
        fp.spendable_lamports_in = Some(0);
        fp.init_buy_lamports = Some(1_515_000_000);
        let gk = group_key_from_fingerprint(&fp);
        assert_eq!(gk["spendable_lamports_in"], "0");
        assert_eq!(gk["initial_buy_sol"], "1.515");
    }

    /// The `g = 0` scoped card labels its axes at the same width the clauses bucket
    /// at, so the card a user reads describes the rows they were actually served.
    #[test]
    fn scoped_group_key_labels_at_the_fingerprint_width() {
        let mut fp = blank_fp(Some(1.0));
        fp.spendable_lamports_in = Some(0);
        fp.init_buy_lamports = Some(2_500_000_000); // 2.5 SOL @ 1.0 ⇒ [2, 3)
        let gk = group_key_from_fingerprint(&fp);
        assert_eq!(gk["spendable_lamports_in"], "0–1", "a 0 axis labels as its own bucket");
        assert_eq!(gk["initial_buy_sol"], "2–3");

        let mut fp = blank_fp(Some(0.1));
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
        // The `group_key` entry stays a bucket LABEL (that's what a card carries);
        // the `field_filters` entry is an exact SOL amount (the filter pins the
        // amount, not the label — see `field_filter_pred`). Both arms must resolve
        // the alias, so this covers the label path and the raw-lamports path.
        let (where_sql, _args) = build_grouped_tokens_where(
            &[GroupField::FirstSlotBuySol],
            &[(GroupField::FirstSlotBuySol, "0.0–0.1".to_string())],
            &[(GroupField::FirstSlotSellSol, vec!["0.05".to_string()])],
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

    // -- trade-counts plan §7 guards -----------------------------------------

    /// Every trade-metric column reuses [`MATURED_PRED`] verbatim — one filter
    /// each for `trades`/`volume_sol`/`trades_per_day`, two for `trades_avg`
    /// (numerator + `NULLIF` denominator) — so a future edit can't drift the
    /// censoring between the outcome columns and the trade metrics
    /// (trade-counts plan §2/§7). `heatmap()`/`trend()` both build their SQL
    /// from this exact fragment (see `trade_metrics_sql`), so this one guard
    /// covers both.
    #[test]
    fn trade_metrics_sql_reuses_the_matured_predicate() {
        let sql = trade_metrics_sql();
        assert_eq!(
            sql.matches(MATURED_PRED).count(),
            5,
            "trades(1) + volume_sol(1) + trades_per_day(1) + trades_avg(2 — numerator & NULLIF) = 5: {sql}"
        );
        assert!(sql.contains("AS trades,"));
        assert!(sql.contains("AS volume_sol,"));
        assert!(sql.contains("AS trades_per_day,"));
        assert!(sql.contains("AS trades_avg"));
    }

    /// `trades_avg` divides by `NULLIF(..., 0)`, never a bare `COUNT(*)` — a
    /// future edit can't reintroduce a division-by-zero on an empty cell.
    #[test]
    fn trades_avg_divides_by_nullif_zero() {
        assert!(
            trade_metrics_sql().contains("NULLIF("),
            "trades_avg must guard its denominator"
        );
    }

    /// `rank_by=trades` / `rank_by=trades_per_token` emit the expected `ORDER BY`
    /// fragment; an absent/unknown tag falls back to the existing `COUNT(*) DESC`
    /// (the "default ranking does not change" rule).
    #[test]
    fn rank_by_order_sql_whitelists_and_defaults_to_count() {
        assert_eq!(rank_by_order_sql("count"), "COUNT(*) DESC, gkey::text");
        assert_eq!(rank_by_order_sql("bogus"), "COUNT(*) DESC, gkey::text");
        assert_eq!(rank_by_order_sql("trades"), "COALESCE(SUM(trade_count), 0) DESC, gkey::text");
        assert_eq!(
            rank_by_order_sql("trades_per_token"),
            "(COALESCE(SUM(trade_count), 0)::float8 / COUNT(*)::float8) DESC, gkey::text",
        );
    }
}
