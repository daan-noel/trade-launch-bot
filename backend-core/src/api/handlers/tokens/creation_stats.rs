use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::core_state::CoreState;
use crate::storage::repositories::creation_stats_repo::{HeatCellRow, StatsFilter, TrendPointRow};
use crate::grouping::GroupField;

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
    let top = clamp_top(query.top);

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

    let repo = state.creation_stats_repo();
    let data = match repo
        .grouped(&fields, bucket, top, &field_filters, ix_labels_filter.as_deref(), filter)
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

fn db_error(e: anyhow::Error) -> HttpResponse {
    tracing::error!("creation-stats query failed: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({
        "error": "failed to compute creation stats"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            r#"{"cu_limit":["300000"," "],"cu_price":[1000],"is_cashback_enabled":[true],"max_sol_cost":[],"ix_labels":["A"]}"#,
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
