use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use hunter_engine::fingerprint::configured_labels;

use crate::api::table_query::TableRequest;
use crate::state::core_state::CoreState;
use crate::storage::repositories::creation_stats_repo::{HeatCellRow, StatsFilter, TrendPointRow};
use crate::grouping::GroupField;

use super::tokens::TokenSummary;

/// Default outcome-maturity window (24h): tokens younger than this are excluded
/// from migrate/dead counts so a fresh bucket doesn't read artificially bad.
const DEFAULT_MATURITY_SECS: f64 = 86_400.0;
/// Default look-back when the client sends no explicit `from`.
const DEFAULT_WINDOW_DAYS: i64 = 30;
/// Hard cap on the look-back span (defensive — keeps the index range scan bounded
/// even if a client hand-crafts a huge `from`..`to`).
const MAX_WINDOW_DAYS: i64 = 366;
/// Default number of fingerprint groups returned by the grouped endpoint.
const DEFAULT_TOP_GROUPS: i64 = 8;
/// Hard cap on returned groups — keeps the small-multiple grid legible + the
/// payload bounded.
const MAX_TOP_GROUPS: i64 = 16;

#[derive(Deserialize)]
pub struct CreationStatsQuery {
    /// `heatmap` (7×24 fold, default) | `trend` (absolute calendar buckets).
    pub view: Option<String>,
    /// Trend granularity: `hour` | `day` | `week` (default `day`). Ignored for heatmap.
    pub bucket: Option<String>,
    /// IANA timezone for bucketing; defaults to UTC.
    pub tz: Option<String>,
    /// Window bounds (RFC3339). Default: last 30d → now.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Outcome maturity window in seconds (default 24h).
    pub maturity_secs: Option<f64>,
    /// Segment filter: `all` | `mayhem` | `non_mayhem` | `cashback` | `non_cashback`.
    pub segment: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct HeatCell {
    pub dow: i32,
    pub hour: i32,
    pub count: i64,
    pub matured: i64,
    pub known: i64,
    pub migrated: i64,
    pub dead: i64,
}

#[derive(Serialize)]
pub struct TrendPoint {
    /// Local wall-clock bucket start, e.g. `2026-06-01T00:00:00` (already shifted
    /// into the requested tz; the client renders it on a UTC-formatting axis).
    pub bucket: NaiveDateTime,
    pub count: i64,
    pub matured: i64,
    pub known: i64,
    pub migrated: i64,
    pub dead: i64,
}

#[derive(Serialize)]
pub struct CreationStatsResponse {
    pub view: String,
    pub bucket: String,
    pub tz: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub maturity_secs: f64,
    pub segment: String,
    /// Totals across the window (volume-view count, matured base, outcome-known
    /// base) so the client can render share-of-total + a coverage % without a
    /// second pass.
    pub total: i64,
    pub matured: i64,
    pub known: i64,
    /// Populated when `view=heatmap`, else empty.
    pub cells: Vec<HeatCell>,
    /// Populated when `view=trend`, else empty.
    pub points: Vec<TrendPoint>,
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested without a DB)
// ---------------------------------------------------------------------------

/// Map a `segment` value to `(mayhem, cashback)` predicate options. `None` =
/// "don't filter on this flag"; an unknown/`all`/missing value filters nothing.
fn parse_segment(segment: &str) -> (Option<bool>, Option<bool>) {
    match segment {
        "mayhem" => (Some(true), None),
        "non_mayhem" => (Some(false), None),
        "cashback" => (None, Some(true)),
        "non_cashback" => (None, Some(false)),
        _ => (None, None),
    }
}

/// Validate the trend granularity against the bucket tags we allow (defends the
/// interpolated expression against an arbitrary value — see `bucket_expr`).
/// Calendar units map to `date_trunc`; the sub-hour / multi-hour tags map to
/// `date_bin`. Defaults to `day`.
fn normalize_bucket(bucket: Option<&str>) -> &'static str {
    match bucket {
        Some("10m") => "10m",
        Some("30m") => "30m",
        Some("hour") => "hour",
        Some("4h") => "4h",
        Some("8h") => "8h",
        Some("12h") => "12h",
        Some("week") => "week",
        _ => "day",
    }
}

/// `YYYY-MM-DDTHH:MM[:SS]` (UTC wall-clock) or RFC3339 → instant. Mirrors the
/// tokens-list `parse_dt` contract: bare datetime-local is treated as UTC.
fn parse_dt(v: &str) -> Option<DateTime<Utc>> {
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(v) {
        return Some(d.with_timezone(&Utc));
    }
    let iso = if v.len() == 16 {
        format!("{v}:00Z")
    } else {
        format!("{v}Z")
    };
    DateTime::parse_from_rfc3339(&iso)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Resolve the `[from, to)` window: defaults to the last 30d, and clamps the span
/// to `MAX_WINDOW_DAYS` (moving `from` forward) so the range scan stays bounded.
fn resolve_window(
    from: Option<&str>,
    to: Option<&str>,
    now: DateTime<Utc>,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let to = to.and_then(parse_dt).unwrap_or(now);
    let mut from = from
        .and_then(parse_dt)
        .unwrap_or_else(|| to - Duration::days(DEFAULT_WINDOW_DAYS));
    if from >= to {
        from = to - Duration::days(DEFAULT_WINDOW_DAYS);
    }
    let max_from = to - Duration::days(MAX_WINDOW_DAYS);
    if from < max_from {
        from = max_from;
    }
    (from, to)
}

fn to_cell(r: HeatCellRow) -> HeatCell {
    HeatCell {
        dow: r.dow,
        hour: r.hour,
        count: r.total,
        matured: r.matured,
        known: r.known,
        migrated: r.migrated,
        dead: r.dead,
    }
}

fn to_point(r: TrendPointRow) -> TrendPoint {
    TrendPoint {
        bucket: r.bucket,
        count: r.total,
        matured: r.matured,
        known: r.known,
        migrated: r.migrated,
        dead: r.dead,
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/tokens/creation-stats` — token-creation-time bias aggregates.
///
/// Server-side GROUP BY over `tokens` ⋈ `tokens_info`; returns count + outcome
/// (migrated/dead) per time bucket plus window totals for share/coverage. All
/// three color metrics ship together so the UI toggles them without a refetch.
pub async fn get_creation_stats(
    state: web::Data<Arc<CoreState>>,
    query: web::Query<CreationStatsQuery>,
) -> impl Responder {
    let now = Utc::now();
    let view = match query.view.as_deref() {
        Some("trend") => "trend",
        _ => "heatmap",
    };
    let bucket = normalize_bucket(query.bucket.as_deref());
    let tz = query.tz.clone().unwrap_or_else(|| "UTC".to_string());
    let maturity_secs = query.maturity_secs.unwrap_or(DEFAULT_MATURITY_SECS).max(0.0);
    let segment = query.segment.clone().unwrap_or_else(|| "all".to_string());
    let (mayhem, cashback) = parse_segment(&segment);
    let (from, to) = resolve_window(query.from.as_deref(), query.to.as_deref(), now);

    let repo = state.creation_stats_repo();
    let filter = StatsFilter {
        tz: &tz,
        maturity_secs,
        from,
        to,
        mayhem,
        cashback,
    };

    let mut resp = CreationStatsResponse {
        view: view.to_string(),
        bucket: bucket.to_string(),
        tz: tz.clone(),
        from,
        to,
        maturity_secs,
        segment,
        total: 0,
        matured: 0,
        known: 0,
        cells: Vec::new(),
        points: Vec::new(),
    };

    if view == "trend" {
        match repo.trend(bucket, filter).await {
            Ok(rows) => {
                for r in &rows {
                    resp.total += r.total;
                    resp.matured += r.matured;
                    resp.known += r.known;
                }
                resp.points = rows.into_iter().map(to_point).collect();
            }
            Err(e) => return db_error(e),
        }
    } else {
        match repo.heatmap(filter).await {
            Ok(rows) => {
                for r in &rows {
                    resp.total += r.total;
                    resp.matured += r.matured;
                    resp.known += r.known;
                }
                resp.cells = rows.into_iter().map(to_cell).collect();
            }
            Err(e) => return db_error(e),
        }
    }

    HttpResponse::Ok().json(resp)
}

// ---------------------------------------------------------------------------
// Grouped (per-fingerprint) creation activity
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GroupedCreationQuery {
    /// Trend granularity: `hour` | `day` | `week` (default `day`).
    pub bucket: Option<String>,
    /// IANA timezone for bucketing; defaults to UTC.
    pub tz: Option<String>,
    /// Window bounds (RFC3339). Default: last 30d → now.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Segment filter: `all` | `mayhem` | `non_mayhem` | `cashback` | `non_cashback`.
    pub segment: Option<String>,
    /// Comma-separated `GroupField` serde tags, in compound-key order
    /// (e.g. `cu_limit,ix_labels`). Empty/missing ⇒ a single "ALL" group.
    pub group_by: Option<String>,
    /// Number of top groups (by volume) to return. Clamped to [1, MAX_TOP_GROUPS].
    pub top: Option<i64>,
    /// Bucket width (SOL) for the continuous SOL group fields — the same knob the
    /// grouped sweep uses, so the dashboard groups a corpus identically to a sweep
    /// at this width. Omitted/invalid ⇒ the default (`grouping::SOL_BUCKET_WIDTH`,
    /// 0.1); floored server-side at `grouping::MIN_BUCKET_WIDTH_SOL`.
    pub bucket_width: Option<f64>,
    /// Per-field value filters restricting the corpus *before* partitioning, as a
    /// JSON object `{"cu_limit":["300000"],"is_cashback_enabled":["true"]}` (keys =
    /// `GroupField` tags, values = allowed string forms matching the group key).
    /// Independent of `group_by`. Omitted/empty ⇒ no filter. `ix_labels` is handled
    /// by `ix_labels_filter` below (set-equality, not value-in).
    pub field_filters: Option<String>,
    /// Exact instruction-label set filter as a JSON array (`["A","B"]`): keep only
    /// tokens whose `ix_labels` set equals these labels (order-independent).
    /// Omitted/empty ⇒ no filter.
    pub ix_labels_filter: Option<String>,
    /// Optional saved-fingerprint scope — same contract as the sweep's/flow
    /// discovery's `fingerprint_id`: when set, the corpus keeps only tokens the
    /// ENGINE matches against that fingerprint (SQL mirror of
    /// `hunter_engine::fingerprint::matches`), collapsed into a single "ALL"
    /// group (`g = 0`); `group_by`/`top`/`field_filters`/`ix_labels_filter`/
    /// `bucket_width` above are all ignored (the fingerprint's own axes/bucket
    /// replace them, exactly as the sweep/discovery scoping does).
    #[serde(default)]
    pub fingerprint_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct GroupedGroup {
    /// 0-based rank index (0 = largest group). Keys `cells`/`points` back to a group.
    pub g: i64,
    /// `{"cu_limit":"200000","ix_labels":"A | B"}` — same shape as the sweep's group key.
    pub group_key: serde_json::Value,
    pub total: i64,
}

#[derive(Serialize)]
pub struct GroupedCell {
    pub g: i64,
    pub dow: i32,
    pub hour: i32,
    pub count: i64,
}

#[derive(Serialize)]
pub struct GroupedPoint {
    pub g: i64,
    pub bucket: NaiveDateTime,
    pub count: i64,
}

#[derive(Serialize)]
pub struct GroupedCreationResponse {
    pub bucket: String,
    pub tz: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub segment: String,
    /// The grouping fields echoed back (serde tags, in selection order).
    pub group_by: Vec<String>,
    /// The applied (clamped) bucket width (SOL) for the continuous SOL group fields.
    pub bucket_width: f64,
    /// The applied per-field value filters echoed back (`{"cu_limit":["300000"]}`).
    pub field_filters: serde_json::Value,
    /// The applied exact instruction-label set filter, or `null` when none.
    pub ix_labels_filter: Option<Vec<String>>,
    /// Σ counts across the returned (top-N) groups.
    pub total: i64,
    pub groups: Vec<GroupedGroup>,
    pub cells: Vec<GroupedCell>,
    pub points: Vec<GroupedPoint>,
}

/// Parse a comma-separated `group_by` tag list into ordered, de-duplicated fields.
/// Blank/missing ⇒ empty (the "ALL" group). `Err(tag)` on an unknown tag.
fn parse_group_by(raw: Option<&str>) -> Result<Vec<GroupField>, String> {
    let mut out: Vec<GroupField> = Vec::new();
    for tag in raw.unwrap_or("").split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let field = GroupField::from_tag(tag).ok_or_else(|| tag.to_string())?;
        if !out.contains(&field) {
            out.push(field);
        }
    }
    Ok(out)
}

/// Clamp the requested group count to `[1, MAX_TOP_GROUPS]` (default when absent).
fn clamp_top(top: Option<i64>) -> i64 {
    top.unwrap_or(DEFAULT_TOP_GROUPS).clamp(1, MAX_TOP_GROUPS)
}

/// Parse the `field_filters` JSON object into ordered `(field, allowed-values)`
/// pairs. Blank/missing ⇒ empty (no filter). `ix_labels` is ignored here (it has
/// its own set-equality filter). Empty value lists and blank values are dropped so
/// an empty box never pins "no values". `Err(msg)` on malformed JSON / unknown tag.
fn parse_field_filters(raw: Option<&str>) -> Result<Vec<(GroupField, Vec<String>)>, String> {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(raw).map_err(|e| format!("invalid field_filters JSON: {e}"))?;
    let mut out: Vec<(GroupField, Vec<String>)> = Vec::new();
    for (tag, vals) in obj {
        if tag == "ix_labels" {
            continue; // handled by ix_labels_filter
        }
        let field = GroupField::from_tag(&tag).ok_or_else(|| format!("unknown field: {tag}"))?;
        let arr = vals
            .as_array()
            .ok_or_else(|| format!("field_filters[{tag}] must be an array"))?;
        let values: Vec<String> = arr
            .iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.trim().to_string()),
                // Tolerate numbers/bools so the client can send raw JSON values.
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect();
        if !values.is_empty() {
            out.push((field, values));
        }
    }
    Ok(out)
}

/// Parse the `ix_labels_filter` JSON array of label strings. Blank/missing or an
/// empty array ⇒ `None` (no filter, so an empty `[]` never pins "no labels").
/// `Err(msg)` on malformed JSON.
fn parse_ix_labels_filter(raw: Option<&str>) -> Result<Option<Vec<String>>, String> {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let arr: Vec<String> =
        serde_json::from_str(raw).map_err(|e| format!("invalid ix_labels_filter JSON: {e}"))?;
    let labels: Vec<String> = arr.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    Ok(if labels.is_empty() { None } else { Some(labels) })
}

/// `GET /api/tokens/creation-stats/grouped` — per-fingerprint creation activity.
///
/// Partitions tokens by a compound fingerprint key (`group_by`), keeps the top-N
/// groups by volume, and returns each group's day×hour fold (`cells`) + calendar
/// trend (`points`). Count only (no outcome join). Reuses the aggregate endpoint's
/// window/tz/segment handling; the fingerprint fields mirror the sweep page.
pub async fn get_grouped_creation_stats(
    state: web::Data<Arc<CoreState>>,
    query: web::Query<GroupedCreationQuery>,
) -> impl Responder {
    let now = Utc::now();
    let bucket = normalize_bucket(query.bucket.as_deref());
    let tz = query.tz.clone().unwrap_or_else(|| "UTC".to_string());
    let segment = query.segment.clone().unwrap_or_else(|| "all".to_string());
    let (mayhem, cashback) = parse_segment(&segment);
    let (from, to) = resolve_window(query.from.as_deref(), query.to.as_deref(), now);
    let repo = state.creation_stats_repo();

    // Saved-fingerprint scope — short-circuits the manual group_by/filters path
    // entirely (same contract as the sweep's/flow discovery's fingerprint_id).
    if let Some(fp_id) = query.fingerprint_id {
        let fp = match state.fingerprint_repo().find(fp_id).await {
            Ok(Some(fp)) => fp,
            Ok(None) => {
                return HttpResponse::NotFound()
                    .json(serde_json::json!({ "error": "fingerprint not found" }));
            }
            Err(e) => return db_error(e),
        };
        let filter = StatsFilter { tz: &tz, maturity_secs: 0.0, from, to, mayhem, cashback };
        let data = match repo.grouped_scoped(&fp, bucket, filter).await {
            Ok(d) => d,
            Err(e) => return db_error(e),
        };
        let total: i64 = data.groups.iter().map(|g| g.total).sum();
        let resp = GroupedCreationResponse {
            bucket: bucket.to_string(),
            tz,
            from,
            to,
            segment,
            group_by: Vec::new(),
            // The fingerprint's OWN width, never the caller's and never a
            // substituted default — the scoped card must report the same width
            // the engine matcher grades this fingerprint at.
            bucket_width: fp.bucket_size_amount,
            field_filters: serde_json::json!({}),
            ix_labels_filter: configured_labels(fp.ix_labels.as_deref()).map(<[String]>::to_vec),
            total,
            groups: data
                .groups
                .into_iter()
                .map(|r| GroupedGroup { g: r.g, group_key: r.group_key, total: r.total })
                .collect(),
            cells: data
                .cells
                .into_iter()
                .map(|r| GroupedCell { g: r.g, dow: r.dow, hour: r.hour, count: r.count })
                .collect(),
            points: data
                .points
                .into_iter()
                .map(|r| GroupedPoint { g: r.g, bucket: r.bucket, count: r.count })
                .collect(),
        };
        return HttpResponse::Ok().json(resp);
    }

    let top = clamp_top(query.top);
    // Clamp the bucket width like the sweep handler: invalid/sub-floor ⇒ default,
    // so the dashboard groups a corpus at the same width a sweep would.
    let bucket_width = match query.bucket_width {
        Some(w) if w.is_finite() && w >= crate::grouping::MIN_BUCKET_WIDTH_SOL => w,
        _ => crate::grouping::SOL_BUCKET_WIDTH,
    };

    let fields = match parse_group_by(query.group_by.as_deref()) {
        Ok(f) => f,
        Err(tag) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("unknown group_by field: {tag}")
            }));
        }
    };

    let field_filters = match parse_field_filters(query.field_filters.as_deref()) {
        Ok(f) => f,
        Err(msg) => {
            return HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }));
        }
    };
    let ix_labels_filter = match parse_ix_labels_filter(query.ix_labels_filter.as_deref()) {
        Ok(f) => f,
        Err(msg) => {
            return HttpResponse::BadRequest().json(serde_json::json!({ "error": msg }));
        }
    };

    let filter = StatsFilter {
        tz: &tz,
        // Unused (count-only) but the shared struct requires it; 0 = no censoring.
        maturity_secs: 0.0,
        from,
        to,
        mayhem,
        cashback,
    };

    let data = match repo
        .grouped(&fields, bucket, top, &field_filters, ix_labels_filter.as_deref(), bucket_width, filter)
        .await
    {
        Ok(d) => d,
        Err(e) => return db_error(e),
    };

    // Echo the applied filters back as a JSON object (same shape the client sent).
    let field_filters_json = serde_json::Value::Object(
        field_filters
            .iter()
            .map(|(f, vals)| {
                (
                    f.as_str().to_string(),
                    serde_json::Value::Array(
                        vals.iter().map(|v| serde_json::Value::String(v.clone())).collect(),
                    ),
                )
            })
            .collect(),
    );

    let total: i64 = data.groups.iter().map(|g| g.total).sum();
    let resp = GroupedCreationResponse {
        bucket: bucket.to_string(),
        tz,
        from,
        to,
        segment,
        group_by: fields.iter().map(|f| f.as_str().to_string()).collect(),
        bucket_width,
        field_filters: field_filters_json,
        ix_labels_filter,
        total,
        groups: data
            .groups
            .into_iter()
            .map(|r| GroupedGroup {
                g: r.g,
                group_key: r.group_key,
                total: r.total,
            })
            .collect(),
        cells: data
            .cells
            .into_iter()
            .map(|r| GroupedCell {
                g: r.g,
                dow: r.dow,
                hour: r.hour,
                count: r.count,
            })
            .collect(),
        points: data
            .points
            .into_iter()
            .map(|r| GroupedPoint {
                g: r.g,
                bucket: r.bucket,
                count: r.count,
            })
            .collect(),
    };

    HttpResponse::Ok().json(resp)
}

// ---------------------------------------------------------------------------
// Grouped-creation "drill-down" token list — the actual tokens behind one
// group card (or one of its heatmap tiles).
// ---------------------------------------------------------------------------

/// Hard cap on the drill-down page size — a table page, not a bulk export.
const MAX_GROUPED_TOKENS_PAGE_SIZE: i64 = 500;

/// `POST /api/tokens/creation-stats/grouped/tokens` body. Mirrors
/// [`GroupedCreationQuery`]'s window/segment/corpus fields (so the drill-down
/// reads the SAME corpus `grouped()` ranked) plus the two selectors that pin it
/// to one row: `group_key` (required — the exact group, echoing back
/// [`GroupedGroup::group_key`]) and, for a heatmap-tile click, `dow`/`hour`.
/// The embedded [`TableRequest`] supplies pagination/sort/search AND its
/// per-column `filters`: the corpus is scoped to one group by the fields above,
/// then the drill-in table's FilterPanel/per-column grammar is layered on via the
/// live Tokens list's SSOT filter→SQL builder (`build_filter_where_from`), so a
/// numeric/flag column filter narrows the rows AND the pager total. (`range` /
/// `tracked_only` remain unused — the corpus window is the fields above.)
#[derive(Deserialize)]
pub struct GroupedCreationTokensRequest {
    #[serde(flatten)]
    pub table: TableRequest,
    pub tz: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub segment: Option<String>,
    pub group_by: Option<String>,
    pub bucket_width: Option<f64>,
    pub field_filters: Option<String>,
    pub ix_labels_filter: Option<String>,
    /// The exact group to drill into: `{"cu_limit":"200000","ix_labels":"A | B"}`
    /// — same rendered shape as [`GroupedGroup::group_key`]. Required to carry a
    /// value for every field named in `group_by` (a missing one is silently
    /// dropped from the match, so an incomplete key over-matches — validated below).
    pub group_key: serde_json::Value,
    /// Recurring weekly slot (a heatmap-tile click): 0=Sun..6=Sat / 0..23. Both
    /// present or both absent — a lone one is a 400.
    pub dow: Option<i32>,
    pub hour: Option<i32>,
    /// Saved-fingerprint scope — mirrors [`GroupedCreationQuery::fingerprint_id`].
    /// When set, `group_by`/`field_filters`/`ix_labels_filter`/`group_key` above
    /// are all ignored (there's only ever one group, `g = 0`) and the corpus is
    /// the fingerprint's own engine-matched tokens.
    #[serde(default)]
    pub fingerprint_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct GroupedCreationTokensResponse {
    pub total: i64,
    pub items: Vec<TokenSummary>,
}

/// `POST /api/tokens/creation-stats/grouped/tokens` — the concrete tokens
/// behind one "Creation by token group" card, optionally narrowed to one of
/// its heatmap tiles. Filters the SAME `tokens`/`tokens_info` corpus
/// `get_grouped_creation_stats` ranks (window, segment, `field_filters`,
/// `ix_labels_filter`, `bucket_width`) down to rows matching one exact
/// `group_key`, then pages it through the live Tokens list's own SQL
/// projection (`token_repo::{find_list_page,count_list}`) — so the drill-down
/// table renders with the identical column set/enrichment as the Tokens page.
pub async fn get_grouped_creation_tokens(
    state: web::Data<Arc<CoreState>>,
    body: web::Json<GroupedCreationTokensRequest>,
) -> impl Responder {
    let body = body.into_inner();
    let now = Utc::now();
    let tz = body.tz.clone().unwrap_or_else(|| "UTC".to_string());
    let segment = body.segment.clone().unwrap_or_else(|| "all".to_string());
    let (mayhem, cashback) = parse_segment(&segment);
    let (from, to) = resolve_window(body.from.as_deref(), body.to.as_deref(), now);
    let bucket_width = match body.bucket_width {
        Some(w) if w.is_finite() && w >= crate::grouping::MIN_BUCKET_WIDTH_SOL => w,
        _ => crate::grouping::SOL_BUCKET_WIDTH,
    };

    // dow/hour: both-or-neither, and in-range when present.
    let cell = match (body.dow, body.hour) {
        (Some(dow), Some(hour)) if (0..=6).contains(&dow) && (0..=23).contains(&hour) => {
            Some((dow, hour))
        }
        (None, None) => None,
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "dow (0..6) and hour (0..23) must both be set, or both omitted"
            }));
        }
    };
    let (dow, hour) = cell.map_or((None, None), |(d, h)| (Some(d), Some(h)));

    let filter = StatsFilter { tz: &tz, maturity_secs: 0.0, from, to, mayhem, cashback };

    let (mut where_sql, mut args) = if let Some(fp_id) = body.fingerprint_id {
        // Saved-fingerprint scope — group_by/field_filters/ix_labels_filter/
        // group_key above are all ignored (mirrors the main endpoint's contract).
        let fp = match state.fingerprint_repo().find(fp_id).await {
            Ok(Some(fp)) => fp,
            Ok(None) => {
                return HttpResponse::NotFound()
                    .json(serde_json::json!({ "error": "fingerprint not found" }));
            }
            Err(e) => return db_error(e),
        };
        crate::storage::repositories::creation_stats_repo::build_grouped_tokens_where_scoped(
            &fp,
            dow,
            hour,
            &body.table.search,
            filter,
        )
    } else {
        let fields = match parse_group_by(body.group_by.as_deref()) {
            Ok(f) => f,
            Err(tag) => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("unknown group_by field: {tag}")
                }));
            }
        };
        let field_filters = match parse_field_filters(body.field_filters.as_deref()) {
            Ok(f) => f,
            Err(msg) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
        };
        let ix_labels_filter = match parse_ix_labels_filter(body.ix_labels_filter.as_deref()) {
            Ok(f) => f,
            Err(msg) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })),
        };

        // `group_key` must name every selected field — else the "exact group" the
        // client thinks it's drilling into is actually broader than what it saw
        // (a silently-dropped field would match every value of that field).
        let group_key_obj = match body.group_key.as_object() {
            Some(o) => o,
            None => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({ "error": "group_key must be a JSON object" }));
            }
        };
        let mut group_key: Vec<(GroupField, String)> = Vec::with_capacity(fields.len());
        for field in &fields {
            let Some(v) = group_key_obj.get(field.as_str()) else {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("group_key missing value for field: {}", field.as_str())
                }));
            };
            let Some(s) = v.as_str() else {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("group_key[{}] must be a string", field.as_str())
                }));
            };
            group_key.push((*field, s.to_string()));
        }

        crate::storage::repositories::creation_stats_repo::build_grouped_tokens_where(
            &fields,
            &group_key,
            &field_filters,
            ix_labels_filter.as_deref(),
            bucket_width,
            dow,
            hour,
            &body.table.search,
            filter,
        )
    };

    // Layer the drill-in table's per-column filters (numeric/amount/flag/identity
    // grammar) onto the base corpus scope. The grouped corpus + window + global
    // search are already applied above; here we lift ONLY the FilterPanel/col-filter
    // grammar from `body.table.filters`, reusing the live Tokens list's SSOT
    // filter→SQL builder (no re-implementation). Search is excluded (applied above);
    // placeholders start after the base args so the two fragments compose cleanly.
    let filter_query =
        crate::api::handlers::tokens::TokenQuery::from_table_request(&body.table);
    if let Some((filter_sql, filter_args)) =
        crate::api::handlers::tokens::build_filter_where_from(&filter_query, now, args.len())
    {
        where_sql = format!("({where_sql}) AND ({filter_sql})");
        args.extend(filter_args);
    }

    let order_sql = crate::storage::repositories::creation_stats_repo::build_grouped_tokens_order(
        &body
            .table
            .sorting
            .iter()
            .map(|s| (s.col.clone(), s.dir.is_desc()))
            .collect::<Vec<_>>(),
    );

    let (limit, offset) = {
        let page = body.table.pagination.page.max(1);
        let page_size = body.table.pagination.page_size.clamp(1, MAX_GROUPED_TOKENS_PAGE_SIZE);
        (page_size, (page - 1) * page_size)
    };

    let repo = state.token_repo();
    let total = match repo.count_list(&where_sql, &args).await {
        Ok(n) => n,
        Err(e) => return db_error(e),
    };
    let rows = match repo.find_list_page(&where_sql, &order_sql, &args, limit, offset).await {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };
    let items: Vec<TokenSummary> = rows.into_iter().map(TokenSummary::from).collect();

    HttpResponse::Ok().json(GroupedCreationTokensResponse { total, items })
}

fn db_error(e: anyhow::Error) -> HttpResponse {
    tracing::error!("creation-stats query failed: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({
        "error": "failed to compute creation stats"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drill-in filter fragment must renumber onto the grouped base's args and
    /// compose into a single valid WHERE (the exact merge the handler performs).
    #[test]
    fn drill_in_filters_merge_onto_grouped_base() {
        use crate::api::handlers::tokens::{build_filter_where_from, SqlArg, TokenQuery};
        use crate::api::table_query::TableRequest;

        // A grouped base that already consumed 3 args ($1..$3).
        let mut where_sql = "t.created_at >= $1 AND t.created_at < $2 AND t.is_mayhem_mode = $3".to_string();
        let mut args: Vec<SqlArg> = vec![
            SqlArg::Ts(Utc::now()),
            SqlArg::Ts(Utc::now()),
            SqlArg::Bool(true),
        ];

        // A drill-in table filtering trade_count >= 100 (numeric) + dead=yes (flag).
        let req: TableRequest = serde_json::from_value(serde_json::json!({
            "filters": {
                "trade_count": {"op": "gte", "val": 100},
                "dead": {"op": "eq", "val": "yes"},
            }
        }))
        .unwrap();
        let tq = TokenQuery::from_table_request(&req);

        let (filter_sql, filter_args) =
            build_filter_where_from(&tq, Utc::now(), args.len()).expect("active filter");
        where_sql = format!("({where_sql}) AND ({filter_sql})");
        args.extend(filter_args);

        // The numeric predicate's bound renumbered to $4 (after the 3 base args);
        // the tri-flag emits no bind, so total args = 3 base + 1 filter.
        assert!(where_sql.contains(">= $4"), "got: {where_sql}");
        assert!(where_sql.contains("COALESCE(i.is_dead, false) = true"), "got: {where_sql}");
        assert!(!where_sql.contains("$5"), "no extra placeholder: {where_sql}");
        assert_eq!(args.len(), 4);
    }

    #[test]
    fn segment_maps_to_flag_predicates() {
        assert_eq!(parse_segment("all"), (None, None));
        assert_eq!(parse_segment("whatever"), (None, None));
        assert_eq!(parse_segment("mayhem"), (Some(true), None));
        assert_eq!(parse_segment("non_mayhem"), (Some(false), None));
        assert_eq!(parse_segment("cashback"), (None, Some(true)));
        assert_eq!(parse_segment("non_cashback"), (None, Some(false)));
    }

    #[test]
    fn bucket_defaults_and_whitelists() {
        assert_eq!(normalize_bucket(None), "day");
        assert_eq!(normalize_bucket(Some("bogus")), "day");
        assert_eq!(normalize_bucket(Some("hour")), "hour");
        assert_eq!(normalize_bucket(Some("week")), "week");
    }

    #[test]
    fn window_defaults_to_30d() {
        let now = DateTime::parse_from_rfc3339("2026-06-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (from, to) = resolve_window(None, None, now);
        assert_eq!(to, now);
        assert_eq!((to - from).num_days(), DEFAULT_WINDOW_DAYS);
    }

    #[test]
    fn window_span_is_clamped() {
        let now = DateTime::parse_from_rfc3339("2026-06-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // from far in the past → clamped to MAX_WINDOW_DAYS before `to`.
        let (from, to) = resolve_window(Some("2000-01-01T00:00:00Z"), None, now);
        assert_eq!((to - from).num_days(), MAX_WINDOW_DAYS);
    }

    #[test]
    fn group_by_parses_and_dedups_in_order() {
        // Empty / missing ⇒ the ALL group.
        assert_eq!(parse_group_by(None).unwrap(), vec![]);
        assert_eq!(parse_group_by(Some("")).unwrap(), vec![]);
        assert_eq!(parse_group_by(Some(" , ")).unwrap(), vec![]);
        // Order preserved; whitespace trimmed; duplicates dropped (keep first).
        assert_eq!(
            parse_group_by(Some("cu_limit, ix_labels , cu_limit")).unwrap(),
            vec![GroupField::CuLimit, GroupField::IxLabels],
        );
        // Unknown tag ⇒ Err(tag).
        assert_eq!(parse_group_by(Some("cu_limit,bogus")), Err("bogus".to_string()));
    }

    #[test]
    fn bucket_whitelists_fine_and_coarse_tags() {
        assert_eq!(normalize_bucket(Some("10m")), "10m");
        assert_eq!(normalize_bucket(Some("30m")), "30m");
        assert_eq!(normalize_bucket(Some("4h")), "4h");
        assert_eq!(normalize_bucket(Some("8h")), "8h");
        assert_eq!(normalize_bucket(Some("12h")), "12h");
        // Unknown / missing → day.
        assert_eq!(normalize_bucket(Some("5m")), "day");
        assert_eq!(normalize_bucket(None), "day");
    }

    #[test]
    fn field_filters_parse_drops_blanks_and_ix_labels() {
        // Empty / missing ⇒ no filter.
        assert_eq!(parse_field_filters(None).unwrap(), vec![]);
        assert_eq!(parse_field_filters(Some(" ")).unwrap(), vec![]);
        // Strings, numbers, and bools all coerce to string; blanks/empty dropped;
        // ix_labels ignored (handled separately).
        let got = parse_field_filters(Some(
            r#"{"cu_limit":["300000"," "],"cu_price":[1000],"is_cashback_enabled":[true],"max_cost_lamports":[],"ix_labels":["A"]}"#,
        ))
        .unwrap();
        assert_eq!(
            got,
            vec![
                (GroupField::CuLimit, vec!["300000".to_string()]),
                (GroupField::CuPrice, vec!["1000".to_string()]),
                (GroupField::IsCashbackEnabled, vec!["true".to_string()]),
            ],
        );
        // Unknown tag / malformed JSON ⇒ Err.
        assert!(parse_field_filters(Some(r#"{"bogus":["x"]}"#)).is_err());
        assert!(parse_field_filters(Some("not json")).is_err());
    }

    #[test]
    fn ix_labels_filter_parse() {
        assert_eq!(parse_ix_labels_filter(None).unwrap(), None);
        assert_eq!(parse_ix_labels_filter(Some("[]")).unwrap(), None);
        assert_eq!(parse_ix_labels_filter(Some(r#"["",  " "]"#)).unwrap(), None);
        assert_eq!(
            parse_ix_labels_filter(Some(r#"["A"," B "]"#)).unwrap(),
            Some(vec!["A".to_string(), "B".to_string()]),
        );
        assert!(parse_ix_labels_filter(Some("not json")).is_err());
    }

    #[test]
    fn top_groups_clamped() {
        assert_eq!(clamp_top(None), DEFAULT_TOP_GROUPS);
        assert_eq!(clamp_top(Some(0)), 1);
        assert_eq!(clamp_top(Some(-5)), 1);
        assert_eq!(clamp_top(Some(5)), 5);
        assert_eq!(clamp_top(Some(999)), MAX_TOP_GROUPS);
    }

    #[test]
    fn inverted_window_falls_back() {
        let now = DateTime::parse_from_rfc3339("2026-06-16T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // from after to → ignore and use the 30d default ending at `to`.
        let (from, to) =
            resolve_window(Some("2026-06-15T00:00:00Z"), Some("2026-06-01T00:00:00Z"), now);
        assert_eq!((to - from).num_days(), DEFAULT_WINDOW_DAYS);
    }
}
