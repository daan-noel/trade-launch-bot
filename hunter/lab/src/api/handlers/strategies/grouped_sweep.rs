//! Endpoints for **grouped** strategy param-sweeps (`/api/strategies/sweeps`).
//!
//! One generic handler set serves every strategy: the `strategy_id` resolves a
//! per-strategy table triple ([`registry::tables_for`]) and a sweep entry point
//! ([`registry::run_grouped`]). `start_grouped_sweep` selects tokens by a
//! `created_at` range, partitions them by the chosen fingerprint fields, sweeps
//! each surviving group, and persists run → groups → ranked combo rows. The
//! reads back the group-summary list and a group's ranked combo table for the
//! drill-in view.
//!
//! Shares the single-flight `sweep_running` gate with the live-cache TPSL2 sweep
//! (one CPU-heavy sweep at a time; the rayon pool is bounded inside the registry
//! so it can't starve the live trading hot path).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use futures_util::StreamExt as _;
use tokio_stream::wrappers::ReceiverStream;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::grouped_sweep::{GroupedSweepGroupWrite, GroupedSweepResult, GroupedSweepRun};
use crate::state::local_state::SweepCorpusCache;
use crate::state::local_state::LocalState;
use crate::storage::repositories::grouped_sweep_repo::{GroupedSweepRepo, GroupedSweepTables};
use crate::sweep::aggregate::ComboMetrics;
use crate::lake::duck::LakeSource;
use crate::sweep::corpus::{sweep_per_mint_cap, CorpusSource, Selection};
use crate::sweep::grouped_engine::{CoverageFloor, GroupResult, GroupSink};
use crate::sweep::grouping::{group_key, normalize_label_vec, GroupField, SOL_BUCKET_WIDTH};
use crate::sweep::registry;
use crate::sweep::strategy::parse_method;
use trading_core::config::constants::{sol_to_lamports, tidy_sol_decimal};
use trading_core::models::Fingerprint;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;

// ---------------------------------------------------------------------------
// Request / query bodies
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct StartGroupedSweepBody {
    /// Which strategy to sweep (`"tpsl2"`). Resolves the table set + entry point.
    pub strategy_id: String,
    /// Selection lower bound (inclusive). Omitted ⇒ no lower bound.
    #[serde(default)]
    pub created_after: Option<DateTime<Utc>>,
    /// Selection upper bound (exclusive). Omitted ⇒ no upper bound.
    #[serde(default)]
    pub created_before: Option<DateTime<Utc>>,
    /// Restrict to bonding-curve trades only (drop migrated-AMM legs).
    #[serde(default)]
    pub curve_only: bool,
    /// Fingerprint fields to group by (exact-value). Empty ⇒ one "ALL" group.
    #[serde(default)]
    pub group_by: Vec<GroupField>,
    /// Per-run bucket width (SOL) for the **continuous** SOL grouping fields
    /// (initial-buy, max-cost, spendable-in, first-slot buy/sell). This is the
    /// single width the partition, the created rule's live matcher, and the
    /// creation-stats dashboard all bucket by — so "what you swept = what you run".
    /// Discrete fields ignore it. Omitted ⇒ [`grouping::SOL_BUCKET_WIDTH`] (0.1);
    /// clamped to a [`grouping::MIN_BUCKET_WIDTH_SOL`] floor server-side.
    #[serde(default = "default_bucket_width_sol")]
    pub bucket_width_sol: f64,
    /// Optional exact-set instruction-label filter: when set (and non-empty),
    /// restrict the corpus to tokens whose normalized `ix_labels` set EXACTLY
    /// equals these labels, then sweep that slice. The page offers this as the
    /// alternative to grouping by `ix_labels` — it's disabled there when the
    /// `IxLabels` group-by is selected. Empty/omitted ⇒ no filter.
    #[serde(default)]
    pub ix_labels_filter: Option<Vec<String>>,
    /// Drop groups with fewer than this many tokens before sweeping.
    #[serde(default = "default_min_tokens")]
    pub min_tokens: usize,
    /// Coverage floor — absolute minimum fired tokens a combo needs before it's
    /// eligible to be a group's headline pick (over-fit guard, finding #2).
    #[serde(default = "default_min_fired_abs")]
    pub min_fired_abs: u64,
    /// Coverage floor — fraction of the group's tokens a combo must fire on
    /// (the floor is `max(min_fired_abs, ceil(fire_frac · group_tokens))`).
    #[serde(default = "default_fire_frac")]
    pub fire_frac: f64,
    /// `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. `refine` runs a coarse LHS
    /// pass of `N` draws then re-sweeps a neighborhood around each group's top-`K`
    /// combos (`K` default 3). Defaults to a full grid.
    #[serde(default)]
    pub method: Option<String>,
    /// Strategy-specific param axes (e.g. TPSL2's `AxesSpec`). Omitted axes fall
    /// back to that strategy's hardcoded defaults.
    #[serde(default = "default_axes")]
    pub axes: serde_json::Value,
    /// Hard cap on tokens loaded for the corpus — newest-N from the lake token
    /// dimension (`ORDER BY created_at DESC LIMIT`). Clamped to
    /// [`registry::MAX_TOKEN_CAP`]; the clamped value is what runs and what is
    /// persisted on the run row.
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
    /// Per-group combo cap override. Omitted ⇒ the default `MAX_COMBOS`; clamped
    /// server-side to `HARD_MAX_COMBOS` so a typo can't run away.
    #[serde(default)]
    pub max_combos: Option<usize>,
    /// Notional (SOL) to price every simulated round-trip at. Affects the fixed
    /// per-leg cost (Jito tip + priority fee) as a fraction of the trade size —
    /// set this to the live `buy_amount_sol` so backtest PnL% matches live results.
    /// Omitted ⇒ 1.0 SOL (the server default).
    #[serde(default = "default_buy_amount_sol")]
    pub buy_amount_sol: f64,
    /// Per-field value filters — restrict the corpus to tokens whose fingerprint
    /// value for the named field is in the allowed set. Map key = `GroupField`
    /// serde tag (e.g. `"cu_price"`); value = JSON array of allowed numbers.
    /// Empty array or absent key ⇒ no filter for that field. `"ix_labels"` is
    /// skipped here (use `ix_labels_filter` instead). Applied post-fingerprint,
    /// in-memory, so the unfiltered Parquet corpus cache is reused.
    #[serde(default)]
    pub field_filters: Option<std::collections::HashMap<String, Vec<serde_json::Value>>>,
    /// Optional saved-fingerprint scope. When set, the corpus keeps only tokens the
    /// ENGINE matches against that fingerprint (`hunter_engine::fingerprint::matches`
    /// — the same exact + bucket axes the live entry gate uses), and the manual
    /// `ix_labels_filter` / `field_filters` are ignored: those compare exact values,
    /// so they cannot express a bucket axis. Same scoping contract as flow discovery.
    /// `group_by` still applies — it partitions *within* the matched slice.
    #[serde(default)]
    pub fingerprint_id: Option<Uuid>,
    /// Host RAM (MB) to leave free for the OS + desktop while this run sizes its
    /// series/fold/combo peaks. Every admission ceiling is `host free − this`, so
    /// a smaller reserve admits bigger runs on a box you're not using and a bigger
    /// one keeps the machine responsive. Omitted ⇒
    /// `DEFAULT_SWEEP_RAM_RESERVE_MB` (1 GB); clamped server-side to
    /// `MIN..=MAX_SWEEP_RAM_RESERVE_MB`. Run-local (not persisted): it's a
    /// property of the machine at run time, not of the sweep's analysis.
    #[serde(default)]
    pub ram_reserve_mb: Option<usize>,
    /// Opt into the AVX-512 vectorized per-`(combo × token)` exit scan for this run
    /// (lab-only speedup; the box never ships to EC2). Honored only when the host
    /// actually has AVX-512 — otherwise the run is forced onto the scalar scan and a
    /// toast says so. Like `ram_reserve_mb` it is a property of *how the box
    /// computed*, not of the analysis, so it is NOT persisted on the run row and
    /// leaves results comparable regardless of path. Omitted ⇒ scalar.
    #[serde(default)]
    pub use_avx512: Option<bool>,
    /// Corpus-wide volume-ix patterns for flow-metric axes (`string[][]`).
    /// Required when `axes` reference `m_flow_split` / `m_flow_split_window`; Promote
    /// writes them into the fingerprint's `metric_config`.
    #[serde(default)]
    pub volume_ix_patterns: Option<Vec<Vec<String>>>,
    /// Which trade in the fill window prices each simulated leg — mirrors
    /// `EngineSimRequest.fill_model`, and threads the same `FillModel` the sweep's
    /// scan and `run_replay` both take. Omitted ⇒ `worst_case`, which is what the
    /// sweep hardcoded before, so every stored/replayed run keeps its meaning.
    ///
    /// **A ranking under one fill model does not carry to another.** Worst-in-slot
    /// penalises short holds disproportionately, so it biases a grid toward wide
    /// retraces and long holds — the exact direction the deep-dip analysis rejected.
    #[serde(default)]
    pub fill_model: trading_core::strategies::paper_fill::FillModel,
    /// Which execution-cost model prices the round-trips. Omitted ⇒
    /// `pumpfun_default` (legacy meaning). Pick `pumpfun_fee_only` alongside any
    /// explicit `fill_model`: the fill price already prices execution slippage, so
    /// charging `slippage_bps` again double-counts it — and because the fixed cost is
    /// per-leg, that haircut is **not** rank-preserving across combos.
    #[serde(default)]
    pub cost_model: trading_core::strategies::kernel::CostModelKind,
}

/// The [`Pricing`](crate::sweep::generic::Pricing) a stored run was computed under.
///
/// The persisted columns are the serde tags; a `NULL` (legacy row, written before the
/// models were selectable) or an unparseable value falls back to what the sweep
/// hardcoded then — worst-case fills + `pumpfun_default` — so an old run re-simulates
/// as it originally scored rather than under today's defaults.
fn run_pricing(run: &GroupedSweepRun) -> crate::sweep::generic::Pricing {
    fn tag<T: serde::de::DeserializeOwned + Default>(v: &Option<String>) -> T {
        v.as_deref()
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok())
            .unwrap_or_default()
    }
    crate::sweep::generic::Pricing {
        buy_amount_sol: run.buy_amount_sol.unwrap_or(registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL),
        fill_model: tag(&run.fill_model),
        cost: tag::<trading_core::strategies::kernel::CostModelKind>(&run.cost_model).model(),
    }
}

fn default_min_tokens() -> usize {
    10
}
fn default_bucket_width_sol() -> f64 {
    crate::sweep::grouping::SOL_BUCKET_WIDTH
}
fn default_min_fired_abs() -> u64 {
    10
}
fn default_fire_frac() -> f64 {
    0.05
}
fn default_token_cap() -> usize {
    registry::DEFAULT_TOKEN_CAP
}
fn default_axes() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}
fn default_buy_amount_sol() -> f64 {
    registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL
}

#[derive(serde::Deserialize)]
pub struct RunsQuery {
    pub strategy_id: String,
    #[serde(default = "default_runs_limit")]
    pub limit: i64,
}

fn default_runs_limit() -> i64 {
    50
}

#[derive(serde::Deserialize)]
pub struct StrategyQuery {
    pub strategy_id: String,
}

#[derive(serde::Deserialize)]
pub struct PruneQuery {
    pub strategy_id: String,
    /// Required cutoff — runs created strictly before this are deleted.
    pub before: DateTime<Utc>,
}

#[derive(serde::Deserialize)]
pub struct ResultsQuery {
    pub strategy_id: String,
    /// Zero-based page index (default 0).
    #[serde(default)]
    pub page: Option<i64>,
    /// Rows per page, clamped to 1–1000 (default 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Multi-key sort: an ordered, comma-joined `col:dir` list (index 0 = primary),
    /// e.g. `sort=n_fired:desc,win_rate:desc`. Each column is a frontend key
    /// (`"score"`, `"n_fired"`, `"p_exit_take_profit"`, …). This is the current
    /// wire format; `sort_col`/`sort_dir` below are the legacy single-key fallback.
    #[serde(default)]
    pub sort: Option<String>,
    /// Legacy single sort column. Omit to keep the default `score DESC`.
    #[serde(default)]
    pub sort_col: Option<String>,
    /// Legacy `"asc"` or `"desc"` (default `"desc"`).
    #[serde(default)]
    pub sort_dir: Option<String>,
    /// Per-column filters as a URL-encoded JSON object: `{"<col>": {op, val|min/max}}`,
    /// where `<col>` is a frontend column key (same vocabulary as `sort`). Every
    /// combo column is numeric, so operands are numbers and the ops are the numeric
    /// set (`gt`/`gte`/`lt`/`lte`/`eq`/`neq`/`between`); non-numeric ops and
    /// unresolvable columns are dropped, exactly like an unknown sort key. Applied to
    /// both the page query and the `X-Total-Count` count so the pager stays correct.
    #[serde(default)]
    pub filters: Option<String>,
}

/// One column filter's wire shape (mirrors the frontend `FilterSpec`). Operands are
/// `f64` because every filterable combo column is numeric; a `contains`/`in` op or a
/// missing operand fails to resolve and the filter is dropped.
#[derive(serde::Deserialize)]
struct FilterWire {
    op: String,
    #[serde(default)]
    val: Option<f64>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
}

/// A validated, injection-safe numeric predicate over one allowlisted column
/// expression. `expr` always comes from [`resolve_result_expr`]; operands are
/// bound as parameters (never interpolated), so this carries the same safety as
/// the sort allowlist.
enum FilterPred {
    /// `(expr) <sql_op> $n` — one bound operand.
    Cmp { expr: String, sql_op: &'static str, val: f64 },
    /// `(expr) BETWEEN $n AND $n+1` — two bound operands.
    Between { expr: String, min: f64, max: f64 },
}

/// Resolve a frontend column key to a table-qualified, injection-safe **numeric**
/// SQL expression over the results row (`r`) / per-run combos row (`c`). This is the
/// single allowlist shared by sort ([`order_by_from`]) and filter
/// ([`build_filter_preds`]) — a column is sortable **iff** it is filterable, so the
/// two can never drift. Every branch returns a fixed expression built only from
/// static SQL plus an `[a-z0-9_]`-validated param key, so interpolating the result
/// is safe.
///
/// Columns whose *displayed* value is scaled (win% / open% render `×100`) fold that
/// scale into the expression, so the operand the frontend sends (in displayed units)
/// compares directly and sorts identically (monotonic). `buy_amount_sol` supplies the
/// run notional the derived `mtm_pnl_pct` needs; `None` (or a non-positive value)
/// leaves that one column unresolvable.
fn resolve_result_expr(col: &str, buy_amount_sol: Option<f64>) -> Option<String> {
    // Param column: frontend prefix `p_`, DB expression `(params->>'key')::numeric`.
    if let Some(param_key) = col.strip_prefix("p_") {
        // Only allow [a-z0-9_]+ to keep the interpolation injection-safe.
        if !param_key.is_empty()
            && param_key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Some(format!("(c.params->>'{}')::numeric", param_key));
        }
        return None;
    }
    // Derived columns with no stored column of their own.
    match col {
        // Mark-to-market total (realized + unrealized).
        "pnl_mtm_sol" => return Some("(r.total_pnl_sol + r.open_pnl_sol)".to_string()),
        // Share of fired positions still open, in percent points to match the cell.
        // `NULLIF` guards n_fired = 0 (→ NULL, parked last / excluded by any compare).
        "open_share" => {
            return Some("(r.n_open::numeric / NULLIF(r.n_fired, 0) * 100)".to_string())
        }
        // Per-trade MTM % = (realized + unrealized) / (buy × fired) × 100. Mirrors the
        // frontend `mtmPctOf`; needs the run notional, so it only resolves when known.
        "mtm_pnl_pct" => {
            let buy = buy_amount_sol?;
            if !buy.is_finite() || buy <= 0.0 {
                return None;
            }
            // `buy` is a trusted f64 from our own runs table; Rust's f64 `Display`
            // never emits scientific notation, so it is a plain SQL decimal literal.
            return Some(format!(
                "((r.total_pnl_sol + r.open_pnl_sol) / NULLIF({} * r.n_fired, 0) * 100)",
                buy
            ));
        }
        _ => {}
    }
    // Win% renders `×100` off a 0..1 fraction — fold the scale in (monotonic for sort).
    if col == "win_rate" {
        return Some("(r.win_rate * 100)".to_string());
    }
    let db_col: &'static str = match col {
        "score"               => "score",
        "n_fired"             => "n_fired",
        "n_closed"            => "n_closed",
        "n_open"              => "n_open",
        "total_pnl_sol"       => "total_pnl_sol",
        "open_pnl_sol"        => "open_pnl_sol",
        "mean_pnl_pct"        => "mean_pnl_pct",
        "median_pnl_pct"      => "median_pnl_pct",
        "p90_pnl_pct"         => "p90_pnl_pct",
        "best_pnl_pct"        => "best_pnl_pct",
        "worst_pnl_pct"       => "worst_pnl_pct",
        "std_pnl_pct"         => "std_pnl_pct",
        "profit_factor"       => "profit_factor",
        "expectancy_sol"      => "expectancy_sol",
        "avg_holding_secs"    => "avg_holding_secs",
        "median_holding_secs" => "median_holding_secs",
        "n_exit_take_profit"  => "n_exit_take_profit",
        "n_exit_stop_loss"    => "n_exit_stop_loss",
        "n_exit_trailing"     => "n_exit_trailing",
        "n_exit_stall"        => "n_exit_stall",
        "n_exit_time"         => "n_exit_time",
        "n_exit_liquidity"    => "n_exit_liquidity",
        "n_exit_next_kill"    => "n_exit_next_kill",
        "n_exit_dead"         => "n_exit_dead",
        "n_exit_metrics"      => "n_exit_metrics",
        "n_exit_open"         => "n_exit_open",
        _                     => return None,
    };
    Some(format!("r.{}", db_col))
}

/// Build the full `ORDER BY` body from the frontend sort params. Prefers the
/// multi-key `sort=col:dir,…` list; falls back to the legacy single
/// `sort_col`/`sort_dir`. Unresolvable levels are dropped. A stable
/// `r.combo_id ASC` tiebreak is always appended so equal rows keep a
/// deterministic order across pages/refetches. Empty ⇒ caller's default.
fn build_order_by(query: &ResultsQuery, buy_amount_sol: Option<f64>) -> String {
    order_by_from(
        query.sort.as_deref(),
        query.sort_col.as_deref(),
        query.sort_dir.as_deref(),
        buy_amount_sol,
    )
}

/// Core of [`build_order_by`], split out so it's unit-testable without building
/// the full `ResultsQuery`.
fn order_by_from(
    sort: Option<&str>,
    sort_col: Option<&str>,
    sort_dir: Option<&str>,
    buy_amount_sol: Option<f64>,
) -> String {
    let dir_sql = |dir: &str| -> &'static str { if dir.trim() == "asc" { "ASC" } else { "DESC" } };
    let mut levels: Vec<String> = Vec::new();
    if let Some(spec) = sort.filter(|s| !s.is_empty()) {
        for entry in spec.split(',') {
            let (col, dir) = entry.split_once(':').unwrap_or((entry, "desc"));
            if let Some(expr) = resolve_result_expr(col.trim(), buy_amount_sol) {
                levels.push(format!("{} {} NULLS LAST", expr, dir_sql(dir)));
            }
        }
    } else if let Some(col) = sort_col {
        let dir = sort_dir.unwrap_or("desc");
        if let Some(expr) = resolve_result_expr(col, buy_amount_sol) {
            levels.push(format!("{} {} NULLS LAST", expr, dir_sql(dir)));
        }
    }
    if levels.is_empty() {
        return String::new();
    }
    levels.push("r.combo_id ASC".to_string());
    levels.join(", ")
}

/// Parse the URL-encoded `filters` JSON into validated numeric predicates. Each
/// entry's column resolves through the shared [`resolve_result_expr`] allowlist and
/// its op maps to a numeric SQL comparison; entries with an unresolvable column,
/// non-numeric op, or missing operand are dropped (mirrors the sort policy). Order
/// is stabilized by sorting on the column key so the emitted `$n` placeholders are
/// deterministic across the count and page queries.
fn build_filter_preds(filters_json: Option<&str>, buy_amount_sol: Option<f64>) -> Vec<FilterPred> {
    let Some(raw) = filters_json.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let map: std::collections::HashMap<String, FilterWire> = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<(String, FilterWire)> = map.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut preds = Vec::new();
    for (col, f) in entries {
        let Some(expr) = resolve_result_expr(&col, buy_amount_sol) else {
            continue;
        };
        let sql_op: Option<&'static str> = match f.op.as_str() {
            "gt" => Some(">"),
            "gte" => Some(">="),
            "lt" => Some("<"),
            "lte" => Some("<="),
            "eq" => Some("="),
            "neq" => Some("<>"),
            "between" => {
                if let (Some(min), Some(max)) = (f.min, f.max) {
                    preds.push(FilterPred::Between { expr, min, max });
                }
                continue;
            }
            // `contains`/`in` have no numeric meaning for these columns — drop them.
            _ => None,
        };
        if let (Some(sql_op), Some(val)) = (sql_op, f.val) {
            preds.push(FilterPred::Cmp { expr, sql_op, val });
        }
    }
    preds
}

/// Render the predicates into a `WHERE`-appendable ` AND (…) AND (…)` fragment plus
/// the operands to bind, numbered from `$start`. Returns an empty fragment (and no
/// binds) when there are no predicates. The caller binds `binds` in order, right
/// after the fixed `run_id`/`group_id` params.
fn render_filter_sql(preds: &[FilterPred], start: usize) -> (String, Vec<f64>) {
    let mut idx = start;
    let mut frags: Vec<String> = Vec::new();
    let mut binds: Vec<f64> = Vec::new();
    for p in preds {
        match p {
            FilterPred::Cmp { expr, sql_op, val } => {
                frags.push(format!("({}) {} ${}", expr, sql_op, idx));
                binds.push(*val);
                idx += 1;
            }
            FilterPred::Between { expr, min, max } => {
                frags.push(format!("({}) BETWEEN ${} AND ${}", expr, idx, idx + 1));
                binds.push(*min);
                binds.push(*max);
                idx += 2;
            }
        }
    }
    let sql = if frags.is_empty() {
        String::new()
    } else {
        format!(" AND {}", frags.join(" AND "))
    };
    (sql, binds)
}

// ---------------------------------------------------------------------------
// POST /api/strategies/sweeps — start a grouped DB-range sweep
// ---------------------------------------------------------------------------

pub async fn start_grouped_sweep(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<StartGroupedSweepBody>,
) -> impl Responder {
    let b = body.into_inner();

    // Resolve the per-strategy table set (also validates the strategy id).
    let tables = match registry::tables_for(&b.strategy_id) {
        Some(t) => t,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!(
                    "unknown strategy_id '{}' (supported: {:?})",
                    b.strategy_id,
                    registry::strategy_ids()
                )
            }))
        }
    };

    // Mutual exclusion with flow-discovery (shared Duck/RAM).
    if state.discovery_running.load(Ordering::Acquire) {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "a flow-discovery job is already running — wait or cancel it before sweeping"
        }));
    }

    // Single-flight: claim the shared sweep gate or reject. Claimed synchronously
    // here so a concurrent request gets its 409 immediately; the spawned job owns
    // the matching release (`run_grouped_sweep_job`'s `Gate`).
    if state
        .sweep_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return HttpResponse::Conflict()
            .json(serde_json::json!({"error": "a sweep is already running"}));
    }

    // Apply this run's desktop RAM reserve now that the gate is ours — every
    // admission/shard/fold ceiling below reads it, and single-flight guarantees
    // no other run is sizing against a different value.
    let reserve_mb = crate::sweep::registry::set_ram_reserve_mb(b.ram_reserve_mb);
    tracing::info!(ram_reserve_mb = reserve_mb, "grouped sweep: desktop RAM reserve for this run");

    // Apply this run's AVX-512 exit-scan choice under the same single-flight gate:
    // honor `use_avx512` only when the host actually has AVX-512, else force scalar
    // and toast the operator (rides the existing `sweep_notice` SSE → info toast).
    // Process-global + set here (not persisted) exactly like the RAM reserve above.
    let want_simd = b.use_avx512.unwrap_or(false);
    let simd_on = want_simd && crate::sweep::registry::avx512_available();
    crate::sweep::registry::set_use_simd(Some(simd_on));
    tracing::info!(
        use_avx512 = simd_on,
        requested = want_simd,
        "grouped sweep: AVX-512 exit scan for this run"
    );
    if want_simd && !simd_on {
        let _ = state.sse_tx.send(crate::models::ingest::SseEvent::SweepNotice {
            strategy_id: b.strategy_id.clone(),
            message: "AVX-512 unavailable on this host — running the scalar exit scan.".to_string(),
        });
    }
    // Reset last run's resident baseline: it is only valid once *this* run's corpus
    // is resident (set at `corpus_loaded` below). A run that bails between here and
    // there must not inherit the prior run's corpus footprint as its own.
    crate::sweep::registry::set_resident_baseline_bytes(None);

    // Detach the run so a client disconnect (browser refresh / SPA navigation)
    // can't drop the request future mid-sweep, AND so this POST returns as soon as
    // the run is admitted (run header persisted) instead of being held open for the
    // whole multi-minute sweep. Holding it open kept one of the browser's ~6
    // per-host HTTP/1.1 connections busy for the entire run, which could leave a
    // concurrent `POST .../sweeps/cancel` queued in the browser until the sweep
    // ended — so the cancel button appeared to do nothing. The job reports its
    // early result (admitted + `run_id`, or a pre-fold validation error) through
    // `early_tx`; the fold then runs detached, recoverable via `GET /api/jobs/status`
    // and SSE. `rt::spawn` keeps it on the worker (no `Send` bound, unlike
    // `tokio::spawn`); we drop the handle — dropping an `rt::spawn` task never
    // cancels it.
    let state = state.get_ref().clone();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_grouped_sweep_job(state, b, tables, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": "sweep task ended before reporting status"}))
    })
}

/// The detached body of a grouped sweep, spawned by [`start_grouped_sweep`] so
/// the work survives a client disconnect (the recovery contract behind
/// `/api/jobs/status`). Owns the single-flight release + progress reset + terminal
/// `SweepFinished` (via `Gate`), loads the corpus, runs the engine, and persists
/// run → groups → results. Assumes `sweep_running` was already claimed by the
/// caller.
///
/// Reports its *early* result through `early_tx` — a pre-fold validation error
/// (bad range, no tokens, …) or the admitted run's `run_id` once the run header is
/// persisted — then runs the (multi-minute) fold detached. The caller returns that
/// early result immediately so the POST connection is freed before the fold,
/// keeping a concurrent cancel from queueing behind it (see [`start_grouped_sweep`]).
/// Post-admission outcomes (done / cancel / error) are surfaced via the DB run
/// status + the `SweepFinished` SSE frame, not this function's (now `()`) return.
async fn run_grouped_sweep_job(
    state: Arc<LocalState>,
    b: StartGroupedSweepBody,
    tables: GroupedSweepTables,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
    // Releases the single-flight gate, resets the progress snapshot, and
    // broadcasts the terminal `SweepFinished` frame on EVERY exit path (done /
    // cancel / config-error / db-error) — putting the emit in Drop means no
    // early-return can forget it, so a global progress indicator always clears.
    struct Gate {
        running: Arc<std::sync::atomic::AtomicBool>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        progress: Arc<crate::state::job_progress::ProgressCell>,
        sse_tx: tokio::sync::broadcast::Sender<crate::models::ingest::SseEvent>,
        strategy_id: String,
        // Set by the post-admission error path before it returns; the run row is
        // deleted there, so carrying the reason on the terminal frame is the
        // client's only signal (else a refused run reads as a frozen page).
        error: Option<String>,
    }
    impl Drop for Gate {
        fn drop(&mut self) {
            self.progress.reset();
            self.running.store(false, Ordering::Release);
            let _ = self.sse_tx.send(crate::models::ingest::SseEvent::SweepFinished {
                strategy_id: self.strategy_id.clone(),
                cancelled: self.cancel.load(Ordering::Acquire),
                error: self.error.take(),
            });
        }
    }
    let mut gate = Gate {
        running: state.sweep_running.clone(),
        cancel: state.sweep_cancel.clone(),
        progress: state.sweep_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        strategy_id: b.strategy_id.clone(),
        error: None,
    };

    // Clear any stale cancel request + progress snapshot from a prior run before
    // this one starts.
    state.sweep_cancel.store(false, Ordering::Release);
    state.sweep_progress.reset();

    // Phase 0.3 — start the RSS/wall-clock trace at admission so every milestone
    // (corpus loaded, done) is measured from the same origin against the baseline.
    let clock = crate::sweep::obs::SweepClock::start();
    crate::sweep::obs::log_milestone(&clock, "admitted");

    let token_cap = registry::clamp_token_cap(b.token_cap);
    let min_tokens = b.min_tokens.max(1);
    // Per-run bucket width for the continuous SOL grouping fields. Mirror the rule
    // validator (`strategies::rules::check_bucket_width`): a non-finite or sub-floor
    // width falls back to the default rather than 400-ing, so a stray input can't
    // block a run. Floored at MIN_BUCKET_WIDTH_SOL to keep the 1e-9 ratio-epsilon
    // valid and edge labels from exploding in decimals.
    let bucket_width = {
        let w = b.bucket_width_sol;
        if w.is_finite() && w >= crate::sweep::grouping::MIN_BUCKET_WIDTH_SOL {
            w
        } else {
            crate::sweep::grouping::SOL_BUCKET_WIDTH
        }
    };
    let floor = CoverageFloor {
        min_fired_abs: b.min_fired_abs,
        fire_frac: b.fire_frac.clamp(0.0, 1.0),
    };
    // `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. A `refine` form runs a coarse
    // LHS pass then a per-group neighborhood refine (see `parse_method`).
    let (method, refine) = b
        .method
        .as_deref()
        .map(parse_method)
        .unwrap_or((crate::sweep::strategy::SweepMethod::Grid, None));
    // Stored run tag: the refine pass reports as its own method, else the sampler.
    let method_tag = if refine.is_some() { "refine".to_string() } else { method.tag().to_string() };

    // Peek axes for flow groups before load so the lake projects ix_labels/wallet.
    let with_flow = axes_json_references_flow(&b.axes);
    if with_flow && b.volume_ix_patterns.is_none() {
        let msg = "axes reference m_flow_split/m_flow_split_window but volume_ix_patterns is missing";
        gate.error = Some(msg.into());
        let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({ "error": msg })));
        return;
    }
    let sel = Selection {
        mints: None,
        token_cap,
        created_after: b.created_after,
        created_before: b.created_before,
        // Uncapped by default (full history) — analysis prices over every trade, so
        // the sweep matches single-rule simulate for high-volume tokens. Set
        // `SWEEP_PER_MINT_CAP` (≥1) only to trade fidelity for a lighter/faster run.
        per_mint_cap: crate::sweep::corpus::sweep_per_mint_cap(),
        // Moot when uncapped (whole history loads); still keeps the launch prefix if a
        // `SWEEP_PER_MINT_CAP` override is set, which the entry logic decides on (Rec 4).
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: b.curve_only,
        // Sweep resolves the trigger by index, not signature — no `tx_signature` needed.
        with_signatures: false,
        with_flow,
    };

    // Load the corpus from the immutable Parquet lake via DuckDB (the sole sweep
    // corpus source). The lake embeds grouping fingerprints (`has_fingerprints`), so
    // no separate `tokens` lookup is needed; trades arrive as slim, wallet-interned
    // `CorpusTrade` buffers in the same launch-window order the engine expects.
    let root = crate::lake::lake_root();
    let src = LakeSource::new(root.clone());

    // Reuse the in-memory corpus cache when the last sweep loaded the **same
    // selection against the same lake version** (the hash folds both). This skips a
    // full DuckDB Parquet re-read on the common tune-the-rule loop — iterating
    // strategy params/filters over one corpus. A fresh `lake-export` moves the lake
    // version → hash miss → reload, so a stale corpus can never be served. The cache
    // stores the **unfiltered** selection corpus, so this run's own field/label
    // filters below re-apply either way.
    let expected_hash = src.selection_hash(&sel);
    let cached = {
        let cache = state.sweep_corpus_cache.read().await;
        cache.as_ref().filter(|c| c.corpus_hash == expected_hash).map(|c| {
            crate::sweep::corpus::Corpus {
                tokens: (*c.tokens).clone(),
                hash: c.corpus_hash.clone(),
                has_fingerprints: true,
                candidates_capped: c.candidates_capped,
            }
        })
    };
    let mut corpus = if let Some(c) = cached {
        tracing::info!(lake = %root.display(),
            "grouped sweep: corpus cache hit (selection + lake-version match) — skipping DuckDB load");
        c
    } else {
        tracing::info!(lake = %root.display(), "grouped sweep: corpus source = Parquet lake (DuckDB)");
        // P0 visibility — the DuckDB lake read is the longest silent pre-fold
        // phase (no engine frames until partitioning). Announce it with an
        // indeterminate frame (`total: 0`) so the dashboard shows a labeled
        // "Loading corpus" bar instead of an unexplained spinner; the next
        // phase's first frame marks it done client-side. Cache hits skip this
        // (the load is instant, a flashed phase would just be noise).
        let _ = state.sse_tx.send(crate::models::ingest::SseEvent::SweepProgress {
            strategy_id: b.strategy_id.clone(),
            phase: "corpus".to_string(),
            processed: 0,
            total: 0,
        });
        let _stage = crate::sweep::obs::Stage::start("corpus_load");
        match src.load(&sel).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("grouped sweep: lake corpus load failed: {e}");
                let _ = early_tx.send(HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": e.to_string()})));
                return;
            }
        }
    };
    if corpus.tokens.is_empty() {
        let _ = early_tx.send(HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "no tokens in that date range — widen the selection"})));
        return;
    }

    // `token_cap` is a `LIMIT` on a `created_at DESC` scan (see `lake::duck`), so a
    // window holding more tokens than the cap is trimmed to its **newest** slice.
    // Flag the **candidate** select (not `corpus.len()`): `curve_only` / empty
    // histories can leave the loaded corpus below the cap even when older mints
    // were dropped (parity plan B10). Simulate has no such cap.
    if corpus.candidates_capped {
        let msg = format!(
            "Corpus hit the {token_cap}-token cap — only the newest {token_cap} tokens in \
             this date range were swept ({} kept after trade filters). Narrow the range \
             or raise token_cap (max {}) to cover it all.",
            corpus.tokens.len(),
            registry::MAX_TOKEN_CAP,
        );
        tracing::warn!(
            token_cap,
            loaded = corpus.tokens.len(),
            "grouped sweep: corpus truncated by token_cap"
        );
        let _ = state.sse_tx.send(crate::models::ingest::SseEvent::SweepNotice {
            strategy_id: b.strategy_id.clone(),
            message: msg,
        });
    }

    // A cancel requested during the corpus load lands here. The lake read is a
    // synchronous DuckDB query (not itself mid-flight abortable), but this is the
    // longest pre-fold phase — and for swing1 by far: it has no token-creation gate,
    // so its corpus is the WHOLE window (the fingerprint/field filters below run
    // in-memory AFTER the load), where tpsl1/2 pre-prune to a fingerprint. Without
    // this poll a cancel clicked during that load looked completely dead: the engine
    // only starts polling once the fold begins. Bail the instant the load returns so
    // the user's cancel takes effect instead of folding a corpus they abandoned.
    // Pre-admission (no run row / `run_id` yet), so nothing to mark cancelled — the
    // `Gate` still releases the single-flight lock and broadcasts the terminal
    // `SweepFinished { cancelled: true }` frame that clears the global indicator.
    if state.sweep_cancel.load(Ordering::Acquire) {
        tracing::info!("grouped sweep: cancelled during corpus load (pre-admission)");
        let _ = early_tx.send(HttpResponse::Ok()
            .json(serde_json::json!({ "run_id": null, "status": "cancelled" })));
        return;
    }

    // Option A: stash the **unfiltered** selection corpus (trades + fingerprints) in
    // the in-memory cache so `list_token_results` calls right after the sweep skip
    // all I/O. This MUST happen BEFORE the in-memory ix_labels/field filters below:
    // the cache is keyed only by `corpus_hash` (selection mints + trade counts), which
    // is identical for two runs over the same window/caps regardless of their filters.
    // If we cached the post-filter subset, a later run with the same selection but a
    // different (or no) filter would hit this cache and `apply_filters` could not
    // recover the tokens this run's filter dropped — yielding an empty token-results
    // table. Caching unfiltered mirrors Option B (the Parquet corpus is also
    // unfiltered), so `apply_filters` re-applies *this* run's own filters either way.
    // Clone is cheap — trades/wallets are `Arc`, fp is a small struct.
    {
        let cached_tokens = Arc::new(corpus.tokens.clone());
        let corpus_hash = corpus.hash.clone();
        let candidates_capped = corpus.candidates_capped;
        let mut cache = state.sweep_corpus_cache.write().await;
        *cache = Some(SweepCorpusCache {
            corpus_hash,
            tokens: cached_tokens,
            candidates_capped,
        });
    }

    // Saved-fingerprint scope (mutually exclusive with the manual filters below —
    // same contract as flow discovery). Matching goes through the ENGINE's
    // `fingerprint::matches` SSOT so the swept slice is exactly the token set the
    // live entry gate would arm on: exact axes stay exact, the continuous SOL axes
    // match by bucket. The manual `field_filters` cannot express that (they compare
    // exact values), which is why this is a separate path rather than a prefill.
    let mut scope_fp: Option<Fingerprint> = None;
    if let Some(fp_id) = b.fingerprint_id {
        let fp_repo = FingerprintRepo::new(state.db.clone());
        let fp = match fp_repo.find(fp_id).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("fingerprint {fp_id} not found")
                })));
                return;
            }
            Err(e) => {
                tracing::error!("grouped sweep: fingerprint lookup failed: {e}");
                let _ = early_tx.send(HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": "database error" })));
                return;
            }
        };
        let engine_fp = trading_core::strategies::fingerprint_axes::fp_to_engine(&fp);
        let before = corpus.tokens.len();
        corpus
            .tokens
            .retain(|t| hunter_engine::fingerprint::matches(&engine_fp, &t.fp));
        tracing::info!(
            fingerprint = %fp.name,
            kept = corpus.tokens.len(),
            dropped = before - corpus.tokens.len(),
            "grouped sweep: saved-fingerprint scope applied"
        );
        if corpus.tokens.is_empty() {
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!(
                    "no tokens in that window match fingerprint “{}” — widen the date range or token cap",
                    fp.name
                )
            })));
            return;
        }
        scope_fp = Some(fp);
    }

    // Manual corpus filters — the `else` arm of the saved-fingerprint scope above.
    // Skipped entirely on a scoped run so a stale filter left in the form can't
    // narrow (or contradict) what the engine matched.
    if scope_fp.is_none() {
        // Optional exact-set ix_labels filter (applied post-fingerprint, in-memory so
        // the unfiltered Parquet corpus cache is reused across filter values): keep only
        // tokens whose label set equals the requested set. Normalize the request set the
        // same way the fingerprint is, so the `==` is order/dup-insensitive. An empty
        // request set is "no filter" (not "tokens with no labels").
        if let Some(want) = b
            .ix_labels_filter
            .as_ref()
            .filter(|f| !f.is_empty())
            .map(|f| crate::sweep::grouping::normalize_label_vec(f.clone()))
        {
            let before = corpus.tokens.len();
            corpus.tokens.retain(|t| t.fp.ix_labels == want);
            tracing::info!(
                kept = corpus.tokens.len(),
                dropped = before - corpus.tokens.len(),
                labels = ?want,
                "grouped sweep: ix_labels exact-set filter applied"
            );
            if corpus.tokens.is_empty() {
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                    "error": "no tokens match that instruction-label filter — adjust the labels or widen the selection"
                })));
                return;
            }
        }
        // Optional per-field numeric filters — restrict the corpus to tokens whose
        // fingerprint value for the named field is in the allowed set. Applied
        // post-fingerprint, in-memory, reusing the unfiltered Parquet cache.
        // Unknown keys and `ix_labels` (handled above) are skipped with a warning.
        if let Some(filters) = &b.field_filters {
            use crate::sweep::grouping::GroupField;
            for (field_str, allowed) in filters {
                if allowed.is_empty() {
                    continue;
                }
                let field: GroupField = match serde_json::from_str(&format!("\"{field_str}\"")) {
                    Ok(f) => f,
                    Err(_) => {
                        tracing::warn!(key = %field_str, "grouped sweep: unknown field_filters key, skipping");
                        continue;
                    }
                };
                if field == GroupField::IxLabels {
                    tracing::warn!("grouped sweep: ix_labels in field_filters — use ix_labels_filter instead, skipping");
                    continue;
                }
                let before = corpus.tokens.len();
                corpus.tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
                tracing::info!(
                    field = %field_str,
                    kept = corpus.tokens.len(),
                    dropped = before - corpus.tokens.len(),
                    "grouped sweep: field filter applied"
                );
                if corpus.tokens.is_empty() {
                    let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                        "error": format!(
                            "no tokens match the '{field_str}' field filter — adjust the values or widen the selection"
                        )
                    })));
                    return;
                }
            }
        }
    }

    // Corpus is fully resident here — the Phase 0 baseline's first peak (every
    // streaming phase is judged against this RSS/seconds reading). It is also the
    // run's permanent resident floor: capture it so the mid-run headroom read
    // (`usable_host_bytes`) treats the corpus as consumed but the fold buffers it is
    // about to allocate as reusable, instead of starving onto the slow fold path as
    // its own RSS grows. Captured once, here, because no fold buffer exists yet.
    crate::sweep::registry::set_resident_baseline_bytes(crate::sweep::obs::process_rss_bytes());
    tracing::info!(
        tokens = corpus.token_count(),
        trades = corpus.trade_count(),
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        milestone = "corpus_loaded",
        "sweep: milestone"
    );

    let corpus_hash = corpus.hash.clone();
    let token_count = corpus.token_count() as i32;
    let grouping_spec = serde_json::to_value(&b.group_by).unwrap_or_else(|_| serde_json::json!([]));

    // Phase 4 — write the run header up front (`status = "running"`) BEFORE the
    // sweep, so the per-group `append_group` writes (FK → this row) can land
    // incrementally and a cancel/crash keeps whatever finished. `group_count` /
    // `combo_count` are placeholders (0) here, set by the engine's `begin` once
    // the surviving + combo sets are known; `axes_spec` is the raw request axes
    // until `finalize_run` swaps in the resolved set. Counts/status/axes are
    // re-stamped at the terminal step.
    let run_id = Uuid::new_v4();
    let run = GroupedSweepRun {
        id: run_id,
        strategy_id: b.strategy_id.clone(),
        source: "db".to_string(),
        method: method_tag,
        created_after: b.created_after,
        created_before: b.created_before,
        curve_only: b.curve_only,
        grouping_spec,
        axes_spec: b.axes.clone(),
        min_tokens: min_tokens as i32,
        token_count,
        group_count: 0,
        combo_count: 0,
        corpus_hash: Some(corpus_hash),
        created_at: Utc::now(),
        status: "running".to_string(),
        groups_done: 0,
        // Persist the corpus filters + **effective** cap knobs (post-clamp), so the
        // history panel / re-run restore what actually ran — not a fat-fingered
        // request that the server silently ceilings. Only store an active
        // (non-empty) ix_labels filter; an empty/omitted one means "no filter"
        // and reads back as NULL.
        // A fingerprint-scoped run stores the fingerprint, NOT the manual filters —
        // they were ignored above, so persisting them would misreport the scope and
        // (worse) re-apply on the token-results reload.
        ix_labels_filter: b
            .ix_labels_filter
            .as_ref()
            .filter(|_| scope_fp.is_none())
            .filter(|f| !f.is_empty())
            .and_then(|f| serde_json::to_value(f).ok()),
        field_filters: b
            .field_filters
            .as_ref()
            .filter(|_| scope_fp.is_none())
            .filter(|f| !f.is_empty())
            .and_then(|f| serde_json::to_value(f).ok()),
        fingerprint_id: scope_fp.as_ref().map(|f| f.id),
        token_cap: Some(token_cap as i32),
        max_combos: b.max_combos.map(|v| v as i32),
        label: None,
        buy_amount_sol: Some(b.buy_amount_sol),
        // The clamped width actually used to partition (not the raw request), so
        // re-run + promotion restore exactly what this run swept.
        bucket_width_sol: Some(bucket_width),
        volume_ix_patterns: b
            .volume_ix_patterns
            .as_ref()
            .and_then(|p| serde_json::to_value(p).ok()),
        // The run's pricing identity. Persisted (not just applied) because a PnL
        // column without them is unreadable: two runs under different models are not
        // comparable, and the drill-in re-simulates against these to reproduce the
        // very row it drills into.
        fill_model: serde_json::to_value(b.fill_model)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string)),
        cost_model: serde_json::to_value(b.cost_model)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string)),
    };
    let repo = GroupedSweepRepo::new(state.batch_db.clone(), tables);
    if let Err(e) = repo.insert_run(&run).await {
        tracing::error!("grouped sweep: insert_run failed: {e}");
        let _ = early_tx.send(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()})));
        return;
    }

    // The run is admitted and its header is persisted — report "started" so the
    // POST returns NOW and the fold below runs detached (see `start_grouped_sweep`
    // for why holding the connection open blocked cancels). `run_id` lets the
    // client jump straight to the run, which fills in live via the per-group writes
    // + SSE progress frames; the terminal state arrives over the `SweepFinished`
    // SSE frame. A pre-fold config error (bad axes / over-cap grid) below now drops
    // this admitted run instead of returning a 400 — a rare trade for never holding
    // the connection across the fold.
    let _ = early_tx.send(HttpResponse::Accepted().json(serde_json::json!({
        "run_id": run_id,
        "status": "started",
    })));

    // Single DB-writer task: the engine's per-group emits (which arrive out of
    // order from the cross-group small-group phase) are serialized through one
    // unbounded channel into one task, so concurrent folds never race the
    // connection. It drains until the sink (and thus the sender) drops when the
    // sweep ends, then returns the persisted-group tally + the first write
    // error (if any) so the terminal status can be honest about missing groups.
    // Unbounded so the sync engine callback never blocks a rayon worker.
    //
    // P0 visibility — each committed group emits a `SweepGroupDone` frame (its
    // rows are readable NOW), so the frontend streams groups into the table
    // mid-run and renders a "persisted N/M" counter. Persistence is a concurrent
    // drain, not a stage, so it is deliberately NOT a progress *phase* (the old
    // `"saving"` phase interleaved with `"sweep"` frames and made the phase bars
    // flip-flop between active/done for the whole run).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SweepWrite>();
    let writer = {
        let db = state.batch_db.clone();
        let writer_sse_tx = state.sse_tx.clone();
        let writer_strategy_id = b.strategy_id.clone();
        actix_web::rt::spawn(async move {
            let repo = GroupedSweepRepo::new(db, tables);
            let mut groups_done = 0u64;
            let mut total_groups = 0u64;
            let mut first_error: Option<String> = None;
            let mut groups_failed = 0u64;
            while let Some(msg) = rx.recv().await {
                match msg {
                    SweepWrite::Begin {
                        group_count,
                        combo_count,
                    } => {
                        total_groups = group_count as u64;
                        if let Err(e) = repo
                            .update_run_counts(run_id, group_count, combo_count)
                            .await
                        {
                            tracing::error!("grouped sweep: update_run_counts failed: {e}");
                            first_error.get_or_insert_with(|| format!("update_run_counts: {e}"));
                        }
                        // Announce the surviving group count up front so the
                        // frontend can show "persisted 0/N" before the first
                        // group commits.
                        let _ = writer_sse_tx.send(crate::models::ingest::SseEvent::SweepGroupDone {
                            strategy_id: writer_strategy_id.clone(),
                            run_id,
                            group_index: None,
                            groups_done: 0,
                            group_count: total_groups,
                        });
                    }
                    SweepWrite::Combos(rows) => {
                        if let Err(e) = repo.insert_combos_indexed(run_id, &rows).await {
                            tracing::error!("grouped sweep: insert_combos_indexed failed: {e}");
                            first_error.get_or_insert_with(|| format!("insert_combos_indexed: {e}"));
                        }
                    }
                    SweepWrite::Group(g) => match repo.append_group(run_id, &g).await {
                        Ok(()) => {
                            groups_done += 1;
                            let _ = writer_sse_tx.send(crate::models::ingest::SseEvent::SweepGroupDone {
                                strategy_id: writer_strategy_id.clone(),
                                run_id,
                                group_index: Some(g.group_index as u64),
                                groups_done,
                                group_count: total_groups,
                            });
                        }
                        Err(e) => {
                            tracing::error!("grouped sweep: append_group failed: {e}");
                            first_error.get_or_insert_with(|| format!("append_group: {e}"));
                        }
                    },
                    // Nothing to persist — just make the shortfall visible so the run
                    // finalizes `partial` with a reason. The engine already logged and
                    // pushed an operator notice.
                    SweepWrite::GroupFailed { group_index, err } => {
                        groups_failed += 1;
                        first_error
                            .get_or_insert_with(|| format!("group {group_index} failed: {err}"));
                    }
                }
            }
            (groups_done, first_error, groups_failed)
        })
    };

    // Two phase-tagged observers: coarse_observer reports the coarse LHS pass
    // (only active for `refine` runs); observer reports the final sweep pass.
    // Both share the cancel flag; only `observer` writes the ProgressCell so
    // `/api/jobs/status` recovery shows the sweep-phase bar (the most useful one).
    let coarse_observer: Arc<dyn crate::sweep::progress::SweepObserver + Send> =
        Arc::new(crate::sweep::progress::SweepProgress::new(
            state.sse_tx.clone(),
            b.strategy_id.clone(),
            state.sweep_cancel.clone(),
            Arc::new(crate::state::job_progress::ProgressCell::default()),
            "coarse",
        ));
    let observer: Arc<dyn crate::sweep::progress::SweepObserver + Send> =
        Arc::new(crate::sweep::progress::SweepProgress::new(
            state.sse_tx.clone(),
            b.strategy_id.clone(),
            state.sweep_cancel.clone(),
            state.sweep_progress.clone(),
            "sweep",
        ));
    // Option C: build group-key → mints map from the fully-filtered corpus so
    // group_done can attach mint lists to each write unit without re-scanning.
    let group_mints_map = {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for t in &corpus.tokens {
            // MUST use the same `bucket_width` the engine partitions by, or these
            // group-key strings won't match the engine's group keys and mints go missing.
            let key_str = group_key(&t.fp, &b.group_by, bucket_width).to_json().to_string();
            map.entry(key_str).or_default().push(t.mint.clone());
        }
        Arc::new(map)
    };

    // The sink hands each fully-folded group to the writer task. `run_grouped`
    // takes ownership, so it (and its sender) drop when the sweep ends — closing
    // the channel and letting the writer task finish.
    let sink: Arc<dyn GroupSink + Send + Sync> =
        Arc::new(HandlerSink { tx, group_mints: group_mints_map });

    let result = registry::run_grouped(
        &b.strategy_id,
        b.axes.clone(),
        method,
        refine,
        corpus,
        b.group_by.clone(),
        bucket_width,
        min_tokens,
        floor,
        b.max_combos,
        crate::sweep::generic::Pricing {
            buy_amount_sol: b.buy_amount_sol,
            fill_model: b.fill_model,
            cost: b.cost_model.model(),
        },
        b.volume_ix_patterns.clone(),
        coarse_observer,
        observer,
        sink,
    )
    .await;

    // The sweep is done — the sink dropped, so the writer drains and returns how
    // many groups actually persisted (the partial count on cancel/error) plus
    // the first write error, if any.
    //
    // Timed: the write channel is unbounded (deliberately — a rayon worker must never
    // stall on the DB), so a writer that fell behind during the fold drains *here*,
    // serially, after the engine already logged "all groups folded". That gap was
    // unlabeled dead time at the end of every slow run.
    let (groups_done, write_error, groups_failed) = {
        let _stage = crate::sweep::obs::Stage::start("writer_drain");
        writer
            .await
            .unwrap_or((0, Some("sweep DB-writer task panicked".to_string()), 0))
    };

    let output = match result {
        Ok(o) => o,
        Err(e) => {
            // A cooperative cancel surfaces as an engine error. Keep whatever
            // groups already committed and mark the run cancelled (honestly
            // partial) instead of discarding them.
            if state.sweep_cancel.load(Ordering::Acquire) {
                tracing::info!(groups_done, "grouped sweep: cancelled by user (partial kept)");
                if let Err(e) = repo.mark_cancelled(run_id).await {
                    tracing::error!("grouped sweep: mark_cancelled failed: {e}");
                }
                // The client already holds `run_id` (admitted at start); the
                // partial run + its `cancelled` status reach it via the runs-list
                // refresh on the `SweepFinished` SSE frame.
                return;
            }
            // Keep the run row on every failure. Groups are streamed to the DB as
            // they fold, so a failure partway through leaves real, queryable
            // results the user already spent minutes producing — deleting the run
            // threw that away along with any diagnostic trace of the attempt.
            //
            // `partial` when some groups committed (same honest status a short
            // write run gets); `failed` when none did — a config error (bad axes /
            // over-cap grid) or a floor-overflow refusal that never folded
            // anything. The reason rides the terminal frame either way, so the
            // client can toast it while the run itself stays inspectable.
            tracing::error!(groups_done, "grouped sweep failed: {e}");
            gate.error = Some(e.to_string());
            let status = if groups_done > 0 { "partial" } else { "failed" };
            if let Err(mark) = repo.mark_status(run_id, status).await {
                tracing::error!("grouped sweep: marking failed run '{status}' failed: {mark}");
            }
            return;
        }
    };

    // Engine success — stamp the terminal status, authoritative counts, and the
    // resolved axes (only known now). `completed` only when every folded group
    // actually committed; a write error (or a writer shortfall) stamps `partial`
    // instead, and the reason rides the terminal `SweepFinished` frame so the
    // client surfaces it rather than trusting a silently-thinner run.
    // `folded` counts groups the engine actually produced — groups it skipped after a
    // fold failure are *not* in there, so `groups_failed` is what keeps a run that
    // dropped groups from reporting `completed` over the thinner set.
    let folded = output.groups.len() as u64;
    let status = if write_error.is_none() && groups_done == folded && groups_failed == 0 {
        "completed"
    } else {
        "partial"
    };
    if groups_failed > 0 {
        tracing::warn!(
            groups_failed,
            groups_done,
            "grouped sweep: finished with skipped groups — marking partial"
        );
    }
    // `write_error` now carries the first problem of *either* kind (a failed DB write or
    // a skipped group), so the reason is phrased from whichever actually happened —
    // labelling a fold failure "write error" would send the user to the wrong subsystem.
    if let Some(err) = write_error {
        gate.error = Some(if groups_failed > 0 {
            format!(
                "sweep skipped {groups_failed} group(s) that failed to fold, and persisted \
                 {groups_done}/{folded} of the rest — first problem: {err}"
            )
        } else {
            format!("sweep persisted {groups_done}/{folded} groups — first write error: {err}")
        });
    } else if groups_done != folded {
        gate.error = Some(format!("sweep persisted only {groups_done}/{folded} groups"));
    }
    if let Err(e) = repo
        .finalize_run(
            run_id,
            status,
            output.groups.len() as i32,
            output.combo_count as i32,
            &output.axes_json,
        )
        .await
    {
        tracing::error!("grouped sweep: finalize failed: {e}");
        return;
    }

    tracing::info!(
        run_id = %run_id,
        strategy = %run.strategy_id,
        status,
        tokens = run.token_count,
        groups = output.groups.len(),
        groups_persisted = groups_done,
        combos = output.combo_count,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        milestone = "done",
        "grouped sweep: done"
    );
}

/// Message to the single per-run DB-writer task. `Begin` carries the engine's
/// fixed group/combo counts (one per run); `Group` carries one completed group's
/// write unit. Boxed so the enum stays small despite the large group payload.
enum SweepWrite {
    Begin {
        group_count: i32,
        combo_count: i32,
    },
    /// Retained combo params for one group — inserted before the group rows so
    /// the results JOIN finds them. Never the full N-combo dictionary.
    Combos(Vec<(i32, serde_json::Value)>),
    Group(Box<GroupedSweepGroupWrite>),
    /// One group was skipped after failing to fold (the engine isolates per-group
    /// errors rather than aborting the run). Carries no rows — it exists so the
    /// terminal status is `partial` with a reason instead of a silent `completed`
    /// over a thinner group set.
    GroupFailed { group_index: usize, err: String },
}

/// The [`GroupSink`] bridge from the (sync, rayon-worker) engine to the (async)
/// DB-writer task: each emit is forwarded over the unbounded channel. Sends are
/// best-effort — a closed channel (writer already gone) just drops the emit,
/// which only happens on shutdown.
struct HandlerSink {
    tx: tokio::sync::mpsc::UnboundedSender<SweepWrite>,
    /// Option C: maps serialized group-key JSON → mints for that group.
    /// Built from the corpus before the sweep so the group_done callback can
    /// attach mints to each write unit without touching the corpus again.
    group_mints: Arc<std::collections::HashMap<String, Vec<String>>>,
}

impl GroupSink for HandlerSink {
    fn begin(&self, group_count: usize, combo_count: usize) {
        let _ = self.tx.send(SweepWrite::Begin {
            group_count: group_count as i32,
            combo_count: combo_count as i32,
        });
    }

    fn group_done(
        &self,
        group_index: usize,
        group: &GroupResult,
        retained_params: &[(u32, serde_json::Value)],
    ) {
        let key_str = group.key.to_json().to_string();
        let mints = self.group_mints.get(&key_str).cloned().unwrap_or_default();
        let write = group_to_write(group_index, group, retained_params, mints);
        // Insert retained combo rows before the group so JOINs resolve.
        let combo_rows: Vec<(i32, serde_json::Value)> = retained_params
            .iter()
            .map(|(id, v)| (*id as i32, v.clone()))
            .collect();
        if !combo_rows.is_empty() {
            let _ = self.tx.send(SweepWrite::Combos(combo_rows));
        }
        let _ = self.tx.send(SweepWrite::Group(Box::new(write)));
    }

    fn group_failed(&self, group_index: usize, err: &str) {
        let _ = self.tx.send(SweepWrite::GroupFailed {
            group_index,
            err: err.to_string(),
        });
    }
}

/// Flatten one fully-folded group into the repo's write unit. `retained_params`
/// is `(combo_id, params_json)` for the retention set only.
fn group_to_write(
    group_index: usize,
    g: &GroupResult,
    retained_params: &[(u32, serde_json::Value)],
    mints: Vec<String>,
) -> GroupedSweepGroupWrite {
    let param_at = |id: u32| -> serde_json::Value {
        retained_params
            .iter()
            .find(|(cid, _)| *cid == id)
            .map(|(_, v)| v.clone())
            .unwrap_or(serde_json::Value::Null)
    };
    let fired_count = g
        .metrics
        .get(g.best_combo_id as usize)
        .map(|m| m.n_fired as i64)
        .unwrap_or(0);
    let keep: std::collections::HashSet<u32> =
        retained_params.iter().map(|(id, _)| *id).collect();
    let results = g
        .metrics
        .iter()
        .filter(|m| keep.contains(&m.combo_id))
        .map(metrics_to_result)
        .collect();
    GroupedSweepGroupWrite {
        group_index: group_index as i32,
        group_key: g.key.to_json(),
        token_count: g.token_count as i32,
        fired_count,
        best_combo_id: g.best_combo_id as i32,
        best_score: g.best_score,
        best_expectancy_sol: g.best_expectancy_sol,
        best_params: param_at(g.best_combo_id),
        results,
        mints,
    }
}

/// Map one combo's aggregated metrics into a persistable row. `params` is **not**
/// stored per row anymore (deduped into the per-run `_combos` dictionary, migration
/// `0007`) — it's left `Null` here and JOINed back on read, so this skips the
/// per-combo param-JSON clone the old signature did.
fn metrics_to_result(m: &ComboMetrics) -> GroupedSweepResult {
    GroupedSweepResult {
        combo_id: m.combo_id as i32,
        params: serde_json::Value::Null,
        n_fired: m.n_fired as i64,
        n_open: m.n_open as i64,
        n_closed: m.n_closed as i64,
        win_rate: m.win_rate,
        total_pnl_sol: m.total_pnl_sol,
        open_pnl_sol: m.open_pnl_sol,
        mean_pnl_pct: m.mean_pnl_pct,
        median_pnl_pct: m.median_pnl_pct,
        p90_pnl_pct: m.p90_pnl_pct,
        best_pnl_pct: m.best_pnl_pct,
        worst_pnl_pct: m.worst_pnl_pct,
        std_pnl_pct: m.std_pnl_pct,
        profit_factor: m.profit_factor,
        score: m.score,
        expectancy_sol: m.expectancy_sol,
        avg_holding_secs: m.avg_holding_secs,
        median_holding_secs: m.median_holding_secs,
        n_exit_take_profit: m.n_exit_take_profit as i32,
        n_exit_stop_loss: m.n_exit_stop_loss as i32,
        n_exit_trailing: m.n_exit_trailing as i32,
        n_exit_stall: m.n_exit_stall as i32,
        n_exit_time: m.n_exit_time as i32,
        n_exit_liquidity: m.n_exit_liquidity as i32,
        n_exit_next_kill: m.n_exit_next_kill as i32,
        n_exit_dead: m.n_exit_dead as i32,
        n_exit_metrics: m.n_exit_metrics as i32,
        n_exit_open: m.n_exit_open as i32,
    }
}

// ---------------------------------------------------------------------------
// GET endpoints
// ---------------------------------------------------------------------------

/// `GET /api/strategies/sweeps?strategy_id=tpsl2&limit=50` — runs, newest first.
pub async fn list_runs(
    state: web::Data<Arc<LocalState>>,
    query: web::Query<RunsQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let limit = query.limit.clamp(1, 200);
    match GroupedSweepRepo::new(state.db.clone(), tables).list_runs(limit).await {
        Ok(runs) => HttpResponse::Ok().json(runs),
        Err(e) => {
            tracing::error!("DB error listing grouped sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/sweeps/{run_id}/groups?strategy_id=tpsl2` — group
/// summaries for a run (best expectancy first).
pub async fn list_groups(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    match GroupedSweepRepo::new(state.db.clone(), tables).list_groups(run_id).await {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => {
            tracing::error!("DB error listing grouped sweep groups: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/sweeps/{run_id}/groups/{group_id}/results?strategy_id=tpsl2&page=0&limit=200`
///
/// Streams one page of ranked combo rows as NDJSON (one JSON object per line).
/// Best-score combos come first (`ORDER BY score DESC NULLS LAST`).
/// The `X-Total-Count` response header carries the unfiltered group total so the
/// frontend can render a correct page count without a second round-trip.
pub async fn list_results(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<ResultsQuery>,
) -> HttpResponse {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    let page = query.page.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let offset = page * limit;

    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    // The derived `mtm_pnl_pct` column needs the run's `buy_amount_sol`. Fetch it
    // only when a sort level or filter actually references that column — the common
    // path (sorting/filtering the stored columns) skips the extra round-trip.
    let needs_buy = query.sort.as_deref().is_some_and(|s| s.contains("mtm_pnl_pct"))
        || query.sort_col.as_deref() == Some("mtm_pnl_pct")
        || query.filters.as_deref().is_some_and(|s| s.contains("mtm_pnl_pct"));
    let buy_amount_sol = if needs_buy {
        repo.run_buy_amount_sol(run_id).await.ok().flatten()
    } else {
        None
    };

    let order_by = build_order_by(&query, buy_amount_sol);

    // Per-column filters shrink both the page and the total. They bind after the
    // fixed `run_id` ($1) / `group_id` ($2) params, so the fragment starts at $3;
    // it is identical for the count and page queries (both share that prefix).
    let preds = build_filter_preds(query.filters.as_deref(), buy_amount_sol);
    let (filter_sql, filter_binds) = render_filter_sql(&preds, 3);

    let total = match repo.count_results(run_id, group_id, &filter_sql, &filter_binds).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("DB error counting grouped sweep results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    let results = match repo
        .list_results_paged(run_id, group_id, limit, offset, &order_by, &filter_sql, &filter_binds)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("DB error listing grouped sweep results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Serialize each row as a newline-delimited JSON line and stream them through
    // a channel so actix uses chunked transfer — the frontend reader sees rows
    // arrive progressively rather than waiting for the full payload.
    // `actix_web::Error` is !Send so we send plain `Bytes` and wrap in `Ok` below.
    let (tx, rx) = tokio::sync::mpsc::channel::<actix_web::web::Bytes>(64);
    tokio::spawn(async move {
        for result in results {
            match serde_json::to_vec(&result) {
                Ok(mut bytes) => {
                    bytes.push(b'\n');
                    if tx.send(actix_web::web::Bytes::from(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    tracing::error!("NDJSON serialization error: {e}");
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok::<_, actix_web::Error>);

    HttpResponse::Ok()
        .content_type("application/x-ndjson")
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .streaming(stream)
}

/// `POST /api/strategies/sweeps/cancel` — request cancellation of the in-flight
/// grouped sweep. Cooperative: flips the shared cancel flag, which the engine
/// polls between groups / in the per-token loop and bails on (the run then ends as
/// a partial `cancelled` run, announced via the `SweepFinished` SSE frame). A no-op
/// when no sweep is running. Logs every hit so the request reaching the backend is
/// positively confirmable in the log (vs. a cancel stuck queued in the browser).
pub async fn cancel_grouped_sweep(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let running = state.sweep_running.load(Ordering::Acquire);
    tracing::info!(running, "grouped sweep: cancel requested");
    if running {
        state.sweep_cancel.store(true, Ordering::Release);
    }
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": running }))
}

/// `DELETE /api/strategies/sweeps/{run_id}?strategy_id=tpsl2` — drop one run.
/// Groups + results cascade. 404 if the id isn't found.
pub async fn delete_run(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    // Use batch_db: CASCADE deletes the combos table (can be hundreds of thousands
    // of rows, ~1.7 GB) and the API pool's 8s statement_timeout kills it.
    match GroupedSweepRepo::new(state.batch_db.clone(), tables).delete_run(run_id).await {
        Ok(0) => HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"})),
        Ok(n) => HttpResponse::Ok().json(serde_json::json!({"deleted": n})),
        Err(e) => {
            tracing::error!("DB error deleting grouped sweep run: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// Body for [`rename_run`]. A blank/whitespace-only label clears the name
/// (stored as NULL → the UI falls back to the timestamp + grouping hint).
#[derive(serde::Deserialize)]
pub struct RenameBody {
    pub label: String,
}

/// `PATCH /api/strategies/sweeps/{run_id}?strategy_id=tpsl2` — set or clear a
/// run's user-given name. 404 if the id isn't found.
pub async fn rename_run(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
    body: web::Json<RenameBody>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    let trimmed = body.label.trim();
    let label = (!trimmed.is_empty()).then_some(trimmed);
    match GroupedSweepRepo::new(state.db.clone(), tables)
        .update_label(run_id, label)
        .await
    {
        Ok(0) => HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"})),
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({"label": label})),
        Err(e) => {
            tracing::error!("DB error renaming grouped sweep run: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `DELETE /api/strategies/sweeps?strategy_id=tpsl2&before=<rfc3339>` — prune all
/// runs created strictly before `before` (groups + results cascade). `before` is
/// required so this can't accidentally wipe everything.
pub async fn prune_runs(
    state: web::Data<Arc<LocalState>>,
    query: web::Query<PruneQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    // Use batch_db: same cascade volume as delete_run; API pool's 8s timeout kills it.
    match GroupedSweepRepo::new(state.batch_db.clone(), tables)
        .delete_runs_before(query.before)
        .await
    {
        Ok(n) => HttpResponse::Ok().json(serde_json::json!({"deleted": n})),
        Err(e) => {
            tracing::error!("DB error pruning grouped sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Promote a winning group + combo → fingerprint + pre-filled rule draft (5.6)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct PromoteQuery {
    pub strategy_id: String,
    /// Which combo to promote. Omitted ⇒ the group's crowned `best_combo_id`.
    #[serde(default)]
    pub combo_id: Option<i32>,
}

/// `POST /api/strategies/sweeps/{run_id}/groups/{group_id}/promote?strategy_id=generic[&combo_id=N]`
///
/// Turns a swept group's winning combo into a ready-to-save rule (plan 5.6):
/// find-or-creates the group's fingerprint **at the run's bucket width** (so the
/// promoted rule's live matcher buckets exactly as the sweep partitioned), then
/// returns a pre-filled [`RuleDraft`](trading_core::strategies::rules::RuleDraft)-
/// shaped body the frontend opens in the rule editor (review → dry-run → save).
/// The rule itself is NOT persisted here — the editor's save endpoint does that.
pub async fn promote_group(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<PromoteQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    let run = match repo.get_run(run_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"})),
        Err(e) => {
            tracing::error!("promote: get_run failed: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}));
        }
    };
    let group = match repo.get_group(group_id).await {
        Ok(Some(g)) => g,
        Ok(None) => return HttpResponse::NotFound().json(serde_json::json!({"error": "group not found"})),
        Err(e) => {
            tracing::error!("promote: get_group failed: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}));
        }
    };

    // The combo's params: an explicit `combo_id` from the `_combos` dict, else the
    // group's crowned `best_params`.
    let combo_id = query.combo_id.unwrap_or(group.best_combo_id);
    let params = match query.combo_id {
        Some(cid) => match repo.get_combo_params(run_id, cid).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                return HttpResponse::NotFound()
                    .json(serde_json::json!({"error": format!("combo {cid} not found in run")}))
            }
            Err(e) => {
                tracing::error!("promote: get_combo_params failed: {e}");
                return HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}));
            }
        },
        None => group.best_params.clone(),
    };

    // Fingerprint at the run's bucket width (fall back to the default for pre-0002
    // runs), find-or-created so identical winning groups map onto ONE fingerprint.
    let width = run.bucket_width_sol.unwrap_or(SOL_BUCKET_WIDTH);
    let fp_repo = FingerprintRepo::new(state.db.clone());

    // A fingerprint-scoped run already HAS the gate that selected its corpus — reuse
    // it instead of synthesizing one from the group key. The group key only carries
    // the grouped fields (and is `{}` for the usual single-"ALL"-group scoped run),
    // so `fingerprint_from_group_key` would hand back a fingerprint with fewer axes
    // than the sweep actually matched on — for an empty key, one that matches EVERY
    // token. That rule would fire far wider than the results it was promoted from.
    let mut fp = match run.fingerprint_id {
        Some(fp_id) => match fp_repo.find(fp_id).await {
            Ok(Some(f)) => {
                if !group.group_key.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    // Grouped *within* the scope: the promoted rule is the scope gate,
                    // which is wider than this group's slice. Say so in the log — the
                    // extra narrowing is a group-by artifact, not a fingerprint axis.
                    tracing::warn!(
                        %fp_id, group_key = %group.group_key,
                        "promote: scoped run grouped by extra fields — promoting the scope fingerprint (wider than the group)"
                    );
                }
                f
            }
            Ok(None) => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!(
                        "this run was scoped by fingerprint {fp_id}, which no longer exists — \
                         re-run the sweep against a saved fingerprint before promoting"
                    )
                }))
            }
            Err(e) => {
                tracing::error!("promote: scope fingerprint lookup failed: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "database error"}));
            }
        },
        None => {
            let name = format!("sweep {} · group {}", short_id(run_id), group.group_index);
            let mut draft_fp = fingerprint_from_group_key(&group.group_key, width, name);
            // V2.2: Promote copies the run's volume-ix patterns into metric_config so
            // the live/sim rule classifies with the same set the sweep scored.
            if let Some(patterns) = &run.volume_ix_patterns {
                draft_fp.metric_config = serde_json::json!({
                    "m_flow_split": { "volume_ix_patterns": patterns }
                });
                if let Err(e) =
                    hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
                        &draft_fp.metric_config,
                    )
                {
                    return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
                }
            }
            match fp_repo.find_or_create(&draft_fp).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("promote: find_or_create fingerprint failed: {e}");
                    return HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "database error"}));
                }
            }
        }
    };
    // find_or_create ignores metric_config for identity — update if the run has
    // patterns and the stored row differs (width-parity precedent for config). On a
    // scoped run this writes onto the *saved* fingerprint on purpose: the promoted
    // rule must classify flow with the exact pattern set that produced these results,
    // and a fingerprint whose patterns disagree with the sweep would silently score
    // differently live. Validate before touching a row the user owns.
    if let Some(patterns) = &run.volume_ix_patterns {
        let want = serde_json::json!({
            "m_flow_split": { "volume_ix_patterns": patterns }
        });
        if fp.metric_config != want {
            if let Err(e) =
                hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(&want)
            {
                return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
            }
            fp.metric_config = want;
            fp.updated_at = Utc::now();
            if let Err(e) = fp_repo.update(&fp).await {
                tracing::error!("promote: update fingerprint metric_config failed: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "database error"}));
            }
        }
    }

    let buy_amount_sol = run.buy_amount_sol.unwrap_or(registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL);
    let draft = serde_json::json!({
        "rule_name": format!("promoted g{} c{}", group.group_index, combo_id),
        "fingerprint_id": fp.id,
        "trade_mode": "paper",
        "buy_amount_lamports": sol_to_lamports(buy_amount_sol),
        "max_concurrent_tokens": 1,
        "max_total_tokens": 0,
        "params": params,
        // Echo the resolved fingerprint so the editor can render its criteria
        // without a second fetch.
        "fingerprint": fp,
    });
    HttpResponse::Ok().json(draft)
}

/// First 8 chars of a UUID, for a compact human label.
fn short_id(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

/// Cheap pre-resolve check: does the axes JSON mention a flow group name?
fn axes_json_references_flow(axes: &serde_json::Value) -> bool {
    let Some(arr) = axes.get("axes").and_then(|v| v.as_array()) else {
        return false;
    };
    arr.iter().any(|a| {
        matches!(
            a.get("group").and_then(|g| g.as_str()),
            Some("m_flow_split" | "m_flow_split_window")
        )
    })
}

/// Rebuild a [`Fingerprint`] from a stored group key at bucket `width`. The group
/// key is `{field_tag: label}` where continuous SOL fields carry the bucket's
/// `"lo–hi"` label (lower edge = a `width`-multiple) — the representative used is
/// that lower edge, which lies in the bucket, so the fingerprint's `same_bucket`
/// match reproduces the group's membership exactly. Grouping-only fields
/// (`token_program_id`, `is_cashback_enabled`) have no fingerprint axis and the
/// `∅` "missing" label is skipped.
pub(crate) fn fingerprint_from_group_key(
    gk: &serde_json::Value,
    width: f64,
    name: String,
) -> Fingerprint {
    let now = Utc::now();
    let mut fp = Fingerprint {
        id: Uuid::new_v4(),
        name,
        cu_limit: None,
        cu_price: None,
        init_buy_lamports: None,
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: None,
        first_slot_sell_lamports: None,
        bucket_size_amount: tidy_sol_decimal(width),
        ix_labels: None,
        metric_config: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    };
    let Some(obj) = gk.as_object() else { return fp };
    for (tag, val) in obj {
        let Some(field) = GroupField::from_tag(tag) else { continue };
        let Some(s) = val.as_str() else { continue };
        if s == "∅" {
            continue; // the grouping "missing" sentinel — not part of identity
        }
        match field {
            GroupField::CuLimit => fp.cu_limit = s.parse().ok(),
            GroupField::CuPrice => fp.cu_price = s.parse().ok(),
            GroupField::InitialBuySol => fp.init_buy_lamports = parse_lo_lamports(s),
            GroupField::MaxCostLamports => fp.max_cost_lamports = parse_lo_lamports(s),
            GroupField::SpendableLamportsIn => fp.spendable_lamports_in = parse_lo_lamports(s),
            GroupField::FirstSlotBuySol => fp.first_slot_buy_lamports = parse_lo_lamports(s),
            GroupField::FirstSlotSellSol => fp.first_slot_sell_lamports = parse_lo_lamports(s),
            GroupField::IxLabels => {
                fp.ix_labels = Some(s.split(" | ").map(str::to_string).collect())
            }
            // Grouping-only axes with no fingerprint identity.
            GroupField::TokenProgramId | GroupField::IsCashbackEnabled => {}
        }
    }
    fp
}

/// Parse a `"lo–hi"` SOL bucket label's lower edge into lamports (a plain numeric
/// label with no separator parses whole). `None` if it isn't numeric.
fn parse_lo_lamports(label: &str) -> Option<i64> {
    let lo = label.split('–').next()?.trim();
    let sol: f64 = lo.parse().ok()?;
    Some(sol_to_lamports(sol))
}

// ---------------------------------------------------------------------------
// Token-results drill-in (re-simulate one combo on its group's corpus slice)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct TokenResultsQuery {
    pub strategy_id: String,
    pub combo_id: i32,
}

/// `GET /api/strategies/sweeps/{run_id}/groups/{group_id}/token-results`
///
/// Re-simulates a single stored combo against the tokens that belong to the
/// given group and returns per-token PnL / exit / holding-time. The corpus is
/// loaded from the Parquet cache (near-instant for settled windows) so this
/// endpoint is cheap to call on repeat.
pub async fn list_token_results(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<TokenResultsQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    // Load the run header for the selection params.
    let run = match repo.get_run(run_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching grouped sweep run: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Load the group header for its stored group_key.
    let group = match repo.get_group(group_id).await {
        Ok(Some(g)) => g,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "group not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching grouped sweep group: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Fetch the params JSON for the requested combo (from the per-run `_combos`
    // dictionary — `params` is run-level, keyed by `(run_id, combo_id)`).
    let params_json = match repo.get_combo_params(run_id, query.combo_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "combo not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching combo params: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // -----------------------------------------------------------------------
    // Load the group's token corpus — A → B → C cascade.
    //
    // A: in-memory cache (warm path — zero I/O).
    // B: Parquet cache with embedded fingerprints (disk, no DB fingerprint query).
    // C: targeted DB load using the stored group-mint list (cold + uncacheable).
    // Fallback: full corpus load + attach_fingerprints (old groups without mints).
    // -----------------------------------------------------------------------

    // Parse grouping fields once — needed for re-keying on the A/B/fallback paths.
    let grouping_fields: Vec<GroupField> =
        match serde_json::from_value(run.grouping_spec.clone()) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Bad grouping_spec in run: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "invalid grouping spec"}));
            }
        };
    let target_key = group.group_key.clone();
    // Re-partition at the SAME width the run swept, so the re-computed group keys
    // match the stored `target_key`. Legacy rows (NULL) fall back to the default,
    // which is exactly the width they were swept at.
    let run_width = run.bucket_width_sol.unwrap_or(crate::sweep::grouping::SOL_BUCKET_WIDTH);

    // A fingerprint-scoped run's corpus can only be reproduced through the engine
    // matcher (the manual filters are NULL on such a row), so resolve the scope
    // fingerprint up front — the reload below re-applies it exactly like the run did.
    // A deleted fingerprint leaves the id dangling: fall back to no scope rather than
    // 500, and say so, since the group's stored mints still bound the reload.
    let scope_engine_fp = match run.fingerprint_id {
        Some(fp_id) => match FingerprintRepo::new(state.db.clone()).find(fp_id).await {
            Ok(Some(f)) => Some(trading_core::strategies::fingerprint_axes::fp_to_engine(&f)),
            Ok(None) => {
                tracing::warn!(%fp_id, "token-results: scope fingerprint deleted — reload unscoped");
                None
            }
            Err(e) => {
                tracing::error!("token-results: fingerprint lookup failed: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "database error"}));
            }
        },
        None => None,
    };

    // Helper: apply the run's fingerprint scope (or its ix_labels + field filters) to
    // an owned token vec — the two are mutually exclusive, as at run time.
    let apply_filters = |mut tokens: Vec<crate::sweep::corpus::CorpusToken>| -> Vec<crate::sweep::corpus::CorpusToken> {
        if let Some(engine_fp) = scope_engine_fp.as_ref() {
            tokens.retain(|t| hunter_engine::fingerprint::matches(engine_fp, &t.fp));
        }
        if let Some(want) = run
            .ix_labels_filter
            .as_ref()
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .filter(|f| !f.is_empty())
            .map(normalize_label_vec)
        {
            tokens.retain(|t| t.fp.ix_labels == want);
        }
        if let Some(filters) = run.field_filters.as_ref().and_then(|v| {
            serde_json::from_value::<std::collections::HashMap<String, Vec<serde_json::Value>>>(
                v.clone(),
            )
            .ok()
        }) {
            for (field_str, allowed) in &filters {
                if allowed.is_empty() {
                    continue;
                }
                let field: GroupField = match serde_json::from_str(&format!("\"{field_str}\"")) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                if field == GroupField::IxLabels {
                    continue;
                }
                tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
            }
        }
        tokens.retain(|t| group_key(&t.fp, &grouping_fields, run_width).to_json() == target_key);
        tokens
    };

    // --- Option A: in-memory corpus cache ---
    let tokens = {
        let cache = state.sweep_corpus_cache.read().await;
        if cache.as_ref().map(|c| c.corpus_hash.as_str()) == run.corpus_hash.as_deref() {
            tracing::debug!("token-results: Option A cache hit");
            Some(apply_filters((*cache.as_ref().unwrap().tokens).clone()))
        } else {
            None
        }
    };

    let tokens = if let Some(t) = tokens {
        t
    } else {
        // Option A missed — reload from the Parquet lake (DuckDB), the sole corpus
        // source. Prefer the group's stored mints for a targeted load; otherwise fall
        // back to the run's full selection window. The lake embeds fingerprints, so no
        // separate `attach_fingerprints` pass is needed.
        let group_mints = repo.get_group_mints(group_id).await.unwrap_or(None);
        let sel = Selection {
            mints: group_mints.filter(|m| !m.is_empty()),
            created_after: run.created_after,
            created_before: run.created_before,
            curve_only: run.curve_only,
            token_cap: registry::clamp_token_cap(
                run.token_cap
                    .map(|n| n as usize)
                    .unwrap_or(registry::DEFAULT_TOKEN_CAP),
            ),
            window: crate::sweep::corpus::TradeWindow::LaunchWindow,
            per_mint_cap: sweep_per_mint_cap(),
            with_signatures: false,
            with_flow: run.volume_ix_patterns.is_some(),
        };
        let root = crate::lake::lake_root();
        match LakeSource::new(root).load(&sel).await {
            Ok(c) => apply_filters(c.tokens),
            Err(e) => {
                tracing::error!("Failed to load corpus for token-results from lake: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "corpus load failed"}));
            }
        }
    };

    if tokens.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!({
            "rows": [],
            "metrics": metrics_to_result(&ComboMetrics::exact_from_rows(query.combo_id as u32, &[])),
        }));
    }

    // Re-simulate on a blocking thread (CPU-bound but short: one combo × N tokens).
    let strategy_id = query.strategy_id.clone();
    // The run's OWN pricing, not today's defaults — same reason as `as_of` below: a
    // drill-in that repriced under a different fill/cost model would disagree with the
    // combo row it was opened from. Legacy rows (NULL) read back as what the sweep
    // hardcoded then: worst-case fills + `pumpfun_default`.
    let pricing = run_pricing(&run);
    // The run's own instant, NOT wall-clock now — a drill-in must reproduce the row
    // it drills into, and deadness is judged against this (parity plan B7).
    let run_as_of = run.created_at;
    let volume_ix_patterns: Option<Vec<Vec<String>>> = run
        .volume_ix_patterns
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let result = tokio::task::spawn_blocking(move || {
        registry::simulate_one_combo(
            &strategy_id,
            &tokens,
            &params_json,
            pricing,
            run_as_of,
            volume_ix_patterns.as_deref(),
        )
    })
    .await;

    let mut rows = match result {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            tracing::error!("Single-combo simulation failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": format!("simulation failed: {e}")}));
        }
        Err(e) => {
            tracing::error!("spawn_blocking panicked: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "internal error"}));
        }
    };

    // Resolve the real base58 entry/exit signatures the chart/table need to link a
    // fill to its candle. The sweep walks a slim `CorpusTrade` with no signature, so
    // the fills only carry the slot; look the signatures up from the `trades` table
    // by (mint, slot, side). Entry fills are buys, exit fills sells.
    let fill_keys: Vec<(String, u64, bool)> = rows
        .iter()
        .flat_map(|r| {
            let entry = r.entry_slot.map(|s| (r.mint_address.clone(), s, true));
            let exit = r.exit_slot.map(|s| (r.mint_address.clone(), s, false));
            entry.into_iter().chain(exit)
        })
        .collect();
    if !fill_keys.is_empty() {
        let repo = crate::storage::repositories::trade_repo::TradeRepo::new(state.db.clone());
        match repo.resolve_fill_signatures(&fill_keys).await {
            Ok(sigs) => {
                for row in &mut rows {
                    if let Some(s) = row.entry_slot {
                        row.entry_tx = sigs.get(&(row.mint_address.clone(), s, true)).cloned();
                    }
                    if let Some(s) = row.exit_slot {
                        row.exit_tx = sigs.get(&(row.mint_address.clone(), s, false)).cloned();
                    }
                }
            }
            Err(e) => tracing::warn!("token-results: fill signature resolve failed: {e}"),
        }
    }

    // Batch-join token metadata via the shared enrichment SSOT (the same source the
    // Matched / Positions / Simulated tables use) so the frontend table shows the
    // full token column set with no per-row or client-side fetch. `created_at`/
    // `ath_price` are row-owned (excluded from `token`); set them off the row too.
    let mints: Vec<String> = rows.iter().map(|r| r.mint_address.clone()).collect();
    if !mints.is_empty() {
        match trading_core::storage::token_enrichment::fetch_by_mints(&state.db, &mints).await {
            Ok(meta_rows) => {
                let by_mint: std::collections::HashMap<String, _> =
                    meta_rows.into_iter().map(|r| (r.mint_address.clone(), r)).collect();
                for row in &mut rows {
                    if let Some(m) = by_mint.get(&row.mint_address) {
                        row.created_at = Some(m.token_created_at.to_rfc3339());
                        row.ath_price = m.ath_price;
                        row.token = m.into();
                    }
                }
            }
            Err(e) => tracing::warn!("token-results: enrichment fetch failed: {e}"),
        }
    }

    // Exact-quantile summary over this exact row set — the drill-in's own
    // small-N aggregate, computed with no sketch approximation (unlike the
    // group's persisted `ComboMetrics`, which streams through the bounded
    // DDSketch because a wide sweep can't hold every token's PnL in memory).
    // Same shape as a ranked-combo-table row so the frontend can reuse one
    // type (parity plan D1).
    let metrics = metrics_to_result(&ComboMetrics::exact_from_rows(query.combo_id as u32, &rows));

    HttpResponse::Ok().json(serde_json::json!({ "rows": rows, "metrics": metrics }))
}

fn bad_strategy(strategy_id: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(serde_json::json!({
        "error": format!(
            "unknown strategy_id '{}' (supported: {:?})",
            strategy_id,
            registry::strategy_ids()
        )
    }))
}

/// Return `true` if the token's fingerprint value for `field` is in `allowed`.
/// `IxLabels` is handled by the `ix_labels_filter` path and never reaches here.
/// `TokenProgramId` isn't offered in the frontend filter UI.
pub(crate) fn matches_field_filter(
    fp: &crate::sweep::grouping::TokenFingerprint,
    field: crate::sweep::grouping::GroupField,
    allowed: &[serde_json::Value],
) -> bool {
    use crate::sweep::grouping::GroupField::*;
    match field {
        CuLimit => {
            let v = fp.cu_limit;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        CuPrice => {
            let v = fp.cu_price;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        MaxCostLamports => {
            let v = fp.max_cost_lamports;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        SpendableLamportsIn => {
            let v = fp.spendable_lamports_in;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        InitialBuySol => {
            let v = fp.initial_buy_sol;
            allowed.iter().any(|a| {
                a.as_f64()
                    .map(|x| v.map(|y| (x - y).abs() < 1e-9).unwrap_or(false))
                    .unwrap_or(false)
            })
        }
        FirstSlotBuySol => {
            let v = fp.first_slot_buy_sol;
            allowed.iter().any(|a| {
                a.as_f64()
                    .map(|x| v.map(|y| (x - y).abs() < 1e-9).unwrap_or(false))
                    .unwrap_or(false)
            })
        }
        FirstSlotSellSol => {
            let v = fp.first_slot_sell_sol;
            allowed.iter().any(|a| {
                a.as_f64()
                    .map(|x| v.map(|y| (x - y).abs() < 1e-9).unwrap_or(false))
                    .unwrap_or(false)
            })
        }
        IsCashbackEnabled => {
            let v = fp.is_cashback_enabled;
            allowed.iter().any(|a| a.as_bool().map(|b| b == v).unwrap_or(false))
        }
        TokenProgramId | IxLabels => false,
    }
}

#[cfg(test)]
mod order_by_tests {
    use super::order_by_from;

    #[test]
    fn multi_key_builds_all_levels_with_stable_tiebreak() {
        // The reported bug: the secondary key never reached SQL, so rows tied on
        // the primary (all FIRED = 6) fell back to heap order. Both levels must
        // appear, in order, followed by the stable `combo_id` tiebreak. `win_rate`
        // sorts on its display-scaled (`×100`) expression — same order as the raw
        // fraction, and the expression the filter path compares against.
        let order = order_by_from(Some("n_fired:desc,win_rate:desc"), None, None, None);
        assert_eq!(
            order,
            "r.n_fired DESC NULLS LAST, (r.win_rate * 100) DESC NULLS LAST, r.combo_id ASC"
        );
    }

    #[test]
    fn param_column_level_uses_jsonb_expression() {
        let order = order_by_from(Some("p_exit_take_profit:asc,score:desc"), None, None, None);
        assert_eq!(
            order,
            "(c.params->>'exit_take_profit')::numeric ASC NULLS LAST, \
             r.score DESC NULLS LAST, r.combo_id ASC"
        );
    }

    #[test]
    fn unknown_levels_are_dropped() {
        let order = order_by_from(Some("bogus:desc,n_closed:asc"), None, None, None);
        assert_eq!(order, "r.n_closed ASC NULLS LAST, r.combo_id ASC");
        // All-unknown → empty (repo applies its score default).
        assert!(order_by_from(Some("bogus:desc"), None, None, None).is_empty());
    }

    #[test]
    fn legacy_single_pair_fallback() {
        let order = order_by_from(None, Some("win_rate"), Some("asc"), None);
        assert_eq!(order, "(r.win_rate * 100) ASC NULLS LAST, r.combo_id ASC");
    }

    #[test]
    fn empty_sort_yields_empty_default() {
        assert!(order_by_from(None, None, None, None).is_empty());
        assert!(order_by_from(Some(""), None, None, None).is_empty());
    }

    #[test]
    fn mtm_pnl_pct_needs_buy_amount() {
        // Without the run notional the derived column is unresolvable → level dropped.
        assert!(order_by_from(Some("mtm_pnl_pct:desc"), None, None, None).is_empty());
        // With it, the level expands to the same formula the frontend `mtmPctOf` uses.
        let order = order_by_from(Some("mtm_pnl_pct:desc"), None, None, Some(1.0));
        assert_eq!(
            order,
            "((r.total_pnl_sol + r.open_pnl_sol) / NULLIF(1 * r.n_fired, 0) * 100) \
             DESC NULLS LAST, r.combo_id ASC"
        );
    }
}

#[cfg(test)]
mod filter_tests {
    use super::{build_filter_preds, render_filter_sql};

    #[test]
    fn numeric_ops_bind_from_start_index() {
        // Two columns → deterministic (key-sorted) placeholder order: n_closed, score.
        let json = r#"{"score":{"op":"gt","val":0.5},"n_closed":{"op":"gte","val":10}}"#;
        let preds = build_filter_preds(Some(json), None);
        let (sql, binds) = render_filter_sql(&preds, 3);
        assert_eq!(sql, " AND (r.n_closed) >= $3 AND (r.score) > $4");
        assert_eq!(binds, vec![10.0, 0.5]);
    }

    #[test]
    fn between_binds_two_operands() {
        let json = r#"{"win_rate":{"op":"between","min":50,"max":80}}"#;
        let preds = build_filter_preds(Some(json), None);
        let (sql, binds) = render_filter_sql(&preds, 3);
        // Win% folds the display `×100` into the expression so the operand (percent)
        // compares directly against the stored 0..1 fraction. (The already-parenthesized
        // expression is wrapped once more by the renderer — harmless in SQL.)
        assert_eq!(sql, " AND ((r.win_rate * 100)) BETWEEN $3 AND $4");
        assert_eq!(binds, vec![50.0, 80.0]);
    }

    #[test]
    fn param_and_unresolvable_columns() {
        // `p_take_profit` → JSONB expr; `bogus` and non-numeric `mtm_pnl_pct`
        // (no buy amount) are dropped.
        let json = r#"{"p_take_profit":{"op":"lte","val":30},"bogus":{"op":"eq","val":1},"mtm_pnl_pct":{"op":"gt","val":0}}"#;
        let preds = build_filter_preds(Some(json), None);
        let (sql, binds) = render_filter_sql(&preds, 3);
        assert_eq!(sql, " AND ((c.params->>'take_profit')::numeric) <= $3");
        assert_eq!(binds, vec![30.0]);
    }

    #[test]
    fn non_numeric_ops_and_empty_are_dropped() {
        // `contains` has no numeric meaning here; a missing operand also drops.
        let json = r#"{"score":{"op":"contains","val":5},"n_fired":{"op":"gt"}}"#;
        let preds = build_filter_preds(Some(json), None);
        let (sql, binds) = render_filter_sql(&preds, 3);
        assert!(sql.is_empty());
        assert!(binds.is_empty());
        // Absent / malformed JSON → no predicates.
        assert!(build_filter_preds(None, None).is_empty());
        assert!(build_filter_preds(Some("not json"), None).is_empty());
    }
}
