use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{types::Json, PgPool};
use std::collections::HashMap;
use uuid::Uuid;

use crate::api::table_query::{FilterOp, FilterSpec, TableRequest};
use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::models::portfolio::ManagedMint;
use crate::models::strategy::{
    PositionsSummary, StrategyPosition, StrategyRule, StrategyRun, StrategyRunMetrics,
};
use crate::storage::token_enrichment::{
    enrich_filter_sql, enrich_sort_sql, FilterKind, TokenEnrichmentRow, ENRICH_SELECT,
};

// `entry_sol`/`exit_sol` are human SOL (f64) in the model but stored as exact
// lamports (`entry_lamports`/`exit_lamports`, BIGINT) in the column, mirroring
// `trades.amount_lamports`. Token amounts are already exact integers (`u64`) and
// bind/read as `i64` directly. SOL ↔ lamports use the shared `config::constants`
// DB-boundary helpers.

/// Repo spanning the unified strategy schema: `strategy_rules`,
/// `strategy_runs`, `strategy_run_metrics`, `strategy_positions`.
#[derive(Clone)]
pub struct StrategyRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows — keep sqlx derives out of domain models
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct StrategyRuleDbRow {
    id: Uuid,
    strategy_id: String,
    rule_name: String,
    buy_amount_sol: f64,
    trade_mode: String,
    is_active: bool,
    max_concurrent_tokens: Option<i64>,
    max_total_tokens: Option<i64>,
    params: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyRuleDbRow> for StrategyRule {
    fn from(r: StrategyRuleDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
            rule_name: r.rule_name,
            buy_amount_sol: r.buy_amount_sol,
            trade_mode: r.trade_mode,
            is_active: r.is_active,
            max_concurrent_tokens: r.max_concurrent_tokens,
            max_total_tokens: r.max_total_tokens,
            params: r.params.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyRunDbRow {
    id: Uuid,
    strategy_id: String,
    rule_id: Option<Uuid>,
    mode: String,
    run_seq: i64,
    status: String,
    params_snapshot: Json<Value>,
    max_total_tokens: Option<i64>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<StrategyRunDbRow> for StrategyRun {
    fn from(r: StrategyRunDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            mode: r.mode,
            run_seq: r.run_seq,
            status: r.status,
            params_snapshot: r.params_snapshot.0,
            max_total_tokens: r.max_total_tokens,
            started_at: r.started_at,
            finished_at: r.finished_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyRunMetricsDbRow {
    run_id: Uuid,
    rolled_up_at: DateTime<Utc>,
    n_fired: i32,
    n_open: i32,
    n_closed: i32,
    win_rate: f32,
    total_pnl_sol: f32,
    expectancy_sol: f32,
    mean_pnl_pct: f32,
    median_pnl_pct: f32,
    p90_pnl_pct: f32,
    best_pnl_pct: f32,
    worst_pnl_pct: f32,
    std_pnl_pct: f32,
    profit_factor: Option<f32>,
    avg_holding_secs: f32,
    median_holding_secs: f32,
    n_exit_take_profit: i32,
    n_exit_stop_loss: i32,
    n_exit_trailing: i32,
    n_exit_stall: i32,
    n_exit_time: i32,
    n_exit_liquidity: i32,
    n_exit_open: i32,
}

impl From<StrategyRunMetricsDbRow> for StrategyRunMetrics {
    fn from(r: StrategyRunMetricsDbRow) -> Self {
        Self {
            run_id: r.run_id,
            rolled_up_at: r.rolled_up_at,
            n_fired: r.n_fired,
            n_open: r.n_open,
            n_closed: r.n_closed,
            win_rate: r.win_rate,
            total_pnl_sol: r.total_pnl_sol,
            expectancy_sol: r.expectancy_sol,
            mean_pnl_pct: r.mean_pnl_pct,
            median_pnl_pct: r.median_pnl_pct,
            p90_pnl_pct: r.p90_pnl_pct,
            best_pnl_pct: r.best_pnl_pct,
            worst_pnl_pct: r.worst_pnl_pct,
            std_pnl_pct: r.std_pnl_pct,
            profit_factor: r.profit_factor,
            avg_holding_secs: r.avg_holding_secs,
            median_holding_secs: r.median_holding_secs,
            n_exit_take_profit: r.n_exit_take_profit,
            n_exit_stop_loss: r.n_exit_stop_loss,
            n_exit_trailing: r.n_exit_trailing,
            n_exit_stall: r.n_exit_stall,
            n_exit_time: r.n_exit_time,
            n_exit_liquidity: r.n_exit_liquidity,
            n_exit_open: r.n_exit_open,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyPositionDbRow {
    id: Uuid,
    run_id: Uuid,
    strategy_id: String,
    rule_id: Option<Uuid>,
    mode: String,
    mint: String,
    wallet: String,
    token_program_id: Option<String>,
    token_account: Option<String>,
    target_price: Option<f64>,
    // Raw token units (BIGINT) → u64 in the model.
    target_token_amount: Option<i64>,
    target_time: Option<DateTime<Utc>>,
    target_tx: Option<String>,
    entry_price: Option<f64>,
    entry_token_amount: Option<i64>,
    // Lamports (BIGINT) → human SOL f64 in the model.
    entry_lamports: Option<i64>,
    entry_time: Option<DateTime<Utc>>,
    entry_tx_signatures: Json<Value>,
    exit_price: Option<f64>,
    exit_token_amount: Option<i64>,
    exit_lamports: Option<i64>,
    exit_time: Option<DateTime<Utc>>,
    exit_tx_signatures: Json<Value>,
    submitted_buy_signatures: Vec<String>,
    status: String,
    exit_reason: Option<String>,
    extra: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyPositionDbRow> for StrategyPosition {
    fn from(r: StrategyPositionDbRow) -> Self {
        Self {
            id: r.id,
            run_id: r.run_id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            mode: r.mode,
            mint: r.mint,
            wallet: r.wallet,
            token_program_id: r.token_program_id,
            token_account: r.token_account,
            target_price: r.target_price,
            target_token_amount: r.target_token_amount.map(|v| v as u64),
            target_time: r.target_time,
            target_tx: r.target_tx,
            entry_price: r.entry_price,
            entry_token_amount: r.entry_token_amount.map(|v| v as u64),
            entry_sol: r.entry_lamports.map(lamports_to_sol),
            entry_time: r.entry_time,
            entry_tx_signatures: r.entry_tx_signatures.0,
            exit_price: r.exit_price,
            exit_token_amount: r.exit_token_amount.map(|v| v as u64),
            exit_sol: r.exit_lamports.map(lamports_to_sol),
            exit_time: r.exit_time,
            exit_tx_signatures: r.exit_tx_signatures.0,
            submitted_buy_signatures: r.submitted_buy_signatures,
            status: r.status,
            exit_reason: r.exit_reason,
            extra: r.extra.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Per-rule position counters for the rules-table columns (open/pending/total +
/// win/loss + derived win-rate/avg-pct/pnl). Mirrors the runtime cache's live
/// counters, but computed from the synced DB rows so the **lab** rules list
/// (which has no runtime cache) can show them. Keyed by `rule_id` in the batched
/// map. Defaults to all-zero (a rule with no positions).
#[derive(Debug, Default, Clone)]
pub struct RuleCounters {
    /// Entered positions (`entry_price IS NOT NULL`) → `total_positions`.
    pub total_positions: i64,
    /// In the holding index (`Holding`/`Arming`/`BuySubmitted`) → `open_positions`.
    pub open_positions: i64,
    /// Armed but not yet filled (`Arming`/`BuySubmitted`) → `pending_positions`.
    pub pending_positions: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
}

/// Raw batched counter row (one per rule) behind
/// [`StrategyRepo::rule_counters_for_latest_paper_runs`]. Same win/open/closed
/// predicates as [`PositionsSummaryRow`], grouped by `rule_id`.
#[derive(sqlx::FromRow)]
struct RuleCountersRow {
    rule_id: Uuid,
    total: i64,
    open: i64,
    pending: i64,
    win: i64,
    loss: i64,
    total_pnl_lamports: i64,
    sum_pnl_pct: f64,
}

/// Raw aggregate row behind [`StrategyRepo::positions_summary`]. Lamport sums are
/// `BIGINT` (cast in SQL), pct/secs sums `DOUBLE PRECISION`; the repo folds these
/// into the human-facing [`PositionsSummary`] (SOL division, win-rate/avg derive).
#[derive(sqlx::FromRow)]
struct PositionsSummaryRow {
    tokens: i64,
    open: i64,
    win: i64,
    loss: i64,
    total_pnl_lamports: i64,
    total_entry_lamports: i64,
    total_holding_lamports: i64,
    total_gains_lamports: i64,
    total_losses_lamports: i64,
    sum_pnl_pct: f64,
    sum_hold_secs: f64,
    best_pct: Option<f64>,
    worst_pct: Option<f64>,
}

// ---------------------------------------------------------------------------
// Explicit column lists (struct order). Not `SELECT *` so a new physical
// column isn't pulled into every read and the wire contract stays decoupled.
// ---------------------------------------------------------------------------

const RULE_COLS: &str = "id, strategy_id, rule_name, buy_amount_sol, trade_mode, is_active, \
    max_concurrent_tokens, max_total_tokens, params, created_at, updated_at";

const RUN_COLS: &str = "id, strategy_id, rule_id, mode, run_seq, status, params_snapshot, \
    max_total_tokens, started_at, finished_at";

const METRICS_COLS: &str = "run_id, rolled_up_at, n_fired, n_open, n_closed, win_rate, \
    total_pnl_sol, expectancy_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
    worst_pnl_pct, std_pnl_pct, profit_factor, avg_holding_secs, median_holding_secs, \
    n_exit_take_profit, n_exit_stop_loss, n_exit_trailing, n_exit_stall, n_exit_time, \
    n_exit_liquidity, n_exit_open";

const POSITION_COLS: &str = "id, run_id, strategy_id, rule_id, mode, mint, wallet, \
    token_program_id, token_account, target_price, target_token_amount, target_time, target_tx, \
    entry_price, entry_token_amount, entry_lamports, entry_time, entry_tx_signatures, \
    exit_price, exit_token_amount, exit_lamports, exit_time, exit_tx_signatures, \
    submitted_buy_signatures, status, exit_reason, extra, created_at, updated_at";

/// `POSITION_COLS` qualified with the `sp` alias — for the paged read that JOINs
/// `tokens` (so the server can sort/filter by token-enrichment columns too).
const POSITION_COLS_SP: &str = "sp.id, sp.run_id, sp.strategy_id, sp.rule_id, sp.mode, sp.mint, \
    sp.wallet, sp.token_program_id, sp.token_account, sp.target_price, sp.target_token_amount, \
    sp.target_time, sp.target_tx, sp.entry_price, sp.entry_token_amount, sp.entry_lamports, \
    sp.entry_time, sp.entry_tx_signatures, sp.exit_price, sp.exit_token_amount, sp.exit_lamports, \
    sp.exit_time, sp.exit_tx_signatures, sp.submitted_buy_signatures, sp.status, sp.exit_reason, \
    sp.extra, sp.created_at, sp.updated_at";

// ---------------------------------------------------------------------------
// Position list query: server-side sort / filter / search (whitelisted)
// ---------------------------------------------------------------------------

/// A sort/filter/search request for the HTTP positions list, built from the
/// frontend `DataTable`'s emitted view-state. Only **whitelisted** column keys are
/// honored — see [`position_sort_sql`] / [`position_filter_sql`]; anything else is
/// dropped (never interpolated), so no user text ever reaches a SQL identifier.
/// Text values bind as parameters. Applies to the paged list + its count so the
/// pager stays consistent with the filtered view; the **summary** is intentionally
/// left whole-run (it mirrors the strategy-table row).
#[derive(Debug, Clone, Default)]
pub struct PositionQuery {
    /// Free-text search over mint / symbol (ILIKE). Empty = no search.
    pub search: String,
    /// Per-column structured filters as `(frontend_key, spec)`. Non-whitelisted
    /// keys — and specs whose operand shape doesn't fit the op/column type — are
    /// dropped by the SQL builder (never interpolated).
    pub filters: Vec<(String, FilterSpec)>,
    /// Ordered sort keys as `(frontend_key, descending?)`. Non-whitelisted keys are
    /// ignored; an empty resolved list falls back to `created_at DESC`.
    pub sort: Vec<(String, bool)>,
}

impl From<TableRequest> for PositionQuery {
    /// Lower the JSON wire request into the repo query. Paging (`pagination`) is
    /// resolved separately by the handler via [`Page::bounds`](crate::api::table_query::Page::bounds);
    /// only the search / filter / sort view-state carries here.
    fn from(req: TableRequest) -> Self {
        PositionQuery {
            search: req.search,
            filters: req.filters.into_iter().collect(),
            sort: req.sorting.into_iter().map(|s| (s.col, s.dir.is_desc())).collect(),
        }
    }
}

/// Map a frontend column key to its **trusted** SQL sort expression (with table
/// alias). `None` for keys that aren't sortable server-side. Aliases:
///   `sp` = strategy_positions, `t` = LEFT-JOINed `tokens`, `i` = `tokens_info`.
/// The token-enrichment keys mirror the frontend `sharedTokenColumns` set 1:1 so
/// every sortable column on the positions table sorts server-side. `pnl_pct` is
/// computed from the fill prices; the four buy-arg fields are extracted from the
/// `initial_buy_instruction` JSONB.
fn position_sort_sql(key: &str) -> Option<&'static str> {
    Some(match key {
        // strategy_positions — the position-owned arms; everything else is a
        // token-enrichment column resolved by the SSOT whitelist below.
        "mint" => "sp.mint",
        "entry_price" => "sp.entry_price",
        "entry_time" => "sp.entry_time",
        "exit_price" => "sp.exit_price",
        "exit_time" => "sp.exit_time",
        "pnl_pct" => "((sp.exit_price - sp.entry_price) / NULLIF(sp.entry_price, 0))",
        "status" => "sp.status",
        "exit_reason" => "sp.exit_reason",
        _ => return enrich_sort_sql(key),
    })
}

/// Map a frontend column key to its **trusted** SQL expression + type for a
/// positions query. `None` = not filterable. Owns only the `sp.*` arms; the
/// token-enrichment (`t.`/`i.`) columns fall through to
/// [`enrich_filter_sql`][crate::storage::token_enrichment::enrich_filter_sql] —
/// the SSOT shared with the Matched table.
fn position_filter_sql(key: &str) -> Option<(&'static str, FilterKind)> {
    use FilterKind::{Numeric, Text};
    Some(match key {
        "mint" => ("sp.mint", Text),
        "status" => ("sp.status", Text),
        "exit_reason" => ("sp.exit_reason", Text),
        "entry_price" => ("sp.entry_price", Numeric),
        "exit_price" => ("sp.exit_price", Numeric),
        _ => return enrich_filter_sql(key),
    })
}

/// Escape a user search/filter needle for a `LIKE`/`ILIKE` pattern: the SQL
/// wildcards `%` `_` and the escape char `\` are neutralized so they match
/// literally (the value still binds as a parameter — this only affects semantics,
/// not injection). Wrapped `%needle%` for a contains-match by the callers.
fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Coerce a JSON filter operand to `f64`. Accepts a JSON number **or** a numeric
/// string (the frontend serializer may send `"5"`); anything else → `None`, which
/// makes the caller drop the predicate.
fn as_number(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Read a filter operand as trimmed text (for `Contains`/`Eq` on text columns).
/// Accepts a JSON string or number (stringified); empty → `None`.
fn as_text(v: &serde_json::Value) -> Option<String> {
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

/// Lower one structured filter into a bound `AND <predicate>` on `qb`. `col` is a
/// **trusted** whitelisted expression; every operand `push_bind`s as a parameter
/// (injection-safe). A shape/type mismatch (numeric op on a text col, non-numeric
/// operand, missing `between` bound) is a no-op — the filter is silently dropped,
/// matching the whitelist's "unknown key → ignored" contract. Reused by the
/// position + token-scoped filter builders.
fn push_filter_predicate(
    qb: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    col: &str,
    kind: FilterKind,
    spec: &FilterSpec,
) {
    match (kind, spec.op) {
        // -- Text columns: only substring / exact string match ------------------
        (FilterKind::Text, FilterOp::Contains) => {
            let Some(text) = as_text(&spec.val) else { return };
            let needle = format!("%{}%", like_escape(&text));
            qb.push(" AND ").push(col).push(" ILIKE ").push_bind(needle);
        }
        (FilterKind::Text, FilterOp::Eq) => {
            // Exact (escaped, no wildcards) — case-insensitive to match Contains.
            let Some(text) = as_text(&spec.val) else { return };
            qb.push(" AND ").push(col).push(" ILIKE ").push_bind(like_escape(&text));
        }
        // A numeric op on a text column is meaningless → drop.
        (FilterKind::Text, _) => {}

        // -- Numeric columns: numeric comparisons -------------------------------
        (FilterKind::Numeric, FilterOp::Between) => {
            let (Some(min), Some(max)) = (as_number(&spec.min), as_number(&spec.max)) else {
                return;
            };
            qb.push(" AND ")
                .push(col)
                .push(" BETWEEN ")
                .push_bind(min)
                .push(" AND ")
                .push_bind(max);
        }
        (FilterKind::Numeric, op) => {
            let Some(val) = as_number(&spec.val) else { return };
            let sql_op = match op {
                FilterOp::Eq => "=",
                FilterOp::Gt => ">",
                FilterOp::Gte => ">=",
                FilterOp::Lt => "<",
                FilterOp::Lte => "<=",
                // `Contains` on a numeric col is treated as equality (a bare
                // number typed into a numeric column filter).
                FilterOp::Contains => "=",
                FilterOp::Between => unreachable!("handled above"),
            };
            qb.push(" AND ").push(col).push(' ').push(sql_op).push(' ').push_bind(val);
        }
    }
}

/// Append the search + per-column-filter predicates to a positions query builder
/// (the scope predicate is already pushed). Only whitelisted keys are honored;
/// every operand binds as a parameter (see [`push_filter_predicate`]).
fn push_position_where(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &PositionQuery) {
    let search = query.search.trim();
    if !search.is_empty() {
        let needle = format!("%{}%", like_escape(search));
        qb.push(" AND (sp.mint ILIKE ")
            .push_bind(needle.clone())
            .push(" OR t.symbol ILIKE ")
            .push_bind(needle)
            .push(")");
    }
    for (key, spec) in &query.filters {
        if let Some((col, kind)) = position_filter_sql(key) {
            push_filter_predicate(qb, col, kind, spec);
        }
    }
}

/// Append the `ORDER BY` from the whitelisted sort keys, falling back to
/// `sp.created_at DESC` when none resolve. `NULLS LAST` keeps empty fills at the
/// bottom regardless of direction. A trailing `sp.id` tiebreaker makes paging
/// stable when the sort column has ties.
fn push_position_order(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &PositionQuery) {
    let resolved: Vec<(&'static str, bool)> = query
        .sort
        .iter()
        .filter_map(|(key, desc)| position_sort_sql(key).map(|sql| (sql, *desc)))
        .collect();
    qb.push(" ORDER BY ");
    if resolved.is_empty() {
        qb.push("sp.created_at DESC, sp.id DESC");
        return;
    }
    let mut first = true;
    for (sql, desc) in resolved {
        if !first {
            qb.push(", ");
        }
        first = false;
        qb.push(sql).push(if desc { " DESC NULLS LAST" } else { " ASC NULLS LAST" });
    }
    qb.push(", sp.id DESC");
}

// ---------------------------------------------------------------------------
// Matched-token list query: `tokens t LEFT JOIN tokens_info i` scoped to a
// materialized mint set, sharing the structured filter machinery above.
// ---------------------------------------------------------------------------

/// Token-scoped filter whitelist. The matched table has no `sp` join, so `mint`
/// resolves to `t.mint_address` (vs positions' `sp.mint`); everything else falls
/// through to the shared [`enrich_filter_sql`][crate::storage::token_enrichment::enrich_filter_sql]
/// SSOT. `None` = not filterable.
fn token_filter_sql(key: &str) -> Option<(&'static str, FilterKind)> {
    match key {
        "mint" => Some(("t.mint_address", FilterKind::Text)),
        _ => enrich_filter_sql(key),
    }
}

/// Token-scoped sort whitelist — the `mint → t.mint_address` alias plus the shared
/// [`enrich_sort_sql`][crate::storage::token_enrichment::enrich_sort_sql] SSOT.
fn token_sort_sql(key: &str) -> Option<&'static str> {
    match key {
        "mint" => Some("t.mint_address"),
        _ => enrich_sort_sql(key),
    }
}

/// Append the search + per-column filters for the token-scoped (matched) query.
/// Search spans mint / symbol (ILIKE), mirroring the positions search.
fn push_token_where(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &PositionQuery) {
    let search = query.search.trim();
    if !search.is_empty() {
        let needle = format!("%{}%", like_escape(search));
        qb.push(" AND (t.mint_address ILIKE ")
            .push_bind(needle.clone())
            .push(" OR t.symbol ILIKE ")
            .push_bind(needle)
            .push(")");
    }
    for (key, spec) in &query.filters {
        if let Some((col, kind)) = token_filter_sql(key) {
            push_filter_predicate(qb, col, kind, spec);
        }
    }
}

/// `ORDER BY` for the token-scoped (matched) query — falls back to
/// `t.created_at DESC` with a `t.mint_address` tiebreaker for stable paging.
fn push_token_order(qb: &mut sqlx::QueryBuilder<sqlx::Postgres>, query: &PositionQuery) {
    let resolved: Vec<(&'static str, bool)> = query
        .sort
        .iter()
        .filter_map(|(key, desc)| token_sort_sql(key).map(|sql| (sql, *desc)))
        .collect();
    qb.push(" ORDER BY ");
    if resolved.is_empty() {
        qb.push("t.created_at DESC, t.mint_address DESC");
        return;
    }
    let mut first = true;
    for (sql, desc) in resolved {
        if !first {
            qb.push(", ");
        }
        first = false;
        qb.push(sql).push(if desc { " DESC NULLS LAST" } else { " ASC NULLS LAST" });
    }
    qb.push(", t.mint_address DESC");
}

// The matched table now returns the **full** shared enrichment row
// ([`TokenEnrichmentRow`]) — the same SSOT the Positions / Simulated / Sweep tables
// use — so the whole token metadata set is in the response body, sort/filter/search
// all work server-side, and the frontend needs no client-side merge. (The old sparse
// `MatchedTokenRow` + client `mergeTokenData` band-aid is gone.)

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl StrategyRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool — for the few callers that need a free-function query
    /// (e.g. `trade_repo::find_tx_by_fill` on the paper fill-recovery path).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // -- Rules ----------------------------------------------------------------

    pub async fn insert_rule(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_rules
                (id, strategy_id, rule_name, buy_amount_sol, trade_mode, is_active,
                 max_concurrent_tokens, max_total_tokens, params, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(rule.id)
        .bind(&rule.strategy_id)
        .bind(&rule.rule_name)
        .bind(rule.buy_amount_sol)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_rule(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_rules SET
                rule_name = $2,
                buy_amount_sol = $3,
                trade_mode = $4,
                is_active = $5,
                max_concurrent_tokens = $6,
                max_total_tokens = $7,
                params = $8,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(rule.id)
        .bind(&rule.rule_name)
        .bind(rule.buy_amount_sol)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_rule(&self, id: Uuid) -> anyhow::Result<Option<StrategyRule>> {
        let row = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRule::from))
    }

    pub async fn find_rules_by_strategy(
        &self,
        strategy_id: &str,
    ) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE strategy_id = $1 ORDER BY created_at DESC"
        ))
        .bind(strategy_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    pub async fn find_active_rules(&self) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE is_active ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    pub async fn delete_rule(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- Runs -----------------------------------------------------------------

    pub async fn insert_run(&self, run: &StrategyRun) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_runs
                (id, strategy_id, rule_id, mode, run_seq, status, params_snapshot,
                 max_total_tokens, started_at, finished_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(run.id)
        .bind(&run.strategy_id)
        .bind(run.rule_id)
        .bind(&run.mode)
        .bind(run.run_seq)
        .bind(&run.status)
        .bind(Json(&run.params_snapshot))
        .bind(run.max_total_tokens)
        .bind(run.started_at)
        .bind(run.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Next monotonic `run_seq` for `(rule_id, mode)` — `MAX + 1`, starting at 1.
    pub async fn next_run_seq(&self, rule_id: Uuid, mode: &str) -> anyhow::Result<i64> {
        let seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(run_seq), 0) + 1 FROM strategy_runs WHERE rule_id = $1 AND mode = $2",
        )
        .bind(rule_id)
        .bind(mode)
        .fetch_one(&self.pool)
        .await?;
        Ok(seq)
    }

    pub async fn find_run(&self, id: Uuid) -> anyhow::Result<Option<StrategyRun>> {
        let row = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT {RUN_COLS} FROM strategy_runs WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRun::from))
    }

    pub async fn latest_run(
        &self,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<Option<StrategyRun>> {
        let row = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT {RUN_COLS} FROM strategy_runs WHERE rule_id = $1 AND mode = $2 \
             ORDER BY run_seq DESC LIMIT 1"
        ))
        .bind(rule_id)
        .bind(mode)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRun::from))
    }

    /// `(run_id, run_seq)` for every run of a rule in a mode, newest first. Backs
    /// the run-history positions view: the caller takes the first (latest) run as
    /// the "current run" to exclude, and maps the rest onto each history position's
    /// `run_seq` for the run column + banding.
    pub async fn run_seqs_for_rule(
        &self,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<Vec<(Uuid, i64)>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT id, run_seq FROM strategy_runs WHERE rule_id = $1 AND mode = $2 \
             ORDER BY run_seq DESC",
        )
        .bind(rule_id)
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn set_run_status(
        &self,
        id: Uuid,
        status: &str,
        finished_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE strategy_runs SET status = $2, finished_at = $3 WHERE id = $1")
            .bind(id)
            .bind(status)
            .bind(finished_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count of `strategy_positions` ever recorded under a run — zero means the
    /// run never held a position and is safe to [`delete_run`](Self::delete_run)
    /// outright instead of finalizing it as history.
    pub async fn run_position_count(&self, run_id: Uuid) -> anyhow::Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM strategy_positions WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    /// Delete a single run outright (single-row form of
    /// [`delete_runs_by_rule`](Self::delete_runs_by_rule)) — only safe when the run
    /// currently holds zero `strategy_positions` rows (nothing references `run_id`
    /// besides position rows, so cascade is a no-op).
    pub async fn delete_run(&self, run_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_runs WHERE id = $1")
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- Metrics --------------------------------------------------------------

    pub async fn upsert_metrics(&self, m: &StrategyRunMetrics) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_run_metrics
                (run_id, rolled_up_at, n_fired, n_open, n_closed, win_rate, total_pnl_sol,
                 expectancy_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct,
                 worst_pnl_pct, std_pnl_pct, profit_factor, avg_holding_secs, median_holding_secs,
                 n_exit_take_profit, n_exit_stop_loss, n_exit_trailing, n_exit_stall, n_exit_time,
                 n_exit_liquidity, n_exit_open)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, $23, $24)
            ON CONFLICT (run_id) DO UPDATE SET
                rolled_up_at = EXCLUDED.rolled_up_at,
                n_fired = EXCLUDED.n_fired,
                n_open = EXCLUDED.n_open,
                n_closed = EXCLUDED.n_closed,
                win_rate = EXCLUDED.win_rate,
                total_pnl_sol = EXCLUDED.total_pnl_sol,
                expectancy_sol = EXCLUDED.expectancy_sol,
                mean_pnl_pct = EXCLUDED.mean_pnl_pct,
                median_pnl_pct = EXCLUDED.median_pnl_pct,
                p90_pnl_pct = EXCLUDED.p90_pnl_pct,
                best_pnl_pct = EXCLUDED.best_pnl_pct,
                worst_pnl_pct = EXCLUDED.worst_pnl_pct,
                std_pnl_pct = EXCLUDED.std_pnl_pct,
                profit_factor = EXCLUDED.profit_factor,
                avg_holding_secs = EXCLUDED.avg_holding_secs,
                median_holding_secs = EXCLUDED.median_holding_secs,
                n_exit_take_profit = EXCLUDED.n_exit_take_profit,
                n_exit_stop_loss = EXCLUDED.n_exit_stop_loss,
                n_exit_trailing = EXCLUDED.n_exit_trailing,
                n_exit_stall = EXCLUDED.n_exit_stall,
                n_exit_time = EXCLUDED.n_exit_time,
                n_exit_liquidity = EXCLUDED.n_exit_liquidity,
                n_exit_open = EXCLUDED.n_exit_open
            "#,
        )
        .bind(m.run_id)
        .bind(m.rolled_up_at)
        .bind(m.n_fired)
        .bind(m.n_open)
        .bind(m.n_closed)
        .bind(m.win_rate)
        .bind(m.total_pnl_sol)
        .bind(m.expectancy_sol)
        .bind(m.mean_pnl_pct)
        .bind(m.median_pnl_pct)
        .bind(m.p90_pnl_pct)
        .bind(m.best_pnl_pct)
        .bind(m.worst_pnl_pct)
        .bind(m.std_pnl_pct)
        .bind(m.profit_factor)
        .bind(m.avg_holding_secs)
        .bind(m.median_holding_secs)
        .bind(m.n_exit_take_profit)
        .bind(m.n_exit_stop_loss)
        .bind(m.n_exit_trailing)
        .bind(m.n_exit_stall)
        .bind(m.n_exit_time)
        .bind(m.n_exit_liquidity)
        .bind(m.n_exit_open)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_metrics(&self, run_id: Uuid) -> anyhow::Result<Option<StrategyRunMetrics>> {
        let row = sqlx::query_as::<_, StrategyRunMetricsDbRow>(&format!(
            "SELECT {METRICS_COLS} FROM strategy_run_metrics WHERE run_id = $1"
        ))
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRunMetrics::from))
    }

    // -- Positions ------------------------------------------------------------

    pub async fn insert_position(&self, p: &StrategyPosition) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_positions
                (id, run_id, strategy_id, rule_id, mode, mint, wallet, token_program_id,
                 target_price, target_token_amount, target_time, target_tx,
                 entry_price, entry_token_amount, entry_lamports, entry_time, entry_tx_signatures,
                 exit_price, exit_token_amount, exit_lamports, exit_time, exit_tx_signatures,
                 submitted_buy_signatures, status, exit_reason, extra, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint)
        .bind(&p.wallet)
        .bind(p.token_program_id.as_ref())
        .bind(p.target_price)
        .bind(p.target_token_amount.map(|v| v as i64))
        .bind(p.target_time)
        .bind(p.target_tx.as_ref())
        .bind(p.entry_price)
        .bind(p.entry_token_amount.map(|v| v as i64))
        .bind(p.entry_sol.map(sol_to_lamports))
        .bind(p.entry_time)
        .bind(Json(&p.entry_tx_signatures))
        .bind(p.exit_price)
        .bind(p.exit_token_amount.map(|v| v as i64))
        .bind(p.exit_sol.map(sol_to_lamports))
        .bind(p.exit_time)
        .bind(Json(&p.exit_tx_signatures))
        .bind(&p.submitted_buy_signatures)
        .bind(&p.status)
        .bind(p.exit_reason.as_ref())
        .bind(Json(&p.extra))
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_position(&self, p: &StrategyPosition) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_positions SET
                run_id = $2,
                strategy_id = $3,
                rule_id = $4,
                mode = $5,
                mint = $6,
                wallet = $7,
                token_program_id = $8,
                target_price = $9,
                target_token_amount = $10,
                target_time = $11,
                target_tx = $12,
                entry_price = $13,
                entry_token_amount = $14,
                entry_lamports = $15,
                entry_time = $16,
                entry_tx_signatures = $17,
                exit_price = $18,
                exit_token_amount = $19,
                exit_lamports = $20,
                exit_time = $21,
                exit_tx_signatures = $22,
                submitted_buy_signatures = $23,
                status = $24,
                exit_reason = $25,
                extra = $26,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint)
        .bind(&p.wallet)
        .bind(p.token_program_id.as_ref())
        .bind(p.target_price)
        .bind(p.target_token_amount.map(|v| v as i64))
        .bind(p.target_time)
        .bind(p.target_tx.as_ref())
        .bind(p.entry_price)
        .bind(p.entry_token_amount.map(|v| v as i64))
        .bind(p.entry_sol.map(sol_to_lamports))
        .bind(p.entry_time)
        .bind(Json(&p.entry_tx_signatures))
        .bind(p.exit_price)
        .bind(p.exit_token_amount.map(|v| v as i64))
        .bind(p.exit_sol.map(sol_to_lamports))
        .bind(p.exit_time)
        .bind(Json(&p.exit_tx_signatures))
        .bind(&p.submitted_buy_signatures)
        .bind(&p.status)
        .bind(p.exit_reason.as_ref())
        .bind(Json(&p.extra))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_position(&self, id: Uuid) -> anyhow::Result<Option<StrategyPosition>> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyPosition::from))
    }

    pub async fn find_positions_by_run(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE run_id = $1 \
             ORDER BY created_at DESC"
        ))
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    pub async fn find_positions_by_rule(
        &self,
        rule_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE rule_id = $1 \
             ORDER BY created_at DESC LIMIT $2"
        ))
        .bind(rule_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    // -- HTTP read views (page-bounded, newest first) -------------------------
    // Back the live position-read endpoints. Every query is `LIMIT/OFFSET`-bound
    // and the list/by-mint/by-wallet views are scoped by `strategy_id` so a
    // growing `strategy_positions` table is never fetched whole.

    /// Page-bounded positions for one run — the by-rule view resolves a paper
    /// rule's latest run to this (paper retains only the current run's bag).
    pub async fn find_positions_by_run_paged(
        &self,
        run_id: Uuid,
        limit: i64,
        offset: i64,
        query: &PositionQuery,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        self.find_positions_paged("sp.run_id", run_id, None, limit, offset, query).await
    }

    /// Page-bounded positions for a rule across all its runs — the by-rule view
    /// for a real rule (full lifetime history, newest first).
    pub async fn find_positions_by_rule_paged(
        &self,
        rule_id: Uuid,
        limit: i64,
        offset: i64,
        query: &PositionQuery,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        self.find_positions_paged("sp.rule_id", rule_id, None, limit, offset, query).await
    }

    /// Page-bounded positions for a rule across **all runs except one** — the
    /// "old runs" (run-history) view, which excludes the rule's current/latest run
    /// so it complements [`find_positions_by_run_paged`] on the latest run.
    pub async fn find_positions_by_rule_excluding_run_paged(
        &self,
        rule_id: Uuid,
        exclude_run_id: Uuid,
        limit: i64,
        offset: i64,
        query: &PositionQuery,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        self.find_positions_paged("sp.rule_id", rule_id, Some(exclude_run_id), limit, offset, query)
            .await
    }

    /// Shared page query behind the paged views. LEFT-JOINs `tokens` so the
    /// [`PositionQuery`] can sort/filter/search by token-enrichment columns too.
    /// `scope_col` is a trusted literal (`"sp.run_id"` / `"sp.rule_id"`); the
    /// where/order fragments come only from the whitelist resolvers (no user text in
    /// identifiers). `exclude_run` (when set) drops that run from the scope — the
    /// run-history view uses it to omit the current run. Falls back to
    /// `sp.created_at DESC` when no sort resolves.
    async fn find_positions_paged(
        &self,
        scope_col: &str,
        scope_id: Uuid,
        exclude_run: Option<Uuid>,
        limit: i64,
        offset: i64,
        query: &PositionQuery,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT {POSITION_COLS_SP} FROM strategy_positions sp \
             LEFT JOIN tokens t ON t.mint_address = sp.mint \
             LEFT JOIN tokens_info i ON i.mint_address = sp.mint WHERE "
        ));
        qb.push(scope_col).push(" = ").push_bind(scope_id);
        if let Some(exclude) = exclude_run {
            qb.push(" AND sp.run_id <> ").push_bind(exclude);
        }
        push_position_where(&mut qb, query);
        push_position_order(&mut qb, query);
        qb.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
        let rows = qb
            .build_query_as::<StrategyPositionDbRow>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Count of positions for one run (matching `query`'s search/filters) — pairs
    /// with [`find_positions_by_run_paged`] so the pager total tracks the view.
    pub async fn count_positions_by_run(
        &self,
        run_id: Uuid,
        query: &PositionQuery,
    ) -> anyhow::Result<i64> {
        self.count_positions("sp.run_id", run_id, None, query).await
    }

    /// Count of positions for a rule across all its runs (matching `query`).
    pub async fn count_positions_by_rule(
        &self,
        rule_id: Uuid,
        query: &PositionQuery,
    ) -> anyhow::Result<i64> {
        self.count_positions("sp.rule_id", rule_id, None, query).await
    }

    /// Count of a rule's positions across all runs except one (the run-history
    /// view) — pairs with [`find_positions_by_rule_excluding_run_paged`].
    pub async fn count_positions_by_rule_excluding_run(
        &self,
        rule_id: Uuid,
        exclude_run_id: Uuid,
        query: &PositionQuery,
    ) -> anyhow::Result<i64> {
        self.count_positions("sp.rule_id", rule_id, Some(exclude_run_id), query).await
    }

    /// Shared count behind the count views — same JOIN + WHERE as
    /// [`find_positions_paged`], so `total` matches the filtered page exactly.
    async fn count_positions(
        &self,
        scope_col: &str,
        scope_id: Uuid,
        exclude_run: Option<Uuid>,
        query: &PositionQuery,
    ) -> anyhow::Result<i64> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "SELECT COUNT(*) FROM strategy_positions sp \
             LEFT JOIN tokens t ON t.mint_address = sp.mint \
             LEFT JOIN tokens_info i ON i.mint_address = sp.mint WHERE ",
        );
        qb.push(scope_col).push(" = ").push_bind(scope_id);
        if let Some(exclude) = exclude_run {
            qb.push(" AND sp.run_id <> ").push_bind(exclude);
        }
        push_position_where(&mut qb, query);
        let (n,): (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(n)
    }

    // -- Matched tokens (materialized mint set, DB-paged) ----------------------

    /// One page of the matched-token table: `tokens t LEFT JOIN tokens_info i`
    /// restricted to `t.mint_address = ANY($mints)` (the materialized match set),
    /// then the same structured sort/filter/search machinery the positions table
    /// uses — via the token-scoped whitelist. `query` carries only view-state
    /// (paging is the caller's `limit`/`offset`). Removes the old 5,000-row display
    /// cap: the full match set is pageable.
    pub async fn find_tokens_by_mints_paged(
        &self,
        mints: &[String],
        limit: i64,
        offset: i64,
        query: &PositionQuery,
    ) -> anyhow::Result<Vec<TokenEnrichmentRow>> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT {ENRICH_SELECT} FROM tokens t \
             LEFT JOIN tokens_info i ON i.mint_address = t.mint_address \
             WHERE t.mint_address = ANY("
        ));
        qb.push_bind(mints).push(")");
        push_token_where(&mut qb, query);
        push_token_order(&mut qb, query);
        qb.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
        let rows = qb.build_query_as::<TokenEnrichmentRow>().fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// Filtered count of the matched set (same JOIN + WHERE as
    /// [`find_tokens_by_mints_paged`]) so the pager's `total` matches the page.
    pub async fn count_tokens_by_mints(
        &self,
        mints: &[String],
        query: &PositionQuery,
    ) -> anyhow::Result<i64> {
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "SELECT COUNT(*) FROM tokens t \
             LEFT JOIN tokens_info i ON i.mint_address = t.mint_address \
             WHERE t.mint_address = ANY(",
        );
        qb.push_bind(mints).push(")");
        push_token_where(&mut qb, query);
        let (n,): (i64,) = qb.build_query_as().fetch_one(&self.pool).await?;
        Ok(n)
    }

    /// Run-wide position aggregates (paper rules resolve their latest run to this).
    pub async fn positions_summary_by_run(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("run_id", run_id, None).await
    }

    /// Rule-wide position aggregates across all runs (real-rule lifetime history).
    pub async fn positions_summary_by_rule(
        &self,
        rule_id: Uuid,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("rule_id", rule_id, None).await
    }

    /// Rule-wide aggregates across all runs except one (the run-history view) —
    /// complements [`positions_summary_by_run`] on the latest run.
    pub async fn positions_summary_by_rule_excluding_run(
        &self,
        rule_id: Uuid,
        exclude_run_id: Uuid,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("rule_id", rule_id, Some(exclude_run_id)).await
    }

    /// Shared aggregate query behind the summary views. `filter_col` is a
    /// trusted literal (`"run_id"` / `"rule_id"`), never user input — no injection
    /// surface. `exclude_run` (when set) drops that run from the scope (run-history).
    /// All aggregation happens in Postgres (`COUNT/SUM FILTER`), so no rows
    /// are shipped. The win rule mirrors [`StrategyPosition::is_win`]: a clean `End`
    /// exit with positive realized SOL (`exit_lamports > entry_lamports`); every other closed
    /// position is a loss. SOL columns are lamports → divided to human SOL here.
    async fn positions_summary(
        &self,
        filter_col: &str,
        id: Uuid,
        exclude_run: Option<Uuid>,
    ) -> anyhow::Result<PositionsSummary> {
        // Predicates (kept in one place so the two summary views can't drift):
        //   entered  := entry_price IS NOT NULL
        //   open     := status IN ('Holding','Arming','BuySubmitted')
        //   closed   := entry_price IS NOT NULL AND status IN ('End','ExitFailed')
        //   win      := status = 'End' AND exit_lamports > entry_lamports   (SOL basis)
        // Realized SOL PnL = exit_lamports - entry_lamports (lamports); % = (exit-entry)/entry.
        let sql = format!(
            "SELECT \
               COUNT(*) FILTER (WHERE entry_price IS NOT NULL) AS tokens, \
               COUNT(*) FILTER (WHERE status IN ('Holding','Arming','BuySubmitted')) AS open, \
               COUNT(*) FILTER (WHERE status = 'End' AND exit_lamports > entry_lamports) AS win, \
               COUNT(*) FILTER (WHERE entry_price IS NOT NULL AND status IN ('End','ExitFailed') \
                                  AND NOT (status = 'End' AND exit_lamports > entry_lamports)) AS loss, \
               COALESCE(SUM(COALESCE(exit_lamports,0) - entry_lamports) \
                        FILTER (WHERE entry_price IS NOT NULL AND status IN ('End','ExitFailed')), 0)::BIGINT AS total_pnl_lamports, \
               COALESCE(SUM(entry_lamports) FILTER (WHERE entry_price IS NOT NULL), 0)::BIGINT AS total_entry_lamports, \
               COALESCE(SUM(entry_lamports) FILTER (WHERE status IN ('Holding','Arming','BuySubmitted')), 0)::BIGINT AS total_holding_lamports, \
               COALESCE(SUM(COALESCE(exit_lamports,0) - entry_lamports) \
                        FILTER (WHERE status = 'End' AND exit_lamports > entry_lamports), 0)::BIGINT AS total_gains_lamports, \
               COALESCE(SUM(entry_lamports - COALESCE(exit_lamports,0)) \
                        FILTER (WHERE entry_price IS NOT NULL AND status IN ('End','ExitFailed') \
                                  AND NOT (status = 'End' AND exit_lamports > entry_lamports)), 0)::BIGINT AS total_losses_lamports, \
               COALESCE(SUM((exit_price - entry_price) / entry_price * 100.0) \
                        FILTER (WHERE entry_price IS NOT NULL AND entry_price > 0 \
                                  AND exit_price IS NOT NULL AND status IN ('End','ExitFailed')), 0)::DOUBLE PRECISION AS sum_pnl_pct, \
               COALESCE(SUM(EXTRACT(EPOCH FROM (exit_time - entry_time))) \
                        FILTER (WHERE entry_time IS NOT NULL AND exit_time IS NOT NULL \
                                  AND status IN ('End','ExitFailed')), 0)::DOUBLE PRECISION AS sum_hold_secs, \
               MAX((exit_price - entry_price) / entry_price * 100.0) \
                        FILTER (WHERE entry_price IS NOT NULL AND entry_price > 0 \
                                  AND exit_price IS NOT NULL AND status IN ('End','ExitFailed'))::DOUBLE PRECISION AS best_pct, \
               MIN((exit_price - entry_price) / entry_price * 100.0) \
                        FILTER (WHERE entry_price IS NOT NULL AND entry_price > 0 \
                                  AND exit_price IS NOT NULL AND status IN ('End','ExitFailed'))::DOUBLE PRECISION AS worst_pct \
             FROM strategy_positions WHERE {filter_col} = $1{exclude_clause}",
            exclude_clause = if exclude_run.is_some() { " AND run_id <> $2" } else { "" },
        );
        let mut q = sqlx::query_as::<_, PositionsSummaryRow>(&sql).bind(id);
        if let Some(exclude) = exclude_run {
            q = q.bind(exclude);
        }
        let row = q.fetch_one(&self.pool).await?;

        let closed = row.win + row.loss;
        let win_rate = if closed > 0 { row.win as f64 / closed as f64 * 100.0 } else { 0.0 };
        let avg_pnl_pct = if closed > 0 { row.sum_pnl_pct / closed as f64 } else { 0.0 };
        let avg_hold_secs = if closed > 0 { row.sum_hold_secs / closed as f64 } else { 0.0 };

        Ok(PositionsSummary {
            tokens: row.tokens,
            open: row.open,
            win: row.win,
            loss: row.loss,
            closed,
            win_rate,
            avg_pnl_pct,
            total_pnl_sol: lamports_to_sol(row.total_pnl_lamports),
            total_entry_sol: lamports_to_sol(row.total_entry_lamports),
            total_holding_sol: lamports_to_sol(row.total_holding_lamports),
            total_gains_sol: lamports_to_sol(row.total_gains_lamports),
            total_losses_sol: lamports_to_sol(row.total_losses_lamports),
            avg_hold_secs,
            best_pct: row.best_pct,
            worst_pct: row.worst_pct,
        })
    }

    /// Batched per-rule position counters for a strategy, each scoped to the rule's
    /// **latest paper run** — the DB-computed analogue of the live runtime cache's
    /// per-rule counters, for the **lab** rules table (which has no runtime cache).
    ///
    /// One query: `DISTINCT ON (rule_id)` picks each rule's newest paper run, then
    /// its positions are aggregated with the same win/open/closed predicates as
    /// [`positions_summary`]. Returns a `rule_id → RuleCounters` map; rules with no
    /// paper run / no positions are simply absent (caller defaults them to zero).
    pub async fn rule_counters_for_latest_paper_runs(
        &self,
        strategy_id: &str,
    ) -> anyhow::Result<HashMap<Uuid, RuleCounters>> {
        // `latest` = each rule's newest paper run; join its positions and aggregate.
        // Predicates mirror `positions_summary` exactly so the rule-row counts and
        // the Positions Summary panel can't drift.
        let sql = "\
            WITH latest AS ( \
                SELECT DISTINCT ON (rule_id) rule_id, id AS run_id \
                FROM strategy_runs \
                WHERE mode = 'paper' AND rule_id IS NOT NULL AND strategy_id = $1 \
                ORDER BY rule_id, run_seq DESC \
            ) \
            SELECT l.rule_id AS rule_id, \
              COUNT(p.*) FILTER (WHERE p.entry_price IS NOT NULL) AS total, \
              COUNT(p.*) FILTER (WHERE p.status IN ('Holding','Arming','BuySubmitted')) AS open, \
              COUNT(p.*) FILTER (WHERE p.status IN ('Arming','BuySubmitted')) AS pending, \
              COUNT(p.*) FILTER (WHERE p.status = 'End' AND p.exit_lamports > p.entry_lamports) AS win, \
              COUNT(p.*) FILTER (WHERE p.entry_price IS NOT NULL AND p.status IN ('End','ExitFailed') \
                                   AND NOT (p.status = 'End' AND p.exit_lamports > p.entry_lamports)) AS loss, \
              COALESCE(SUM(COALESCE(p.exit_lamports,0) - p.entry_lamports) \
                       FILTER (WHERE p.entry_price IS NOT NULL AND p.status IN ('End','ExitFailed')), 0)::BIGINT AS total_pnl_lamports, \
              COALESCE(SUM((p.exit_price - p.entry_price) / p.entry_price * 100.0) \
                       FILTER (WHERE p.entry_price IS NOT NULL AND p.entry_price > 0 \
                                 AND p.exit_price IS NOT NULL AND p.status IN ('End','ExitFailed')), 0)::DOUBLE PRECISION AS sum_pnl_pct \
            FROM latest l \
            LEFT JOIN strategy_positions p ON p.run_id = l.run_id \
            GROUP BY l.rule_id";
        let rows = sqlx::query_as::<_, RuleCountersRow>(sql)
            .bind(strategy_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let closed = r.win + r.loss;
                let win_rate = if closed > 0 { r.win as f64 / closed as f64 * 100.0 } else { 0.0 };
                let avg_pnl_pct = if closed > 0 { r.sum_pnl_pct / closed as f64 } else { 0.0 };
                (
                    r.rule_id,
                    RuleCounters {
                        total_positions: r.total,
                        open_positions: r.open,
                        pending_positions: r.pending,
                        win_count: r.win,
                        loss_count: r.loss,
                        win_rate,
                        avg_pnl_pct,
                        total_pnl_sol: lamports_to_sol(r.total_pnl_lamports),
                    },
                )
            })
            .collect())
    }

    /// Page-bounded positions for a strategy family — the HTTP list view.
    pub async fn find_positions_by_strategy(
        &self,
        strategy_id: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE strategy_id = $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        ))
        .bind(strategy_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded in-holding-index positions (`Arming`/`BuySubmitted`/`Holding`)
    /// for one mint within a strategy — the HTTP by-mint view.
    pub async fn find_holding_by_mint(
        &self,
        strategy_id: &str,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE strategy_id = $1 AND mint = $2 \
               AND status IN ('Holding', 'Arming', 'BuySubmitted') \
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        ))
        .bind(strategy_id)
        .bind(mint)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded in-holding-index positions for one wallet within a strategy —
    /// the HTTP by-wallet view.
    pub async fn find_holding_by_wallet(
        &self,
        strategy_id: &str,
        wallet: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE strategy_id = $1 AND wallet = $2 \
               AND status IN ('Holding', 'Arming', 'BuySubmitted') \
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        ))
        .bind(strategy_id)
        .bind(wallet)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    pub async fn find_open_positions(&self) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status NOT IN ('End', 'ExitFailed') ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Cross-strategy "who manages this mint" — every open (unsettled) position,
    /// each tagged with its rule's human name via a `LEFT JOIN strategy_rules`.
    /// A thin, projection-only read (no full `StrategyPosition` rows) backing the
    /// portfolio bot badge / Trade-page interlock. Open positions are a tiny set,
    /// so this is unbounded-but-cheap; the service correlates it against the held
    /// mints in memory. `real_only` restricts to live-money positions (`mode='real'`).
    /// A mint can have several open positions — the caller picks the one that matters.
    pub async fn managed_mints(&self, real_only: bool) -> anyhow::Result<Vec<ManagedMint>> {
        let rows: Vec<(String, Option<Uuid>, Option<String>, String, String)> = sqlx::query_as(
            "SELECT p.mint, p.rule_id, r.rule_name, p.status, p.mode \
             FROM strategy_positions p \
             LEFT JOIN strategy_rules r ON r.id = p.rule_id \
             WHERE p.status NOT IN ('End', 'ExitFailed') \
               AND ($1 = false OR p.mode = 'real') \
             ORDER BY p.created_at DESC",
        )
        .bind(real_only)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(mint, rule_id, rule_name, status, mode)| ManagedMint {
                mint,
                rule_id,
                rule_name,
                status,
                mode,
            })
            .collect())
    }

    /// Distinct mints with an unsettled **real** position — every mint the wallet
    /// could legitimately hold a bag for. Covers `Holding`/`Arming`/`BuySubmitted`/
    /// `ExitPending`/`ExitFailed` (the last can still hold a bag whose sell failed),
    /// across all strategies. Backs the boot wallet-reconcile sweep. Positive `IN`
    /// over the unsettled statuses so the predicate stays index-servable.
    pub async fn distinct_unsettled_real_mints(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT mint FROM strategy_positions \
             WHERE mode = 'real' \
               AND status IN ('Holding', 'Arming', 'BuySubmitted', 'ExitPending', 'ExitFailed')",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }

    /// The latest run per rule for a given `mode` (one row per `rule_id`, highest
    /// `run_seq`). Backs the lab rule-list enrichment (derive a paper rule's
    /// `Finished`/`Idle` lifecycle in one query, no N+1). `DISTINCT ON` keyed by
    /// `rule_id` after ordering by `run_seq DESC`.
    pub async fn latest_runs_by_rule(&self, mode: &str) -> anyhow::Result<Vec<StrategyRun>> {
        let rows = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT DISTINCT ON (rule_id) {RUN_COLS} FROM strategy_runs \
             WHERE mode = $1 AND rule_id IS NOT NULL \
             ORDER BY rule_id, run_seq DESC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRun::from).collect())
    }

    /// Delete every run (and, via `ON DELETE CASCADE`, its positions/metrics) for a
    /// rule in a given `mode`. Backs the lab "clear paper results" action. Returns
    /// the number of runs removed.
    pub async fn delete_runs_by_rule(&self, rule_id: Uuid, mode: &str) -> anyhow::Result<u64> {
        let res = sqlx::query("DELETE FROM strategy_runs WHERE rule_id = $1 AND mode = $2")
            .bind(rule_id)
            .bind(mode)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    pub async fn delete_position(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_positions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Append a submitted snipe-buy signature and flip the row to `BuySubmitted`
    /// (the durable write-ahead "buy in flight" marker), returning the updated row.
    /// Guarded by `entry_price IS NULL` so a concurrent fill that already advanced
    /// the row to `Holding` is never clobbered back — returns `None` in that benign
    /// case. Single round-trip (`RETURNING`) so the caller syncs the cache off it.
    pub async fn mark_buy_submitted(
        &self,
        id: Uuid,
        signature: &str,
    ) -> anyhow::Result<Option<StrategyPosition>> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "UPDATE strategy_positions \
             SET status = 'BuySubmitted', \
                 submitted_buy_signatures = array_append(submitted_buy_signatures, $2), \
                 updated_at = now() \
             WHERE id = $1 AND entry_price IS NULL \
             RETURNING {POSITION_COLS}"
        ))
        .bind(id)
        .bind(signature)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyPosition::from))
    }

    /// Record the entry fill atomically (entry tx/amount/price/sol/time + flip to
    /// `Holding`) and return the fresh row in one round-trip. The single-leg entry
    /// signature is stored as a JSONB array. Mirrors the old per-strategy
    /// `update_entry`; the `RETURNING` lets the caller sync the cache without a
    /// follow-up read.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_entry_fill(
        &self,
        id: Uuid,
        entry_tx: &str,
        entry_token_amount: u64,
        entry_price: f64,
        entry_sol: f64,
        entry_time: DateTime<Utc>,
        // The wallet's token account for this mint, captured from the trader cache
        // at fill time. Persisted so subsequent buys reuse one account and the sell
        // reads it from the row (survives restarts). `None` keeps the existing value
        // (`COALESCE`) — a later fill never blanks an already-recorded account.
        token_account: Option<&str>,
    ) -> anyhow::Result<StrategyPosition> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "UPDATE strategy_positions \
             SET entry_tx_signatures = $2, entry_token_amount = $3, entry_price = $4, \
                 entry_lamports = $5, entry_time = $6, \
                 token_account = COALESCE($7, token_account), \
                 status = 'Holding', updated_at = now() \
             WHERE id = $1 \
             RETURNING {POSITION_COLS}"
        ))
        .bind(id)
        .bind(Json(json!([entry_tx])))
        .bind(entry_token_amount as i64)
        .bind(entry_price)
        .bind(sol_to_lamports(entry_sol))
        .bind(entry_time)
        .bind(token_account)
        .fetch_one(&self.pool)
        .await?;
        Ok(StrategyPosition::from(row))
    }

    // -- Recovery reaper queries (mode-scoped) --------------------------------
    // The live service runs these per mode ('real' / 'paper'); they replace the
    // per-strategy real/paper repos' reaper queries. Small result sets in normal
    // operation (index-served by the status predicates).

    /// Positions stranded in `ExitPending` for a mode — the exit-recovery reaper
    /// re-drives a sell whose task panicked / was lost to a restart (the holding
    /// cache only loads open rows, so these are otherwise invisible).
    pub async fn find_all_exit_pending(&self, mode: &str) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status = 'ExitPending' AND mode = $1 ORDER BY updated_at ASC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Positions stuck in `BuySubmitted` for a mode — the buy-recovery reaper
    /// checks each row's submitted signatures against the feed/chain and
    /// adopts/waits/drops (never blindly deletes — tokens may exist on-chain).
    pub async fn find_all_buy_submitted(&self, mode: &str) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status = 'BuySubmitted' AND mode = $1 ORDER BY updated_at ASC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Open (`Holding`, entry-recorded) positions for one mint in a mode — drives
    /// the manual-sell reconcile (an externally-cleared bag closed without a sell).
    pub async fn find_open_by_mint(
        &self,
        mint: &str,
        mode: &str,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE mint = $1 AND mode = $2 AND status = 'Holding' AND entry_price IS NOT NULL"
        ))
        .bind(mint)
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// The persisted `token_account` for any open position on the same
    /// `(wallet, mint)` in a mode — so a subsequent bot buy into a mint already
    /// held reuses that one account instead of the template pool minting a second.
    /// Scans `Arming`/`BuySubmitted`/`Holding` rows (any in-flight or held
    /// position), newest first, returning the first non-null account. `None` =
    /// nothing held with a recorded account, so the buy proceeds as today.
    pub async fn find_reusable_token_account(
        &self,
        wallet: &str,
        mint: &str,
        mode: &str,
    ) -> anyhow::Result<Option<String>> {
        let acct: Option<String> = sqlx::query_scalar(
            "SELECT token_account FROM strategy_positions \
             WHERE wallet = $1 AND mint = $2 AND mode = $3 \
               AND status IN ('Arming','BuySubmitted','Holding') \
               AND token_account IS NOT NULL \
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(wallet)
        .bind(mint)
        .bind(mode)
        .fetch_optional(&self.pool)
        .await?;
        Ok(acct)
    }

    /// Terminally fail positions stuck in `ExitPending` past `stale_after` (orphaned
    /// mid-exit). Re-arming a half-done real exit risks a double-sell, so fail it.
    /// Returns rows affected.
    pub async fn fail_stale_exit_pending(
        &self,
        mode: &str,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            "UPDATE strategy_positions SET status = 'ExitFailed', updated_at = now() \
             WHERE status = 'ExitPending' AND mode = $1 AND updated_at < $2",
        )
        .bind(mode)
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Delete positions left `Arming` with no entry fill past `stale_after` — they
    /// matched a rule but never sent a buy (no SOL, no tokens), so they're safe to
    /// drop. Scoped to `Arming` only: a `BuySubmitted` row may own tokens and is the
    /// buy-recovery reaper's responsibility. Returns rows deleted.
    pub async fn delete_stale_unentered(
        &self,
        mode: &str,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            "DELETE FROM strategy_positions \
             WHERE status = 'Arming' AND entry_price IS NULL AND mode = $1 AND created_at < $2",
        )
        .bind(mode)
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Distinct mints with an entry-recorded `Holding` real position whose net traded
    /// balance (Σbuys − Σsells) has fallen to ≤ `threshold_raw` — the bag was cleared
    /// outside the strategy exit path (a manual sell). Drives the boot/maintenance
    /// manual-sell reaper. `threshold_raw` is in **raw token base units** (the new
    /// `trades.token_amount` is BIGINT raw units, not decimal tokens). Joins
    /// `wallet_dict` to resolve the interned `wallet_id`.
    pub async fn find_externally_cleared_holding_mints(
        &self,
        threshold_raw: i64,
    ) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT p.mint
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = 'Holding' AND p.entry_price IS NOT NULL
              -- Require a sell on record: else a position whose BUY merely aged out of
              -- the rolling buffer (net = 0, no sell) would be falsely "cleared".
              AND EXISTS (
                    SELECT 1 FROM trades s
                    WHERE s.wallet_id = w.id AND s.mint_address = p.mint
                      AND s.trade_type = 'sell'
                  )
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint
                  ), 0)::bigint <= $1
            "#,
        )
        .bind(threshold_raw)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }
}

#[cfg(test)]
mod filter_sql_tests {
    //! Pure (no-DB) tests over the SQL that `push_position_where` emits for the new
    //! structured [`FilterSpec`]s: numeric ops must lower to a numeric predicate
    //! (not `::text ILIKE`), and an illegal op/column pairing must be dropped.
    use super::*;
    use crate::api::table_query::{FilterOp, FilterSpec};

    /// Build the `WHERE` fragment `push_position_where` emits for one filter and
    /// return the accumulated SQL string (bind params show as `$N`).
    fn where_sql(key: &str, spec: FilterSpec) -> String {
        let query = PositionQuery {
            search: String::new(),
            filters: vec![(key.to_string(), spec)],
            sort: Vec::new(),
        };
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 WHERE true");
        push_position_where(&mut qb, &query);
        qb.sql().to_string()
    }

    fn num(op: FilterOp, v: f64) -> FilterSpec {
        FilterSpec { op, val: serde_json::json!(v), ..Default::default() }
    }

    #[test]
    fn gt_on_numeric_col_emits_numeric_predicate() {
        let sql = where_sql("volume", num(FilterOp::Gt, 100.0));
        assert!(sql.contains("i.volume_sol >"), "expected numeric compare, got: {sql}");
        assert!(!sql.contains("::text"), "numeric op must not cast to text: {sql}");
        assert!(!sql.contains("ILIKE"), "numeric op must not ILIKE: {sql}");
    }

    #[test]
    fn between_on_numeric_col_emits_between() {
        let spec = FilterSpec {
            op: FilterOp::Between,
            min: serde_json::json!(1),
            max: serde_json::json!(10),
            ..Default::default()
        };
        let sql = where_sql("market_cap", spec);
        assert!(sql.contains("BETWEEN"), "expected BETWEEN, got: {sql}");
        assert!(!sql.contains("::text"), "between must not cast to text: {sql}");
    }

    #[test]
    fn numeric_op_on_text_col_is_dropped() {
        // `symbol` is a Text column; a `gt` on it is meaningless → no predicate.
        let sql = where_sql("symbol", num(FilterOp::Gt, 5.0));
        assert!(!sql.contains(" AND "), "numeric op on text col must be dropped: {sql}");
    }

    #[test]
    fn contains_on_text_col_still_ilikes() {
        let spec = FilterSpec {
            op: FilterOp::Contains,
            val: serde_json::json!("pump"),
            ..Default::default()
        };
        let sql = where_sql("symbol", spec);
        assert!(sql.contains("t.symbol ILIKE"), "text contains must ILIKE: {sql}");
    }

    #[test]
    fn numeric_op_with_non_number_val_is_dropped() {
        let spec = FilterSpec {
            op: FilterOp::Gt,
            val: serde_json::json!("not-a-number"),
            ..Default::default()
        };
        let sql = where_sql("volume", spec);
        assert!(!sql.contains(" AND "), "non-numeric val on numeric op must be dropped: {sql}");
    }

    #[test]
    fn numeric_string_val_is_accepted() {
        // The serializer may send `"5"` as a string; still a valid numeric compare.
        let spec = FilterSpec {
            op: FilterOp::Gte,
            val: serde_json::json!("5"),
            ..Default::default()
        };
        let sql = where_sql("current_price", spec);
        assert!(sql.contains("i.current_price >="), "numeric string must compare: {sql}");
    }
}
