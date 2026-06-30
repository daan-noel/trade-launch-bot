//! Grouped param-sweep persistence models — strategy-agnostic.
//!
//! One sweep run partitions its corpus into fingerprint groups and ranks param
//! combos within each group. These map the per-strategy tables
//! (`<strategy>_grouped_sweep_runs` / `_groups` / `_results`) the registry
//! resolves; the generic repo is table-name-driven, so a new strategy reuses
//! these models verbatim. Serialize-only — the API never deserializes them from
//! the client; field names are the JSON the frontend tables bind to.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// One grouped-sweep invocation header: which strategy over what selection, the
/// grouping fields, the resolved axes, and the realised population counts.
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepRun {
    pub id: Uuid,
    pub strategy_id: String,
    pub source: String,
    pub method: String,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub curve_only: bool,
    /// The grouping fields, e.g. `["creator_wallet","max_sol_cost"]`.
    pub grouping_spec: Value,
    /// The resolved param axes (post-defaults/dedup) for echo / re-run.
    pub axes_spec: Value,
    pub min_tokens: i32,
    pub token_count: i32,
    pub group_count: i32,
    pub combo_count: i32,
    pub corpus_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    /// The exact-set instruction-label corpus filter the run used, as the JSON
    /// array the request sent (`None` = no filter / grouped by `ix_labels`).
    /// Persisted so the history panel can show what the sweep was pinned to and
    /// the re-run can restore it.
    pub ix_labels_filter: Option<Value>,
    /// Per-field value filters the corpus was pinned to (`{"cu_price":[1000],…}`);
    /// `None` = no field filter. Stored verbatim for display + re-run.
    pub field_filters: Option<Value>,
    /// The per-run token cap the form submitted (`None` = legacy / backend
    /// default). Distinct from the realized `token_count`.
    pub token_cap: Option<i32>,
    /// The per-group combo-cap override the form submitted (`None` = backend
    /// default). Distinct from the realized `combo_count`.
    pub max_combos: Option<i32>,
    /// Optional user-given name for the run (`None` = unnamed → UI falls back to
    /// timestamp + grouping hint). Editable via the rename endpoint.
    pub label: Option<String>,
    /// Notional (SOL) every simulated round-trip in this run was priced at.
    /// `None` on legacy rows — callers fall back to the server default (1.0 SOL).
    pub buy_amount_sol: Option<f64>,
    /// Lifecycle: `running` (in flight), `completed` (full sweep), or
    /// `cancelled` (cancelled / crash-recovered → only `groups_done` groups
    /// present). Phase 4 partial persistence — a `cancelled` run is honest about
    /// being partial so the UI never shows it as a complete sweep.
    pub status: String,
    /// Groups persisted so far; equals `group_count` for a `completed` run, fewer
    /// for a `cancelled`/partial one. Drives the run picker's "37 / 200 groups".
    pub groups_done: i32,
}

/// One group's summary row (the group-list table): its fingerprint key, sample
/// size, and the winning combo. The winner is picked on the robust realized
/// `best_score` (the headline metric); `fired_count` is its `n_fired` — the
/// sample size behind the pick — and `best_expectancy_sol` its expectancy
/// (kept as a secondary readout, no longer the ranking metric).
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepGroupSummary {
    pub id: Uuid,
    pub group_index: i32,
    pub group_key: Value,
    pub token_count: i32,
    pub fired_count: i64,
    pub best_combo_id: i32,
    /// Robust realized `score` of the winning combo (`μ−Z·σ/√n` over closed
    /// trades); `None` when it has < 2 closed trades. The page's headline metric.
    pub best_score: Option<f64>,
    pub best_expectancy_sol: f64,
    pub best_params: Value,
}

/// One ranked param-combo row within a group (the drill-in table). Metric set
/// matches the flat per-combo `ComboMetrics` so the frontend reuses
/// `buildSweepColumns`.
#[derive(Debug, Clone, Serialize)]
pub struct GroupedSweepResult {
    pub combo_id: i32,
    pub params: Value,
    pub n_fired: i64,
    pub n_open: i64,
    pub n_closed: i64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    /// Stddev of realized per-trade pnl% — the dispersion term in `score`.
    pub std_pnl_pct: f64,
    /// `None` = no losing trades (infinite profit factor); UI shows ∞.
    pub profit_factor: Option<f64>,
    /// Robust rank `μ − z·σ/√n` over closed trades; `None` when n_closed < 2.
    pub score: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    /// Per-exit-reason trade counts — how many of this combo's closed trades
    /// terminated on each reason. Counts, **not** params: distinct from the
    /// `exit_take_profit`/`exit_stop_loss` *threshold* knobs inside `params`.
    pub n_exit_take_profit: i32,
    pub n_exit_stop_loss: i32,
    pub n_exit_trailing: i32,
    pub n_exit_stall: i32,
    pub n_exit_time: i32,
    pub n_exit_liquidity: i32,
    pub n_exit_cohort: i32,
    /// swing1's symmetric next-kill flee count; 0 for tpsl1/tpsl2.
    pub n_exit_next_kill: i32,
    pub n_exit_open: i32,
}

/// Per-token outcome when a single combo is re-simulated on a group's corpus
/// slice. Returned by the `GET …/token-results` drill-in endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ComboTokenResult {
    pub mint: String,
    pub symbol: String,
    pub fired: bool,
    pub pnl_sol: f32,
    pub pnl_pct: f32,
    pub holding_secs: i64,
    /// Exit reason string: `"TakeProfit"`, `"StopLoss"`, `"TrailingStop"`,
    /// `"Stall"`, `"TimeStop"`, `"LiquidityExit"`, `"CohortExit"`,
    /// `"Open"` (still open at end of history), or `"NoEntry"`.
    pub exit: String,
    // --- Simulation fill details (populated by single-combo re-sim) ---
    /// RFC3339 block time of the simulated entry fill; `None` when not fired.
    pub entry_time: Option<String>,
    /// Simulated entry fill price in SOL/token; `None` when not fired.
    pub entry_price: Option<f64>,
    /// RFC3339 block time of the simulated exit fill; `None` when open or not fired.
    pub exit_time: Option<String>,
    /// Simulated exit fill price in SOL/token; `None` when open or not fired.
    pub exit_price: Option<f64>,
    // --- Token metadata (batch-joined from tokens + tokens_info after sim) ---
    pub created_at: Option<String>,
    pub creator_wallet: Option<String>,
    pub ath_price: Option<f64>,
    pub ath_timestamp: Option<String>,
    pub current_price: Option<f64>,
    pub market_cap: Option<f64>,
    pub volume_sol: Option<f64>,
    pub trade_count: Option<i64>,
    pub is_migrated: Option<bool>,
    pub is_dead: Option<bool>,
}

/// A group plus its ranked combo rows, handed to the repo's `save_run` as the
/// write unit (the repo links them via a freshly-minted group id).
pub struct GroupedSweepGroupWrite {
    pub group_index: i32,
    pub group_key: Value,
    pub token_count: i32,
    pub fired_count: i64,
    pub best_combo_id: i32,
    pub best_score: Option<f64>,
    pub best_expectancy_sol: f64,
    pub best_params: Value,
    pub results: Vec<GroupedSweepResult>,
    /// Option C: mint addresses of every token that fell into this group. Stored
    /// in DB so `list_token_results` can load only these N tokens cold instead of
    /// re-loading the entire corpus.
    pub mints: Vec<String>,
}
