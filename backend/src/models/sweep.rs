//! Param-sweep persistence models — `sweep_runs` / `sweep_results`. Produced by
//! the `sweep` CLI, read by the per-strategy sweep page. Serialize-only (the API
//! never deserializes them from the client); field names are the JSON the
//! frontend table binds to.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// One sweep invocation: which strategy/rule over what corpus.
#[derive(Debug, Clone, Serialize)]
pub struct SweepRun {
    pub id: Uuid,
    pub strategy: String,
    pub rule_id: Option<Uuid>,
    pub source: String,
    pub method: String,
    pub token_count: i32,
    pub combo_count: i32,
    pub corpus_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One ranked param-pair row within a run — the aggregated outcome across all
/// swept tokens. `params` carries the strategy's swept knob values as JSON so the
/// table can show/sort any knob without a per-strategy schema.
#[derive(Debug, Clone, Serialize)]
pub struct SweepResult {
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
    /// `None` = no losing trades (infinite profit factor); UI shows ∞.
    pub profit_factor: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    pub exit_take_profit: i32,
    pub exit_stop_loss: i32,
    pub exit_trailing: i32,
    pub exit_stall: i32,
    pub exit_time: i32,
    pub exit_liquidity: i32,
    pub exit_cohort: i32,
    pub exit_open: i32,
}
