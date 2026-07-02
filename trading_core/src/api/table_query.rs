//! Shared **table request** wire contract for every server-side strategy token
//! table (Positions / Paper / Matched / Simulated). One JSON `POST` body shape
//! deserializes into [`TableRequest`]; handlers convert it into the repo-level
//! [`PositionQuery`](crate::storage::repositories::strategy_repo::PositionQuery)
//! (and its token-scoped siblings) via `From`.
//!
//! The key departure from the old flat query-string contract is the **structured
//! per-column filter** ([`FilterSpec`] + [`FilterOp`]): numeric operators (`gt`,
//! `between`, …) compare *numerically server-side* instead of the previous
//! `::text ILIKE '%…%'` substring match. See the SQL builder in `strategy_repo`
//! (`push_position_where`) for how each op is lowered to a bound predicate.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// One JSON request body shared by all four strategy token tables. Every field is
/// `#[serde(default)]` so a partial/empty body is valid (empty search, no filters,
/// default paging). `range` is only meaningful for the matched/simulated tables.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRequest {
    #[serde(default)]
    pub pagination: Page,
    #[serde(default)]
    pub sorting: Vec<SortSpec>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub filters: HashMap<String, FilterSpec>,
    /// Matched / simulated only — the creation-time analysis window.
    #[serde(default)]
    pub range: Option<AnalysisRange>,
}

/// 1-based page + page size. [`Page::bounds`] clamps to the repo's historical
/// `(limit ∈ 1..=1000, offset ≥ 0)` envelope.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    200
}

impl Default for Page {
    fn default() -> Self {
        Self { page: default_page(), page_size: default_page_size() }
    }
}

impl Page {
    /// Resolve to `(limit, offset)` for a SQL `LIMIT/OFFSET`, applying the same
    /// bounds the old `PositionListParams::bounds` used: `limit.clamp(1, 1000)` and
    /// `offset = (page - 1) * limit`, never negative.
    pub fn bounds(&self) -> (i64, i64) {
        let limit = self.page_size.clamp(1, 1000);
        let offset = (self.page.max(1) - 1) * limit;
        (limit, offset.max(0))
    }
}

/// One ordered sort key. `col` is a **frontend** column key; the repo whitelists it
/// against a trusted SQL expression (non-whitelisted → dropped).
#[derive(Debug, Clone, Deserialize)]
pub struct SortSpec {
    pub col: String,
    #[serde(default)]
    pub dir: SortDir,
}

/// Sort direction — defaults to ascending.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    /// `true` for descending — the boolean the repo sort machinery expects.
    pub fn is_desc(self) -> bool {
        matches!(self, SortDir::Desc)
    }
}

/// A structured per-column filter. `op` selects the comparison; the operand lives
/// in `val` (single-operand ops) or `min`/`max` (`between`). Values are
/// `serde_json::Value` so a numeric op can read a JSON number and a text op a
/// string from the **same** field — the SQL builder validates the shape per op and
/// drops mismatches (e.g. a numeric op with a non-number `val`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilterSpec {
    #[serde(default)]
    pub op: FilterOp,
    #[serde(default)]
    pub val: serde_json::Value,
    #[serde(default)]
    pub min: serde_json::Value,
    #[serde(default)]
    pub max: serde_json::Value,
}

/// The comparison operator for a [`FilterSpec`]. `Contains` is the default and the
/// only op allowed on text columns (current `ILIKE '%…%'` behavior); the numeric
/// ops require a numeric-typed column (see `strategy_repo` whitelists).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterOp {
    #[default]
    Contains,
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Between,
}

/// Transient creation-time analysis window (matched/simulated scans). Both bounds
/// optional; empty = all-time. `from` → `since` (inclusive), `to` → `until`
/// (exclusive). Accepts a full RFC3339 instant or the frontend's offset-less UTC
/// wall-clock (`YYYY-MM-DDTHH:MM[:SS]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnalysisRange {
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "de_opt_wallclock_utc")]
    pub to: Option<DateTime<Utc>>,
}

/// Lenient deserializer for the optional analysis-window bounds: accepts a full
/// RFC3339 instant or the frontend's offset-less UTC wall-clock
/// (`YYYY-MM-DDTHH:MM[:SS]`), appending `Z`/`:00Z` to the latter.
fn de_opt_wallclock_utc<'de, D>(de: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(de)?;
    let Some(v) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if let Ok(d) = DateTime::parse_from_rfc3339(v) {
        return Ok(Some(d.with_timezone(&Utc)));
    }
    let iso = if v.len() == 16 { format!("{v}:00Z") } else { format!("{v}Z") };
    DateTime::parse_from_rfc3339(&iso)
        .map(|d| Some(d.with_timezone(&Utc)))
        .map_err(serde::de::Error::custom)
}
