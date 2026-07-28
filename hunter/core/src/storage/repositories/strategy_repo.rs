use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{types::Json, PgPool};
use std::collections::HashMap;
use uuid::Uuid;

use crate::api::table_query::{FilterOp, FilterSpec, MAX_FILTER_IN_VALUES, TableRequest};
use crate::config::constants::{lamports_to_sol, sol_to_lamports};
use crate::strategies::kernel::{round_trip_with_costs, weighted_return_pct, CostModel};
use crate::models::portfolio::ManagedMint;
use crate::models::strategy::{
    ExitReasonCounts, PositionsSummary, StrategyPosition, StrategyRun, StrategyRunMetrics,
};
use crate::storage::token_enrichment::{
    enrich_filter_sql, enrich_sort_sql, FilterKind, TokenEnrichmentRow, ENRICH_SELECT,
};

// `entry_sol`/`exit_sol` are human SOL (f64) in the model but stored as exact
// lamports (`entry_lamports`/`exit_lamports`, BIGINT) in the column, mirroring
// `trades.amount_lamports`. Token amounts are already exact integers (`u64`) and
// bind/read as `i64` directly. SOL ↔ lamports use the shared `config::constants`
// DB-boundary helpers.

/// Repo spanning the unified strategy schema (`strategy_runs`,
/// `strategy_run_metrics`, `strategy_positions`). The generic engine's rule CRUD
/// lives on `RuleRepo` (`strategy_rules`); the pre-0004 `strategy_rules_legacy`
/// table is no longer read here (retired in Phase 7).
#[derive(Clone)]
pub struct StrategyRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows — keep sqlx derives out of domain models
// ---------------------------------------------------------------------------

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

/// Flat LEFT JOIN of `strategy_runs` × `strategy_run_metrics` for the Rules
/// Evidence run navigator (`GET …/strategy-rules/{id}/runs`).
#[derive(sqlx::FromRow)]
struct RuleRunListDbRow {
    id: Uuid,
    run_seq: i64,
    status: String,
    mode: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    n_closed: Option<i32>,
    n_open: Option<i32>,
    win_rate: Option<f32>,
    total_pnl_sol: Option<f32>,
    expectancy_sol: Option<f32>,
    n_exit_take_profit: Option<i32>,
    n_exit_stop_loss: Option<i32>,
    n_exit_trailing: Option<i32>,
    n_exit_stall: Option<i32>,
    n_exit_time: Option<i32>,
    n_exit_liquidity: Option<i32>,
}

/// Public list row for a rule's runs (+ optional finalized metrics).
#[derive(Debug, Clone, Serialize)]
pub struct RuleRunListRow {
    pub id: Uuid,
    pub run_seq: i64,
    pub status: String,
    pub mode: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// True when a `strategy_run_metrics` row exists (typically finished runs).
    pub has_metrics: bool,
    pub n_closed: Option<i32>,
    pub n_open: Option<i32>,
    pub win_rate: Option<f32>,
    pub total_pnl_sol: Option<f32>,
    pub expectancy_sol: Option<f32>,
    pub n_exit_take_profit: Option<i32>,
    pub n_exit_stop_loss: Option<i32>,
    pub n_exit_trailing: Option<i32>,
    pub n_exit_stall: Option<i32>,
    pub n_exit_time: Option<i32>,
    pub n_exit_liquidity: Option<i32>,
}

impl From<RuleRunListDbRow> for RuleRunListRow {
    fn from(r: RuleRunListDbRow) -> Self {
        let has_metrics = r.n_closed.is_some();
        Self {
            id: r.id,
            run_seq: r.run_seq,
            status: r.status,
            mode: r.mode,
            started_at: r.started_at,
            finished_at: r.finished_at,
            has_metrics,
            n_closed: r.n_closed,
            n_open: r.n_open,
            win_rate: r.win_rate,
            total_pnl_sol: r.total_pnl_sol,
            expectancy_sol: r.expectancy_sol,
            n_exit_take_profit: r.n_exit_take_profit,
            n_exit_stop_loss: r.n_exit_stop_loss,
            n_exit_trailing: r.n_exit_trailing,
            n_exit_stall: r.n_exit_stall,
            n_exit_time: r.n_exit_time,
            n_exit_liquidity: r.n_exit_liquidity,
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
    mint_address: String,
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
    origin: String,
    manual_exit: Option<Json<Value>>,
    exit_redrive_count: i32,
    exit_parked: bool,
    last_entry_error: Option<String>,
    extra: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyPositionDbRow> for StrategyPosition {
    fn from(r: StrategyPositionDbRow) -> Self {
        // ONE derivation site for the B3 attention flag: a real BuySubmitted older
        // than the review window with no adopted fill needs a manual Verify.
        let needs_review = r.status == "BuySubmitted"
            && r.mode == "real"
            && (Utc::now() - r.updated_at).num_seconds()
                > crate::config::constants::BUY_SUBMITTED_REVIEW_SECS as i64;
        Self {
            id: r.id,
            run_id: r.run_id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            mode: r.mode,
            mint_address: r.mint_address,
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
            origin: r.origin,
            manual_exit: r.manual_exit.map(|j| j.0),
            exit_redrive_count: r.exit_redrive_count,
            exit_parked: r.exit_parked,
            last_entry_error: r.last_entry_error,
            needs_review,
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
    /// Open-partition rows (not `End`/`EntryFailed`) → `open_positions`.
    pub open_positions: i64,
    /// Buy in flight, not yet filled (`BuySubmitted`) → `pending_positions`.
    pub pending_positions: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub avg_pnl_pct: f64,
    pub total_pnl_sol: f64,
}

/// One rule's closed-trade rollup over a calendar window — Portfolio page.
#[derive(Debug, Clone, Serialize)]
pub struct RulePeriodPnlRow {
    pub rule_id: Uuid,
    pub rule_name: Option<String>,
    pub closed: i64,
    pub win: i64,
    pub loss: i64,
    pub win_rate: f64,
    pub realized_pnl_sol: f64,
    pub total_entry_sol: f64,
}

#[derive(sqlx::FromRow)]
struct RulePeriodPnlDbRow {
    rule_id: Uuid,
    rule_name: Option<String>,
    closed: i64,
    win: i64,
    loss: i64,
    total_pnl_lamports: i64,
    total_entry_lamports: i64,
}

impl From<RulePeriodPnlDbRow> for RulePeriodPnlRow {
    fn from(r: RulePeriodPnlDbRow) -> Self {
        let closed = r.closed;
        let win_rate = if closed > 0 {
            (r.win as f64 / closed as f64) * 100.0
        } else {
            0.0
        };
        Self {
            rule_id: r.rule_id,
            rule_name: r.rule_name,
            closed,
            win: r.win,
            loss: r.loss,
            win_rate,
            realized_pnl_sol: lamports_to_sol(r.total_pnl_lamports),
            total_entry_sol: lamports_to_sol(r.total_entry_lamports),
        }
    }
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
    /// SOL deployed (entry cost) across the closed positions — the capital base for
    /// the canonical capital-weighted return; paired with `total_pnl_lamports`.
    closed_entry_lamports: i64,
}

/// Shared aggregate column list for the batched per-rule latest-run counters
/// ([`StrategyRepo::rule_counters_for_latest_paper_runs`]). Kept in ONE place so
/// the win/open/closed predicates can't drift from each other — and
/// they mirror [`StrategyRepo::positions_summary`] so the rule-row and the Positions
/// Summary panel always agree. Expects a `latest l(rule_id, run_id)` CTE LEFT-JOINed
/// to `strategy_positions p` and a trailing `GROUP BY l.rule_id`.
/// Aggregate arms shared by latest-run (alias `l`/`p`) and all-time (alias `p`)
/// rule-counter queries — keep predicates identical to `positions_summary`.
const RULE_COUNTERS_AGGS: &str = "\
    COUNT(p.*) FILTER (WHERE p.entry_price IS NOT NULL) AS total, \
    COUNT(p.*) FILTER (WHERE p.status NOT IN ('End','EntryFailed')) AS open, \
    COUNT(p.*) FILTER (WHERE p.status = 'BuySubmitted') AS pending, \
    COUNT(p.*) FILTER (WHERE p.status = 'End' AND p.exit_lamports > p.entry_lamports) AS win, \
    COUNT(p.*) FILTER (WHERE p.entry_price IS NOT NULL AND p.status = 'End' \
                         AND NOT (p.exit_lamports > p.entry_lamports)) AS loss, \
    COALESCE(SUM(COALESCE(p.exit_lamports,0) - p.entry_lamports) \
             FILTER (WHERE p.entry_price IS NOT NULL AND p.status = 'End'), 0)::BIGINT AS total_pnl_lamports, \
    COALESCE(SUM(p.entry_lamports) \
             FILTER (WHERE p.entry_price IS NOT NULL AND p.status = 'End'), 0)::BIGINT AS closed_entry_lamports";

const RULE_COUNTERS_SELECT: &str = "\
    l.rule_id AS rule_id, \
    {aggs}";

/// Fold the raw batched rows into the `rule_id → RuleCounters` map (win-rate +
/// capital-weighted return derived here). Shared by both `rule_counters_*` queries.
fn map_rule_counters(rows: Vec<RuleCountersRow>) -> HashMap<Uuid, RuleCounters> {
    rows.into_iter()
        .map(|r| {
            let closed = r.win + r.loss;
            let win_rate = if closed > 0 { r.win as f64 / closed as f64 * 100.0 } else { 0.0 };
            // Canonical capital-weighted return — sign-locked to `total_pnl_sol`.
            let avg_pnl_pct =
                weighted_return_pct(r.total_pnl_lamports as f64, r.closed_entry_lamports as f64);
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
        .collect()
}

/// The one "this position is closed" predicate. Every closed-scoped aggregate in
/// [`StrategyRepo::positions_summary`] refers to it, so the exit-reason counts
/// can't drift from the `closed` they're reconciled against (a count scoped even
/// slightly wider would exceed `closed` and drive the frontend's `Other` slice
/// negative).
// Realized PnL is `End`-only: an `EntryFailed` never deployed SOL (and its NULL
// entry excludes it anyway); a stuck/unconfirmed exit is OPEN, not closed.
const CLOSED_PRED: &str = "sp.entry_price IS NOT NULL AND sp.status = 'End'";

/// (`exit_reason` value → result column) for the summary's per-reason counts.
/// One list so the SQL, the row struct, and `ExitReasonCounts` stay in step.
/// Values are compile-time constants, never user input. `"Metrics"` is special-
/// cased in SQL via [`metrics_exit_sql_pred`] (also matches `{metric}{op}`).
const EXIT_REASON_COUNTS: &[(&str, &str)] = &[
    ("TakeProfit", "n_take_profit"),
    ("StopLoss", "n_stop_loss"),
    ("Metrics", "n_metrics"),
    ("Dead", "n_dead"),
    ("Manual", "n_manual"),
    ("TrailingStop", "n_trailing"),
    ("Stall", "n_stall"),
    ("TimeStop", "n_time"),
    ("LiquidityExit", "n_liquidity"),
    ("NextKill", "n_next_kill"),
];

/// SQL predicate: legacy bare `Metrics`, spaced `name op value`, or brief
/// `{name}{op}` — SSOT with [`hunter_engine::event::is_metric_exit_label`].
fn metrics_exit_sql_pred(col: &str) -> String {
    use std::collections::BTreeSet;
    let names: BTreeSet<&str> = hunter_engine::metrics::REGISTRY
        .iter()
        .flat_map(|g| g.metrics.iter().map(|m| m.name))
        .collect();
    let alt = names.into_iter().collect::<Vec<_>>().join("|");
    // Compact `stall>` OR spaced `stall > 3` (value is anything non-empty to EOL).
    format!(
        "({col} = 'Metrics' OR {col} ~ '^({alt})(>=|<=|!=|>|<|=)($| )' OR {col} ~ '^({alt}) (>=|<=|!=|>|<|=) ')"
    )
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
    /// SOL deployed (entry cost) across the **closed** positions only — the capital
    /// base for the canonical capital-weighted return (distinct from
    /// `total_entry_lamports`, which spans all entered incl. still-open positions).
    closed_entry_lamports: i64,
    sum_hold_secs: f64,
    best_pct: Option<f64>,
    worst_pct: Option<f64>,
    migrated: i64,
    n_take_profit: i64,
    n_stop_loss: i64,
    n_metrics: i64,
    n_metrics_win: i64,
    n_metrics_loss: i64,
    n_dead: i64,
    n_manual: i64,
    n_trailing: i64,
    n_stall: i64,
    n_time: i64,
    n_liquidity: i64,
    n_next_kill: i64,
    /// The still-open positions' `(mint_address, entry_price, entry_lamports)`,
    /// aggregated into JSON by the same query rather than fetched separately —
    /// open positions are bounded by the rule's concurrency cap (a handful), so
    /// this ships a tiny array instead of costing a second round-trip. Marked to
    /// the live cache price by [`StrategyRepo::positions_summary`]'s `price_of`.
    open_marks: serde_json::Value,
}

/// One still-open position's basis, for marking to the current cache price.
#[derive(serde::Deserialize)]
struct OpenMark {
    mint_address: String,
    entry_price: Option<f64>,
    entry_lamports: i64,
}

// ---------------------------------------------------------------------------
// Explicit column lists (struct order). Not `SELECT *` so a new physical
// column isn't pulled into every read and the wire contract stays decoupled.
// ---------------------------------------------------------------------------

const RUN_COLS: &str = "id, strategy_id, rule_id, mode, run_seq, status, params_snapshot, \
    max_total_tokens, started_at, finished_at";

/// Cap on `last_entry_error` (mig 0017). A `TradeError` can stringify an entire
/// RPC error payload; the column is a diagnostic, not a log sink.
const MAX_ENTRY_ERROR_LEN: usize = 300;

/// Truncate to at most `max` **chars** (never bytes — a byte slice can split a
/// UTF-8 sequence and panic), marking the cut with an ASCII ellipsis. Stays ASCII
/// because this is stored data, not a comment.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("...");
    out
}

/// The `strategy_positions` read column list, in `StrategyPositionDbRow` field
/// order (sqlx maps positionally, so order is load-bearing).
///
/// The two aliased variants below are hand-maintained copies — SQL has no way to
/// prefix a list — and `position_col_lists_stay_aliased_copies` is the no-DB guard
/// that keeps all three identical modulo the prefix. Add a column here and that
/// test tells you exactly which copy you missed.
const POSITION_COLS: &str = "id, run_id, strategy_id, rule_id, mode, mint_address, wallet, \
    token_program_id, token_account, target_price, target_token_amount, target_time, target_tx, \
    entry_price, entry_token_amount, entry_lamports, entry_time, entry_tx_signatures, \
    exit_price, exit_token_amount, exit_lamports, exit_time, exit_tx_signatures, \
    submitted_buy_signatures, status, exit_reason, origin, manual_exit, \
    exit_redrive_count, exit_parked, last_entry_error, extra, created_at, updated_at";

/// `POSITION_COLS` qualified with the `sp` alias — for the paged read that JOINs
/// `tokens` (so the server can sort/filter by token-enrichment columns too).
const POSITION_COLS_SP: &str = "sp.id, sp.run_id, sp.strategy_id, sp.rule_id, sp.mode, sp.mint_address, \
    sp.wallet, sp.token_program_id, sp.token_account, sp.target_price, sp.target_token_amount, \
    sp.target_time, sp.target_tx, sp.entry_price, sp.entry_token_amount, sp.entry_lamports, \
    sp.entry_time, sp.entry_tx_signatures, sp.exit_price, sp.exit_token_amount, sp.exit_lamports, \
    sp.exit_time, sp.exit_tx_signatures, sp.submitted_buy_signatures, sp.status, sp.exit_reason, \
    sp.origin, sp.manual_exit, sp.exit_redrive_count, sp.exit_parked, sp.last_entry_error, \
    sp.extra, sp.created_at, sp.updated_at";

/// `POSITION_COLS` qualified with the `p` alias — for reads that JOIN `wallet_dict`
/// (which also has an `id` column, so the bare list is ambiguous there).
const POSITION_COLS_P: &str = "p.id, p.run_id, p.strategy_id, p.rule_id, p.mode, p.mint_address, \
    p.wallet, p.token_program_id, p.token_account, p.target_price, p.target_token_amount, \
    p.target_time, p.target_tx, p.entry_price, p.entry_token_amount, p.entry_lamports, \
    p.entry_time, p.entry_tx_signatures, p.exit_price, p.exit_token_amount, p.exit_lamports, \
    p.exit_time, p.exit_tx_signatures, p.submitted_buy_signatures, p.status, p.exit_reason, \
    p.origin, p.manual_exit, p.exit_redrive_count, p.exit_parked, p.last_entry_error, \
    p.extra, p.created_at, p.updated_at";

// ---------------------------------------------------------------------------
// Position list query: server-side sort / filter / search (whitelisted)
// ---------------------------------------------------------------------------

/// A sort/filter/search request for the HTTP positions list, built from the
/// frontend `DataTable`'s emitted view-state. Only **whitelisted** column keys are
/// honored — see [`position_sort_sql`] / [`position_filter_sql`]; anything else is
/// dropped (never interpolated), so no user text ever reaches a SQL identifier.
/// Text values bind as parameters. Applies to the paged list + its count + the
/// summary aggregate so the pager and summary card always describe the same cohort.
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

/// Canonical SQL for the position **PnL%** column, in the same **percentage** units
/// the frontend cell shows (`Position::pnl_percentage` = `(exit-entry)/entry × 100`).
/// Single-sourced so the sort whitelist, the filter whitelist, and the displayed
/// value can't drift — a `>0` filter and the column ordering both agree with the cell
/// (the `× 100` is monotonic, so it doesn't change sort order vs. the bare ratio).
const PNL_PCT_SQL: &str = "(((sp.exit_price - sp.entry_price) / NULLIF(sp.entry_price, 0)) * 100)";

/// Canonical SQL for the position **realized SOL PnL** column, mirroring
/// `Position::pnl_sol` exactly: `exit_price × COALESCE(exit_token_amount, 0) −
/// entry_price × entry_token_amount` (price is SOL per raw token unit, counts are raw
/// units, so the product is human SOL). Single-sourced across the sort + filter
/// whitelists. NULL (→ excluded by any compare) whenever `entry_price`,
/// `entry_token_amount`, or `exit_price` is absent — matching the model's `None`.
const PNL_SOL_SQL: &str =
    "(sp.exit_price * COALESCE(sp.exit_token_amount, 0) - sp.entry_price * sp.entry_token_amount)";

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
        "mint_address" => "sp.mint_address",
        "entry_price" => "sp.entry_price",
        "entry_time" => "sp.entry_time",
        "exit_price" => "sp.exit_price",
        "exit_time" => "sp.exit_time",
        "pnl_pct" => PNL_PCT_SQL,
        "pnl_sol" => PNL_SOL_SQL,
        // The positions "Holding" column shows the raw exit-token count held.
        "holding" => "sp.exit_token_amount",
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
        "mint_address" => ("sp.mint_address", Text),
        "status" => ("sp.status", Text),
        // Null while still open — UI badges that as "Open", so filter must
        // COALESCE or typing "Open" never matches live holding rows.
        "exit_reason" => ("COALESCE(sp.exit_reason, 'Open')", Text),
        "entry_price" => ("sp.entry_price", Numeric),
        "exit_price" => ("sp.exit_price", Numeric),
        "pnl_pct" => (PNL_PCT_SQL, Numeric),
        "pnl_sol" => (PNL_SOL_SQL, Numeric),
        "holding" => ("sp.exit_token_amount", Numeric),
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
        // Set membership: `val` is a JSON array → `col = ANY($n::text[])`. Operands
        // are trimmed/non-empty and capped (backstop; the UI caps too). An empty or
        // non-array operand drops the predicate.
        (FilterKind::Text, FilterOp::In) => {
            let Some(arr) = spec.val.as_array() else { return };
            let vals: Vec<String> = arr
                .iter()
                .filter_map(as_text)
                .take(MAX_FILTER_IN_VALUES)
                .collect();
            if vals.is_empty() {
                return;
            }
            qb.push(" AND ").push(col).push(" = ANY(").push_bind(vals).push("::text[])");
        }
        // A numeric op on a text column is meaningless → drop.
        (FilterKind::Text, _) => {}

        // -- Numeric columns: numeric comparisons -------------------------------
        // `In` is a text-only set op; on a numeric column it's a no-op.
        (FilterKind::Numeric, FilterOp::In) => {}
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
                FilterOp::Neq => "!=",
                FilterOp::Gt => ">",
                FilterOp::Gte => ">=",
                FilterOp::Lt => "<",
                FilterOp::Lte => "<=",
                // `Contains` on a numeric col is treated as equality (a bare
                // number typed into a numeric column filter).
                FilterOp::Contains => "=",
                FilterOp::Between => unreachable!("handled above"),
                FilterOp::In => unreachable!("handled by the numeric In arm above"),
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
        qb.push(" AND (sp.mint_address ILIKE ")
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
/// resolves to `t.mint_address` (vs positions' `sp.mint_address`); everything else falls
/// through to the shared [`enrich_filter_sql`][crate::storage::token_enrichment::enrich_filter_sql]
/// SSOT. `None` = not filterable.
fn token_filter_sql(key: &str) -> Option<(&'static str, FilterKind)> {
    match key {
        "mint_address" => Some(("t.mint_address", FilterKind::Text)),
        _ => enrich_filter_sql(key),
    }
}

/// Token-scoped sort whitelist — the `mint → t.mint_address` alias plus the shared
/// [`enrich_sort_sql`][crate::storage::token_enrichment::enrich_sort_sql] SSOT.
fn token_sort_sql(key: &str) -> Option<&'static str> {
    match key {
        "mint_address" => Some("t.mint_address"),
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
    /// (paper fill-recovery / per-signature attribution paths).
    pub fn pool(&self) -> &PgPool {
        &self.pool
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

    /// Delete empty `Running` shells that sit ahead of an older still-`Running`
    /// bag for the same `(rule_id, mode)`.
    ///
    /// Pre-fix `Sink::ensure_run` minted a new run on every cold cache miss
    /// (process restart / warm), so paper scoreboard joins on
    /// `DISTINCT ON (rule_id) … ORDER BY run_seq DESC` saw an empty latest run
    /// while positions lived under prior `run_id`s. Safe vs a future
    /// finish-on-stop → new-run-on-activate lifecycle: those older siblings are
    /// terminal (`Finished`/`Stopped`), so this DELETE does not touch them.
    pub async fn delete_empty_leading_runs(
        &self,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<u64> {
        let res = sqlx::query(
            "DELETE FROM strategy_runs r \
             WHERE r.rule_id = $1 AND r.mode = $2 AND r.status = 'Running' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM strategy_positions p WHERE p.run_id = r.id \
             ) \
             AND EXISTS ( \
                 SELECT 1 FROM strategy_runs older \
                 WHERE older.rule_id = r.rule_id AND older.mode = r.mode \
                   AND older.run_seq < r.run_seq \
                   AND older.status = 'Running' \
                   AND EXISTS ( \
                       SELECT 1 FROM strategy_positions p2 \
                       WHERE p2.run_id = older.id \
                   ) \
             )",
        )
        .bind(rule_id)
        .bind(mode)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
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

    /// One run by `(rule_id, mode, run_seq)` — Evidence pane "Run #N" scope.
    pub async fn find_run_by_seq(
        &self,
        rule_id: Uuid,
        mode: &str,
        run_seq: i64,
    ) -> anyhow::Result<Option<StrategyRun>> {
        let row = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT {RUN_COLS} FROM strategy_runs \
             WHERE rule_id = $1 AND mode = $2 AND run_seq = $3"
        ))
        .bind(rule_id)
        .bind(mode)
        .bind(run_seq)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRun::from))
    }

    /// Runs for a rule (newest first) with optional finalized
    /// [`strategy_run_metrics`](crate::models::StrategyRunMetrics) (LEFT JOIN — null
    /// while Running / never rolled up). Backs `GET …/strategy-rules/{id}/runs`.
    pub async fn list_runs_with_metrics(
        &self,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<Vec<RuleRunListRow>> {
        let rows = sqlx::query_as::<_, RuleRunListDbRow>(
            r#"
            SELECT r.id, r.run_seq, r.status, r.mode, r.started_at, r.finished_at,
                   m.n_closed, m.n_open, m.win_rate, m.total_pnl_sol, m.expectancy_sol,
                   m.n_exit_take_profit, m.n_exit_stop_loss, m.n_exit_trailing,
                   m.n_exit_stall, m.n_exit_time, m.n_exit_liquidity
            FROM strategy_runs r
            LEFT JOIN strategy_run_metrics m ON m.run_id = r.id
            WHERE r.rule_id = $1 AND r.mode = $2
            ORDER BY r.run_seq DESC
            "#,
        )
        .bind(rule_id)
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(RuleRunListRow::from).collect())
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

    // -- Positions ------------------------------------------------------------

    pub async fn insert_position(&self, p: &StrategyPosition) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_positions
                (id, run_id, strategy_id, rule_id, mode, mint_address, wallet, token_program_id,
                 target_price, target_token_amount, target_time, target_tx,
                 entry_price, entry_token_amount, entry_lamports, entry_time, entry_tx_signatures,
                 exit_price, exit_token_amount, exit_lamports, exit_time, exit_tx_signatures,
                 submitted_buy_signatures, status, exit_reason, origin, manual_exit, extra,
                 created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint_address)
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
        .bind(&p.origin)
        .bind(p.manual_exit.as_ref().map(Json))
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
                mint_address = $6,
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
                origin = $26,
                manual_exit = $27,
                extra = $28,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint_address)
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
        .bind(&p.origin)
        .bind(p.manual_exit.as_ref().map(Json))
        .bind(Json(&p.extra))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The one manual-position run (`strategy_id='manual'`, `rule_id` NULL, mode
    /// real) — find the running one or mint it. Manual positions all hang off
    /// this run so `strategy_runs`' NOT NULL FK is satisfied without a fake rule.
    pub async fn ensure_manual_run(&self) -> anyhow::Result<Uuid> {
        if let Some(id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM strategy_runs \
             WHERE strategy_id = 'manual' AND mode = 'real' AND status = 'Running' \
             ORDER BY started_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(id);
        }
        let run = StrategyRun {
            id: Uuid::new_v4(),
            strategy_id: "manual".to_string(),
            rule_id: None,
            mode: "real".to_string(),
            run_seq: 1,
            status: "Running".to_string(),
            params_snapshot: serde_json::json!({}),
            max_total_tokens: None,
            started_at: Utc::now(),
            finished_at: None,
        };
        self.insert_run(&run).await?;
        Ok(run.id)
    }

    /// Whether any OPEN (not `End`/`EntryFailed`) real position exists on `mint`
    /// — the manual-buy one-position-per-mint guard.
    pub async fn has_open_real_position_on_mint(&self, mint: &str) -> anyhow::Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM strategy_positions \
                 WHERE mint_address = $1 AND mode = 'real' \
                   AND status NOT IN ('End','EntryFailed') \
             )",
        )
        .bind(mint)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Persist a manual position's TP/SL config (`NULL` clears → tracked-only).
    /// Manual-origin rows only; returns rows affected so the handler can 404/409.
    pub async fn set_manual_exit(
        &self,
        id: Uuid,
        manual_exit: Option<&Value>,
    ) -> anyhow::Result<u64> {
        let res = sqlx::query(
            "UPDATE strategy_positions SET manual_exit = $2, updated_at = now() \
             WHERE id = $1 AND origin = 'manual'",
        )
        .bind(id)
        .bind(manual_exit.map(Json))
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
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

    /// Durably flip a live position to a transient status (`ExitPending`) in a
    /// single round-trip, without rewriting the whole row. Used on the exit path so
    /// the "selling" state is committed BEFORE the sell is dispatched — a mid-sell
    /// crash then leaves the row as `ExitPending` (reaper re-drives the sell), not
    /// `Holding` (boot-adopt would re-enter it and double-sell). Terminal rows are
    /// never resurrected: the `WHERE` refuses to overwrite a closed status.
    pub async fn mark_status(
        &self,
        id: Uuid,
        status: &str,
        exit_reason: Option<&str>,
    ) -> anyhow::Result<u64> {
        let res = sqlx::query(
            "UPDATE strategy_positions \
             SET status = $2, \
                 exit_reason = COALESCE($3, exit_reason), \
                 updated_at = now() \
             WHERE id = $1 AND status NOT IN ('End','EntryFailed','ExitStuck','ExitUnconfirmed')",
        )
        .bind(id)
        .bind(status)
        .bind(exit_reason)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
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
             LEFT JOIN tokens t ON t.mint_address = sp.mint_address \
             LEFT JOIN tokens_info i ON i.mint_address = sp.mint_address WHERE "
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
             LEFT JOIN tokens t ON t.mint_address = sp.mint_address \
             LEFT JOIN tokens_info i ON i.mint_address = sp.mint_address WHERE ",
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
    /// `price_of` resolves a mint's current price for the open mark — see
    /// [`positions_summary`](Self::positions_summary).
    pub async fn positions_summary_by_run(
        &self,
        run_id: Uuid,
        query: &PositionQuery,
        price_of: impl Fn(&str) -> Option<f64>,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("sp.run_id", run_id, None, query, price_of).await
    }

    /// Rule-wide position aggregates across all runs (real-rule lifetime history).
    pub async fn positions_summary_by_rule(
        &self,
        rule_id: Uuid,
        query: &PositionQuery,
        price_of: impl Fn(&str) -> Option<f64>,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("sp.rule_id", rule_id, None, query, price_of).await
    }

    /// Rule-wide aggregates across all runs except one (the run-history view) —
    /// complements [`positions_summary_by_run`] on the latest run.
    pub async fn positions_summary_by_rule_excluding_run(
        &self,
        rule_id: Uuid,
        exclude_run_id: Uuid,
        query: &PositionQuery,
        price_of: impl Fn(&str) -> Option<f64>,
    ) -> anyhow::Result<PositionsSummary> {
        self.positions_summary("sp.rule_id", rule_id, Some(exclude_run_id), query, price_of).await
    }

    /// Shared aggregate query behind the summary views. `scope_col` is a trusted
    /// literal (`"sp.run_id"` / `"sp.rule_id"`), never user input — no injection
    /// surface. `exclude_run` (when set) drops that run from the scope (run-history).
    /// LEFT-JOINs `tokens` so `query`'s search/filters match the paged list exactly.
    /// All aggregation happens in Postgres (`COUNT/SUM FILTER`), so no rows
    /// are shipped. The win rule mirrors [`StrategyPosition::is_win`]: a clean `End`
    /// exit with positive realized SOL (`exit_lamports > entry_lamports`); every other closed
    /// position is a loss. SOL columns are lamports → divided to human SOL here.
    ///
    /// `price_of` resolves a mint's current price so the still-open positions can be
    /// marked to market into `open_pnl_sol`. It is a caller-supplied closure rather
    /// than a cache handle because the price lives in the live runtime cache, not in
    /// Postgres — this keeps the repo layer free of a dependency on it (the `lab`
    /// bin, which has no such cache, passes `|_| None` and gets a `0.0` mark).
    async fn positions_summary(
        &self,
        scope_col: &str,
        scope_id: Uuid,
        exclude_run: Option<Uuid>,
        query: &PositionQuery,
        price_of: impl Fn(&str) -> Option<f64>,
    ) -> anyhow::Result<PositionsSummary> {
        // Predicates (kept in one place so the two summary views can't drift):
        //   entered  := entry_price IS NOT NULL
        //   open     := status NOT IN ('End','EntryFailed')   (the open partition —
        //               a stuck/unconfirmed exit still holds deployed SOL)
        //   closed   := entry_price IS NOT NULL AND status = 'End'
        //   win      := status = 'End' AND exit_lamports > entry_lamports   (SOL basis)
        // Realized SOL PnL = COALESCE(exit_lamports,0) - entry_lamports (lamports),
        // `End`-only: an `EntryFailed` never deployed SOL, and a stuck bag is open
        // (unrealized) until written off or sold. The headline return % is
        // capital-weighted (Σ pnl / Σ entry, via `weighted_return_pct`), so it is
        // sign-locked to the SOL total; `best_pct`/`worst_pct` are per-trade price
        // extremes for the distribution tails.
        // One `COUNT(*) FILTER` per known exit reason, generated from the single
        // `EXIT_REASON_COUNTS` list so a new reason is added in exactly one place.
        let metrics_pred = metrics_exit_sql_pred("sp.exit_reason");
        let exit_cols: String = EXIT_REASON_COUNTS
            .iter()
            .map(|(reason, col)| {
                let pred = if *reason == "Metrics" {
                    metrics_pred.clone()
                } else {
                    format!("sp.exit_reason = '{reason}'")
                };
                format!("COUNT(*) FILTER (WHERE {CLOSED_PRED} AND {pred}) AS {col}, ")
            })
            .collect();
        // Metric exits further split by the same win predicate as `win` / `is_win`
        // so the summary bar can show Metric+ / Metric- (PnL outcome) without a
        // second pass. `n_metrics` remains the total; win + loss must equal it.
        let metrics_split_cols = format!(
            "COUNT(*) FILTER (WHERE {CLOSED_PRED} AND {metrics_pred} \
                AND sp.status = 'End' AND sp.exit_lamports > sp.entry_lamports) AS n_metrics_win, \
             COUNT(*) FILTER (WHERE {CLOSED_PRED} AND {metrics_pred} \
                AND NOT (sp.status = 'End' AND sp.exit_lamports > sp.entry_lamports)) AS n_metrics_loss, "
        );
        let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
            "SELECT \
               COUNT(*) FILTER (WHERE sp.entry_price IS NOT NULL) AS tokens, \
               COUNT(*) FILTER (WHERE sp.status NOT IN ('End','EntryFailed')) AS open, \
               COUNT(*) FILTER (WHERE sp.status = 'End' AND sp.exit_lamports > sp.entry_lamports) AS win, \
               COUNT(*) FILTER (WHERE sp.entry_price IS NOT NULL AND sp.status = 'End' \
                                  AND NOT (sp.exit_lamports > sp.entry_lamports)) AS loss, \
               COALESCE(SUM(COALESCE(sp.exit_lamports,0) - sp.entry_lamports) \
                        FILTER (WHERE sp.entry_price IS NOT NULL AND sp.status = 'End'), 0)::BIGINT AS total_pnl_lamports, \
               COALESCE(SUM(sp.entry_lamports) FILTER (WHERE sp.entry_price IS NOT NULL), 0)::BIGINT AS total_entry_lamports, \
               COALESCE(SUM(sp.entry_lamports) FILTER (WHERE sp.entry_price IS NOT NULL \
                                  AND sp.status NOT IN ('End','EntryFailed')), 0)::BIGINT AS total_holding_lamports, \
               COALESCE(SUM(COALESCE(sp.exit_lamports,0) - sp.entry_lamports) \
                        FILTER (WHERE sp.status = 'End' AND sp.exit_lamports > sp.entry_lamports), 0)::BIGINT AS total_gains_lamports, \
               COALESCE(SUM(sp.entry_lamports - COALESCE(sp.exit_lamports,0)) \
                        FILTER (WHERE sp.entry_price IS NOT NULL AND sp.status = 'End' \
                                  AND NOT (sp.exit_lamports > sp.entry_lamports)), 0)::BIGINT AS total_losses_lamports, \
               COALESCE(SUM(sp.entry_lamports) \
                        FILTER (WHERE sp.entry_price IS NOT NULL AND sp.status = 'End'), 0)::BIGINT AS closed_entry_lamports, \
               COALESCE(SUM(EXTRACT(EPOCH FROM (sp.exit_time - sp.entry_time))) \
                        FILTER (WHERE sp.entry_time IS NOT NULL AND sp.exit_time IS NOT NULL \
                                  AND sp.status = 'End'), 0)::DOUBLE PRECISION AS sum_hold_secs, \
               MAX((sp.exit_price - sp.entry_price) / sp.entry_price * 100.0) \
                        FILTER (WHERE sp.entry_price IS NOT NULL AND sp.entry_price > 0 \
                                  AND sp.status = 'End')::DOUBLE PRECISION AS best_pct, \
               MIN((sp.exit_price - sp.entry_price) / sp.entry_price * 100.0) \
                        FILTER (WHERE sp.entry_price IS NOT NULL AND sp.entry_price > 0 \
                                  AND sp.status = 'End')::DOUBLE PRECISION AS worst_pct, \
               COUNT(*) FILTER (WHERE sp.entry_price IS NOT NULL AND i.is_migrated) AS migrated, \
               {exit_cols}\
               {metrics_split_cols}\
               COALESCE(json_agg(json_build_object( \
                          'mint_address', sp.mint_address, \
                          'entry_price', sp.entry_price, \
                          'entry_lamports', sp.entry_lamports)) \
                        FILTER (WHERE sp.entry_price IS NOT NULL \
                                  AND sp.status NOT IN ('End','EntryFailed')), '[]') AS open_marks \
             FROM strategy_positions sp \
             LEFT JOIN tokens t ON t.mint_address = sp.mint_address \
             LEFT JOIN tokens_info i ON i.mint_address = sp.mint_address WHERE "
        ));
        qb.push(scope_col).push(" = ").push_bind(scope_id);
        if let Some(exclude) = exclude_run {
            qb.push(" AND sp.run_id <> ").push_bind(exclude);
        }
        push_position_where(&mut qb, query);
        let row: PositionsSummaryRow = qb.build_query_as().fetch_one(&self.pool).await?;

        let closed = row.win + row.loss;
        let win_rate = if closed > 0 { row.win as f64 / closed as f64 * 100.0 } else { 0.0 };
        // Canonical capital-weighted return (lamports ratio is scale-invariant), so
        // this figure's sign is locked to `total_pnl_sol`.
        let avg_pnl_pct =
            weighted_return_pct(row.total_pnl_lamports as f64, row.closed_entry_lamports as f64);
        let avg_hold_secs = if closed > 0 { row.sum_hold_secs / closed as f64 } else { 0.0 };

        // Mark the open positions to the caller-supplied current price, pricing the
        // hypothetical round-trip through the **same** `CostModel` the sim and the
        // sweep use — so a live rule's unrealized figure is directly comparable to a
        // backtest's `open_pnl_sol` instead of being a raw price delta. A position
        // whose token has no cached price yet (just entered, no post-entry trade)
        // contributes nothing rather than a fabricated 0-price loss.
        let open_pnl_sol = serde_json::from_value::<Vec<OpenMark>>(row.open_marks)
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let entry_price = m.entry_price.filter(|p| *p > 0.0)?;
                let current = price_of(&m.mint_address).filter(|p| p.is_finite() && *p > 0.0)?;
                let notional = lamports_to_sol(m.entry_lamports);
                // `pumpfun_default` is depth-blind, so no reserve is threaded here —
                // the open-mark row has no pool snapshot to supply one honestly.
                let (pnl_sol, _) = round_trip_with_costs(
                    entry_price,
                    current,
                    notional,
                    None,
                    &CostModel::pumpfun_default(),
                );
                Some(pnl_sol)
            })
            .sum();

        Ok(PositionsSummary {
            tokens: row.tokens,
            open: row.open,
            win: row.win,
            loss: row.loss,
            closed,
            win_rate,
            avg_pnl_pct,
            total_pnl_sol: lamports_to_sol(row.total_pnl_lamports),
            open_pnl_sol,
            total_entry_sol: lamports_to_sol(row.total_entry_lamports),
            total_holding_sol: lamports_to_sol(row.total_holding_lamports),
            total_gains_sol: lamports_to_sol(row.total_gains_lamports),
            total_losses_sol: lamports_to_sol(row.total_losses_lamports),
            avg_hold_secs,
            best_pct: row.best_pct,
            worst_pct: row.worst_pct,
            migrated: row.migrated,
            exits: ExitReasonCounts {
                take_profit: row.n_take_profit,
                stop_loss: row.n_stop_loss,
                metrics: row.n_metrics,
                metrics_win: row.n_metrics_win,
                metrics_loss: row.n_metrics_loss,
                dead: row.n_dead,
                manual: row.n_manual,
                trailing: row.n_trailing,
                stall: row.n_stall,
                time: row.n_time,
                liquidity: row.n_liquidity,
                next_kill: row.n_next_kill,
            },
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
        self.rule_counters_for_latest_runs(strategy_id, "paper").await
    }

    /// Latest-run counters for every rule in `mode` (`paper` or `real`). Shared by
    /// the paper scoreboard and the live Rules Control `score_scope=current` chip.
    pub async fn rule_counters_for_latest_runs(
        &self,
        strategy_id: &str,
        mode: &str,
    ) -> anyhow::Result<HashMap<Uuid, RuleCounters>> {
        let select = RULE_COUNTERS_SELECT.replace("{aggs}", RULE_COUNTERS_AGGS);
        let sql = format!(
            "WITH latest AS ( \
                SELECT DISTINCT ON (rule_id) rule_id, id AS run_id \
                FROM strategy_runs \
                WHERE mode = $2 AND rule_id IS NOT NULL AND strategy_id = $1 \
                ORDER BY rule_id, run_seq DESC \
            ) \
            SELECT {select} \
            FROM latest l \
            LEFT JOIN strategy_positions p ON p.run_id = l.run_id \
            GROUP BY l.rule_id"
        );
        let rows = sqlx::query_as::<_, RuleCountersRow>(&sql)
            .bind(strategy_id)
            .bind(mode)
            .fetch_all(&self.pool)
            .await?;
        Ok(map_rule_counters(rows))
    }

    /// All-time per-rule counters for **real** positions — Rules scoreboard (live).
    /// Paper rules use [`rule_counters_for_latest_paper_runs`] (current run only).
    pub async fn rule_counters_for_all_real(&self) -> anyhow::Result<HashMap<Uuid, RuleCounters>> {
        let sql = format!(
            "SELECT p.rule_id AS rule_id, {RULE_COUNTERS_AGGS} \
             FROM strategy_positions p \
             WHERE p.mode = 'real' AND p.rule_id IS NOT NULL \
             GROUP BY p.rule_id"
        );
        let rows = sqlx::query_as::<_, RuleCountersRow>(&sql)
            .fetch_all(&self.pool)
            .await?;
        Ok(map_rule_counters(rows))
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

    /// Page-bounded in-holding-index positions (`BuySubmitted`/`Holding`)
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
             WHERE strategy_id = $1 AND mint_address = $2 \
               AND status IN ('Holding', 'BuySubmitted') \
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
               AND status IN ('Holding', 'BuySubmitted') \
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
             WHERE status NOT IN ('End', 'EntryFailed') ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Most recent terminal closes (`End` / `EntryFailed`) across all rules —
    /// Recent hydrate. A stuck/unconfirmed exit is OPEN and never lands here.
    /// Ordered by exit time (fallback `updated_at`), newest first. Bounded.
    pub async fn find_recent_closed(
        &self,
        limit: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let limit = limit.clamp(1, 200);
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status IN ('End', 'EntryFailed') \
             ORDER BY COALESCE(exit_time, updated_at) DESC \
             LIMIT $1"
        ))
        .bind(limit)
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
            // Positions reference a generic-engine rule (`strategy_rules`); the name
            // rides along via a LEFT JOIN (null if the rule was since deleted).
            "SELECT p.mint_address, p.rule_id, r.rule_name, p.status, p.mode \
             FROM strategy_positions p \
             LEFT JOIN strategy_rules r ON r.id = p.rule_id \
             WHERE p.status NOT IN ('End', 'EntryFailed') \
               AND ($1 = false OR p.mode = 'real') \
             ORDER BY p.created_at DESC",
        )
        .bind(real_only)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(mint, rule_id, rule_name, status, mode)| ManagedMint {
                mint_address: mint,
                rule_id,
                rule_name,
                status,
                mode,
            })
            .collect())
    }

    /// Realized SOL PnL (lamports) from **real** positions that cleanly exited
    /// (`status='End'`) on/after `since` — the "realized today" KPI (pass 00:00
    /// UTC). Same `exit_lamports − entry_lamports` basis as [`Self::positions_summary`].
    /// A failed exit realized nothing bookable, so `End`-only.
    pub async fn realized_pnl_lamports_since(
        &self,
        since: DateTime<Utc>,
    ) -> anyhow::Result<i64> {
        let sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(exit_lamports - entry_lamports), 0)::BIGINT \
             FROM strategy_positions \
             WHERE mode = 'real' AND status = 'End' \
               AND exit_lamports IS NOT NULL AND entry_lamports IS NOT NULL \
               AND exit_time >= $1",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum)
    }

    /// Cross-rule closed-trade rollup for the Portfolio page — one row per rule
    /// with closes in the window. `since = None` ⇒ all-time. Win = clean `End`
    /// with positive realized SOL (same predicate as [`RuleCounters`]).
    pub async fn portfolio_pnl_by_rule(
        &self,
        mode: &str,
        since: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<RulePeriodPnlRow>> {
        let rows = sqlx::query_as::<_, RulePeriodPnlDbRow>(
            r#"
            SELECT p.rule_id AS rule_id,
                   r.rule_name AS rule_name,
                   COUNT(*) FILTER (
                     WHERE p.entry_lamports IS NOT NULL
                       AND p.exit_lamports IS NOT NULL
                   )::BIGINT AS closed,
                   COUNT(*) FILTER (
                     WHERE p.entry_lamports IS NOT NULL
                       AND p.exit_lamports IS NOT NULL
                       AND (p.exit_lamports - p.entry_lamports) > 0
                   )::BIGINT AS win,
                   COUNT(*) FILTER (
                     WHERE p.entry_lamports IS NOT NULL
                       AND p.exit_lamports IS NOT NULL
                       AND (p.exit_lamports - p.entry_lamports) <= 0
                   )::BIGINT AS loss,
                   COALESCE(SUM(p.exit_lamports - p.entry_lamports) FILTER (
                     WHERE p.entry_lamports IS NOT NULL
                       AND p.exit_lamports IS NOT NULL
                   ), 0)::BIGINT AS total_pnl_lamports,
                   COALESCE(SUM(p.entry_lamports) FILTER (
                     WHERE p.entry_lamports IS NOT NULL
                       AND p.exit_lamports IS NOT NULL
                   ), 0)::BIGINT AS total_entry_lamports
            FROM strategy_positions p
            LEFT JOIN strategy_rules r ON r.id = p.rule_id
            WHERE p.mode = $1
              AND p.rule_id IS NOT NULL
              AND p.status = 'End'
              AND ($2::timestamptz IS NULL OR p.exit_time >= $2)
            GROUP BY p.rule_id, r.rule_name
            HAVING COUNT(*) FILTER (
              WHERE p.entry_lamports IS NOT NULL
                AND p.exit_lamports IS NOT NULL
            ) > 0
            ORDER BY total_pnl_lamports DESC
            "#,
        )
        .bind(mode)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(RulePeriodPnlRow::from).collect())
    }

    /// Distinct mints with an unsettled **real** position — every mint the wallet
    /// could legitimately hold a bag for. Covers `Holding`/`BuySubmitted`/
    /// `ExitPending`/`ExitStuck`/`ExitUnconfirmed` (the last two can still hold a
    /// bag whose sell failed / never confirmed), across all strategies. Backs the
    /// boot wallet-reconcile sweep. Positive `IN` over the unsettled statuses so
    /// the predicate stays index-servable.
    pub async fn distinct_unsettled_real_mints(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT mint_address FROM strategy_positions \
             WHERE mode = 'real' \
               AND status IN ('Holding', 'BuySubmitted', 'ExitPending', \
                              'ExitStuck', 'ExitUnconfirmed')",
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
    /// Persist the trigger-trade (`target_*`) snapshot that armed a paper/sim
    /// position — distinct from the worst-case `entry_*` fill. No-op fields stay
    /// null when the caller has no trigger (legacy rows).
    pub async fn record_target(
        &self,
        id: Uuid,
        price: f64,
        token_amount: u64,
        time: DateTime<Utc>,
        tx: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE strategy_positions \
             SET target_price = $2, target_token_amount = $3, target_time = $4, \
                 target_tx = $5, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(price)
        .bind(token_amount as i64)
        .bind(time)
        .bind(tx)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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
             WHERE mint_address = $1 AND mode = $2 AND status = 'Holding' AND entry_price IS NOT NULL"
        ))
        .bind(mint)
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// All `Holding` rows with an entry fill for a mode — boot registry adopt
    /// (PG-only). Real adopts resume TP/SL/Dead + Ops close; paper adopts resume
    /// the simulated exit so a restart doesn't strand paper bags as forever-`Open`.
    pub async fn find_all_holding(&self, mode: &str) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE mode = $1 AND status = 'Holding' AND entry_price IS NOT NULL \
             ORDER BY created_at ASC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Terminal-position counts per (rule, mint) over the given mints — the boot
    /// seed for the re-entry episode cap (plan Ph4): after a restart the engine's
    /// in-RAM episode counters are gone, and without this a re-entry rule would
    /// restart a token's episode budget from zero and over-trade it. One batched
    /// indexed query (`idx_strategy_positions_mint_address_status`), boot-only.
    pub async fn count_closed_by_rule_mint(
        &self,
        mints: &[String],
    ) -> anyhow::Result<Vec<(Uuid, String, i64)>> {
        if mints.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
            "SELECT rule_id, mint_address, COUNT(*) FROM strategy_positions \
             WHERE mint_address = ANY($1) AND rule_id IS NOT NULL \
               AND status IN ('End','EntryFailed','ExitStuck','ExitUnconfirmed') \
             GROUP BY rule_id, mint_address",
        )
        .bind(mints)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Delete stale **paper** `BuySubmitted` rows that never recorded an entry fill
    /// (`entry_price IS NULL`) past `stale_after`. Paper buys are a synchronous
    /// simulation with no on-chain tokens, so a crash mid-buy leaves an
    /// unrecoverable never-filled row — safe to drop (unlike a real `BuySubmitted`,
    /// which may own an on-chain bag and is the buy-recovery reaper's job). Returns
    /// rows deleted.
    pub async fn delete_stale_paper_buy_submitted(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            "DELETE FROM strategy_positions \
             WHERE status = 'BuySubmitted' AND mode = 'paper' \
               AND entry_price IS NULL AND created_at < $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Unsettled real rows on `(wallet, mint)` excluding `exclude_id` — siblings that
    /// may still share the ATA after a leader sell clears the bag.
    pub async fn find_unsettled_real_on_mint(
        &self,
        wallet: &str,
        mint: &str,
        exclude_id: Uuid,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE wallet = $1 AND mint_address = $2 AND mode = 'real' AND id <> $3 \
               AND status IN ('Holding','ExitPending','ExitStuck','ExitUnconfirmed') \
               AND entry_price IS NOT NULL \
             ORDER BY created_at ASC"
        ))
        .bind(wallet)
        .bind(mint)
        .bind(exclude_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Real `ExitStuck` rows whose wallet still shows a positive `trades` net on
    /// the mint (bag stranded). PG-only — no RPC. `threshold_raw` is the cleared
    /// floor (typically 0).
    pub async fn find_exit_stuck_bags(
        &self,
        threshold_raw: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        self.find_bags_by_status("ExitStuck", threshold_raw).await
    }

    /// Real, un-parked rows in `status` whose wallet still shows a positive
    /// `trades` net on the mint — i.e. the bag is still held. PG-only, no RPC.
    ///
    /// The ONE stranded-bag query: `ExitStuck` and `ExitUnconfirmed` both need it
    /// and their "is there still a bag" definition must not drift apart.
    pub async fn find_bags_by_status(
        &self,
        status: &str,
        threshold_raw: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            r#"
            SELECT {POSITION_COLS_P}
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = $2 AND p.entry_price IS NOT NULL
              AND NOT p.exit_parked
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint_address
                  ), 0)::bigint > $1
            ORDER BY p.updated_at ASC
            "#
        ))
        .bind(threshold_raw)
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Re-point an open position at the token account that actually holds its bag.
    ///
    /// Needed when the account recorded at entry is not where the tokens are —
    /// historically caused by two concurrent same-mint buys overwriting one
    /// per-mint cache entry, which left one position's sell aimed at an account
    /// that had been drained by its sibling. The buy path now records the funded
    /// account directly, so this is the *recovery* half: rows written before that
    /// fix (and any future divergence) can be repaired from on-chain truth.
    pub async fn set_token_account(&self, id: Uuid, token_account: &str) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE strategy_positions SET token_account = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(token_account)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Record why the most recent buy attempt did not fill — the ONE writer of
    /// `last_entry_error` (mig 0017). Called by the real-exec adapter at each
    /// give-up / retry point, where the `TradeError` or the Anchor custom code is
    /// still in hand; without it an `EntryFailed` row explains nothing (the engine
    /// has no `ExitReason` for a position that never opened) and the cause is only
    /// recoverable from container logs on the box.
    ///
    /// A dedicated column, not `extra`, for the same reason as
    /// [`Self::bump_exit_redrive`]: the sink's full-row `update_position` booking
    /// the terminal status lands *after* this write and would clobber it.
    ///
    /// `cause` is truncated to `MAX_ENTRY_ERROR_LEN` — a `TradeError` can carry a
    /// whole RPC payload and this column is a diagnostic, not a log.
    ///
    /// Returns `false` when no row matched. That is a real case, not an error: the
    /// row is inserted asynchronously (Pass-1), so a buy that fails *before* any
    /// network I/O — the already-migrated skip — can run ahead of its own insert.
    /// The caller retries briefly on `false`, exactly as `mark_buy_submitted` does.
    pub async fn note_last_entry_error(&self, id: Uuid, cause: &str) -> anyhow::Result<bool> {
        let cause = truncate_chars(cause, MAX_ENTRY_ERROR_LEN);
        let res = sqlx::query(
            "UPDATE strategy_positions SET last_entry_error = $2, updated_at = now() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(cause)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Atomically increment the reaper's ExitStuck redrive counter, returning the
    /// new count. A dedicated column (not `extra`) so the fold's full-row
    /// `update_position` re-booking ExitStuck after a failed redrive can never
    /// clobber it. See [`Self::set_exit_parked`] / migration 0012.
    pub async fn bump_exit_redrive(&self, id: Uuid) -> anyhow::Result<i32> {
        let (n,): (i32,) = sqlx::query_as(
            "UPDATE strategy_positions SET exit_redrive_count = exit_redrive_count + 1 \
             WHERE id = $1 RETURNING exit_redrive_count",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    /// Park an ExitStuck position. Parked ⇒ the reaper stops auto-redriving it
    /// (retry cap hit); it stays ExitStuck (open, attention lane) surfaced for a
    /// manual decision (retry / force-dump / write-off). Only a manual action or a
    /// bag-cleared heal resolves it from here.
    pub async fn set_exit_parked(&self, id: Uuid, parked: bool) -> anyhow::Result<()> {
        sqlx::query("UPDATE strategy_positions SET exit_parked = $2 WHERE id = $1")
            .bind(id)
            .bind(parked)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Un-park after a manual Retry: clears the parked flag AND resets the redrive
    /// counter so the position gets a fresh bounded auto-redrive budget.
    pub async fn unpark_exit(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE strategy_positions SET exit_parked = false, exit_redrive_count = 0 \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Real rows in `status` whose wallet mint net is already ≤ `threshold_raw`
    /// (bag gone — book End without a sell). Requires a sell on record so a
    /// rolling-buffer age-out of the buy alone cannot false-clear. PG-only.
    /// Drives the reaper's bag-cleared heals for `ExitStuck` and
    /// `ExitUnconfirmed` (the on-demand Verify action reuses the same shape).
    pub async fn find_cleared_by_status(
        &self,
        status: &str,
        threshold_raw: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            r#"
            SELECT {POSITION_COLS_P}
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = $2 AND p.entry_price IS NOT NULL
              AND EXISTS (
                    SELECT 1 FROM trades s
                    WHERE s.wallet_id = w.id AND s.mint_address = p.mint_address
                      AND s.trade_type = 'sell'
                  )
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint_address
                  ), 0)::bigint <= $1
            ORDER BY p.updated_at ASC
            "#
        ))
        .bind(threshold_raw)
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Real `ExitPending` whose wallet mint net is already ≤ `threshold_raw` (the
    /// in-flight sell already cleared the bag — book End without RE-selling). Same
    /// shape as [`find_exit_failed_cleared`]: requires a sell on record so a
    /// rolling-buffer age-out of the buy alone cannot false-clear. Drives the
    /// reaper's cleared-ExitPending heal, which must run BEFORE the ExitPending
    /// re-drive so a gone bag is booked instead of triggering a phantom sell.
    pub async fn find_exit_pending_cleared(
        &self,
        threshold_raw: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            r#"
            SELECT {POSITION_COLS_P}
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = 'ExitPending' AND p.entry_price IS NOT NULL
              AND EXISTS (
                    SELECT 1 FROM trades s
                    WHERE s.wallet_id = w.id AND s.mint_address = p.mint_address
                      AND s.trade_type = 'sell'
                  )
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint_address
                  ), 0)::bigint <= $1
            ORDER BY p.updated_at ASC
            "#
        ))
        .bind(threshold_raw)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// The persisted `token_account` for any open position on the same
    /// `(wallet, mint)` in a mode — so a subsequent bot buy into a mint already
    /// held reuses that one account instead of the template pool minting a second.
    /// Scans `BuySubmitted`/`Holding`/`ExitPending`/`ExitStuck` rows (any
    /// in-flight, held, or stuck-bag position), newest first, returning the first
    /// non-null account. `None` = nothing held with a recorded account, so the buy
    /// proceeds as today.
    pub async fn find_reusable_token_account(
        &self,
        wallet: &str,
        mint: &str,
        mode: &str,
    ) -> anyhow::Result<Option<String>> {
        let acct: Option<String> = sqlx::query_scalar(
            "SELECT token_account FROM strategy_positions \
             WHERE wallet = $1 AND mint_address = $2 AND mode = $3 \
               AND status IN ('BuySubmitted','Holding','ExitPending','ExitStuck') \
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

    /// True if any OTHER position (excluding `exclude_id`) on this `(wallet, mint)` in
    /// `mode` is still unsettled — `BuySubmitted`/`Holding`/`ExitPending`/
    /// `ExitStuck`/`ExitUnconfirmed` — i.e. may still hold tokens in the SHARED
    /// token account (two positions on one mint intentionally reuse ONE account, see
    /// [`find_reusable_token_account`]). Gates the rent-reclaim `close_token_account`
    /// so one position's exit never closes the account out from under a sibling's bag
    /// (M1). Index-served by
    /// `idx_strategy_positions_mint_address_status`. Restart-safe (reads the DB SSOT,
    /// not an in-memory refcount).
    pub async fn has_other_open_position_on_mint(
        &self,
        wallet: &str,
        mint: &str,
        mode: &str,
        exclude_id: Uuid,
    ) -> anyhow::Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM strategy_positions \
                 WHERE wallet = $1 AND mint_address = $2 AND mode = $3 AND id <> $4 \
                   AND status IN ('BuySubmitted','Holding','ExitPending',\
                                  'ExitStuck','ExitUnconfirmed') \
             )",
        )
        .bind(wallet)
        .bind(mint)
        .bind(mode)
        .bind(exclude_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Flip **real** positions stuck in `ExitPending` past `stale_after` (orphaned
    /// mid-exit) to `ExitStuck` — but ONLY those NOT eligible for the bag-cleared
    /// heal (S4 fix: a landed-but-feed-lagged sell must be booked `End` by
    /// `find_exit_pending_cleared` + book, never stamped as a stuck loss). A row is
    /// heal-eligible when its wallet net is ≤ `threshold_raw` AND a sell is on
    /// record; everything else stale flips to `ExitStuck` (open, attention lane —
    /// the reaper's redrive/park machinery takes over). Returns rows affected.
    pub async fn mark_stale_exit_pending_stuck(
        &self,
        stale_after: std::time::Duration,
        threshold_raw: i64,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            r#"
            UPDATE strategy_positions p SET status = 'ExitStuck', updated_at = now()
            FROM wallet_dict w
            WHERE w.address = p.wallet
              AND p.status = 'ExitPending' AND p.mode = 'real' AND p.updated_at < $1
              AND NOT (
                    EXISTS (
                      SELECT 1 FROM trades s
                      WHERE s.wallet_id = w.id AND s.mint_address = p.mint_address
                        AND s.trade_type = 'sell'
                    )
                    AND COALESCE((
                      SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                      WHEN t.trade_type = 'sell' THEN -t.token_amount
                                      ELSE 0 END)
                      FROM trades t
                      WHERE t.wallet_id = w.id AND t.mint_address = p.mint_address
                    ), 0)::bigint <= $2
                  )
            "#,
        )
        .bind(cutoff)
        .bind(threshold_raw)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Stale **paper** `ExitPending` rows (crash mid-simulated-sell). Paper has no
    /// on-chain bag to check — the reaper books these `End` directly.
    pub async fn find_stale_paper_exit_pending(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status = 'ExitPending' AND mode = 'paper' AND updated_at < $1 \
             ORDER BY updated_at ASC",
        ))
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
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
            SELECT DISTINCT p.mint_address
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = 'Holding' AND p.entry_price IS NOT NULL
              -- Require a sell on record: else a position whose BUY merely aged out of
              -- the rolling buffer (net = 0, no sell) would be falsely "cleared".
              AND EXISTS (
                    SELECT 1 FROM trades s
                    WHERE s.wallet_id = w.id AND s.mint_address = p.mint_address
                      AND s.trade_type = 'sell'
                  )
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint_address
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
mod position_col_guard {
    //! No-DB guard over the three `strategy_positions` read column lists. SQL has
    //! no way to prefix a list, so the two aliased variants are hand-maintained
    //! copies of `POSITION_COLS` — exactly the "same fact defined twice" shape the
    //! super-root `CLAUDE.md` requires a guard for. sqlx maps
    //! `StrategyPositionDbRow` **positionally**, so a copy that drifts by one
    //! column decodes every later field into the wrong slot.
    use super::*;

    fn cols(list: &str) -> Vec<String> {
        list.split(',').map(|c| c.trim().to_string()).collect()
    }

    #[test]
    fn position_col_lists_stay_aliased_copies() {
        let base = cols(POSITION_COLS);
        assert!(base.len() > 30, "sanity: base list parsed as {} cols", base.len());
        for (alias, list) in [("sp", POSITION_COLS_SP), ("p", POSITION_COLS_P)] {
            let expected: Vec<String> = base.iter().map(|c| format!("{alias}.{c}")).collect();
            assert_eq!(
                cols(list),
                expected,
                "POSITION_COLS_{} drifted from POSITION_COLS (sqlx decodes positionally)",
                alias.to_uppercase()
            );
        }
    }

    /// Three columns are READ by the full row but written ONLY by their dedicated
    /// writer, because the engine sink's `update_position` lands *after* that
    /// writer and a shared write path would clobber it (mig 0012 / 0017). That
    /// invariant is what makes the reaper's redrive counter and the buy-failure
    /// cause survive at all, and nothing tested it until now.
    #[test]
    fn writer_owned_columns_never_enter_the_full_row_write() {
        const OWNED: [&str; 3] = ["exit_redrive_count", "exit_parked", "last_entry_error"];
        // Slice out `update_position`'s SQL literal so the scan cannot be fooled by
        // the dedicated writers' own `UPDATE`s elsewhere in this file.
        let src = include_str!("strategy_repo.rs");
        let from = src.find("pub async fn update_position").expect("update_position moved");
        let set_list = &src[from..];
        let set_list =
            &set_list[..set_list.find("WHERE id = $1").expect("update_position SQL shape changed")];
        for col in OWNED {
            assert!(
                !set_list.contains(col),
                "{col} must never appear in update_position's SET list — its dedicated writer \
                 would be clobbered (see migration 0012 / 0017)"
            );
            assert!(
                cols(POSITION_COLS).iter().any(|c| c == col),
                "{col} is written by a dedicated writer but must still be READ"
            );
        }
    }

    #[test]
    fn a_long_cause_is_truncated_on_a_char_boundary() {
        let long = "6002 ".repeat(200);
        let out = truncate_chars(&long, MAX_ENTRY_ERROR_LEN);
        assert_eq!(out.chars().count(), MAX_ENTRY_ERROR_LEN + 3);
        assert!(out.ends_with("..."));
        // Multi-byte input must not panic or split a sequence.
        let wide = "é".repeat(MAX_ENTRY_ERROR_LEN + 50);
        assert_eq!(
            truncate_chars(&wide, MAX_ENTRY_ERROR_LEN).chars().count(),
            MAX_ENTRY_ERROR_LEN + 3
        );
        // Short input is passed through untouched.
        assert_eq!(truncate_chars("reverted 6002", MAX_ENTRY_ERROR_LEN), "reverted 6002");
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
    fn neq_on_numeric_col_emits_not_equal() {
        // `!=333333` on a numeric column must compare with `!=`, not collapse to `=`.
        let sql = where_sql("cu_price", num(FilterOp::Neq, 333333.0));
        assert!(sql.contains(" != "), "expected `!=` compare, got: {sql}");
        assert!(!sql.contains(" = "), "neq must not emit an equality: {sql}");
    }

    #[test]
    fn pnl_pct_is_numeric_filterable_in_percent_units() {
        // `pnl_pct` is a computed column — `>0` must lower to a numeric compare over
        // the shared percentage expression (× 100), not be dropped.
        let sql = where_sql("pnl_pct", num(FilterOp::Gt, 0.0));
        assert!(sql.contains("* 100"), "pnl_pct filter must use the percent expr: {sql}");
        assert!(sql.contains(" > "), "pnl_pct `>0` must emit a numeric compare: {sql}");
    }

    #[test]
    fn pnl_sol_is_numeric_filterable() {
        // `pnl_sol` is computed from the fill prices × token counts; a `>0` filter must
        // lower to a numeric compare over that expression, not be dropped.
        let sql = where_sql("pnl_sol", num(FilterOp::Gt, 0.0));
        assert!(sql.contains("exit_token_amount"), "pnl_sol filter must use the proceeds expr: {sql}");
        assert!(sql.contains(" > "), "pnl_sol `>0` must emit a numeric compare: {sql}");
    }

    #[test]
    fn holding_is_numeric_filterable() {
        // The "Holding" column is the raw exit-token count — numeric-filterable.
        let sql = where_sql("holding", num(FilterOp::Gte, 1000.0));
        assert!(sql.contains("sp.exit_token_amount >="), "holding must compare on the token count: {sql}");
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
