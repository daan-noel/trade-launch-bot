use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A configured strategy rule — the authored knobs that spawn runs.
/// Backs the `strategy_rules` table (unified schema, replacing per-strategy
/// tpsl1/tpsl2 tables). `params` holds the strategy-specific tuning as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRule {
    pub id: Uuid,
    /// Strategy family identifier (e.g. `tpsl_sniper_1`).
    pub strategy_id: String,
    /// Human-facing rule label.
    pub rule_name: String,
    /// Buy size in SOL per fired token.
    pub buy_amount: f64,
    /// Execution mode: `paper` or `real`.
    pub trade_mode: String,
    /// Whether the rule is eligible to fire.
    pub is_active: bool,
    /// Cap on concurrently-open tokens (None = unbounded).
    pub max_concurrent_tokens: Option<i64>,
    /// Cap on total tokens across the run's lifetime (None = unbounded).
    pub max_total_tokens: Option<i64>,
    /// Strategy-specific parameters as JSON.
    pub params: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One execution of a rule (real or paper). Backs the `strategy_runs` table.
/// `run_seq` is monotonic per `(rule_id, mode)`; `params_snapshot` freezes the
/// rule params at launch so later rule edits don't rewrite history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRun {
    pub id: Uuid,
    pub strategy_id: String,
    /// Owning rule (None if the rule was deleted — `ON DELETE SET NULL`).
    pub rule_id: Option<Uuid>,
    /// Execution mode: `real` or `paper`.
    pub mode: String,
    /// Monotonic sequence per `(rule_id, mode)`.
    pub run_seq: i64,
    /// `Running` | `Finished` | `Stopped` | `Cancelled`.
    pub status: String,
    /// Frozen copy of the rule params at launch.
    pub params_snapshot: Value,
    pub max_total_tokens: Option<i64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Rolled-up performance metrics for a single run. Backs the
/// `strategy_run_metrics` table (1:1 with `strategy_runs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRunMetrics {
    pub run_id: Uuid,
    pub rolled_up_at: DateTime<Utc>,
    pub n_fired: i32,
    pub n_open: i32,
    pub n_closed: i32,
    pub win_rate: f32,
    pub total_pnl_sol: f32,
    pub expectancy_sol: f32,
    pub mean_pnl_pct: f32,
    pub median_pnl_pct: f32,
    pub p90_pnl_pct: f32,
    pub best_pnl_pct: f32,
    pub worst_pnl_pct: f32,
    pub std_pnl_pct: f32,
    pub profit_factor: Option<f32>,
    pub avg_holding_secs: f32,
    pub median_holding_secs: f32,
    pub n_exit_take_profit: i32,
    pub n_exit_stop_loss: i32,
    pub n_exit_trailing: i32,
    pub n_exit_stall: i32,
    pub n_exit_time: i32,
    pub n_exit_liquidity: i32,
    pub n_exit_cohort: i32,
    pub n_exit_open: i32,
}

/// A single position lifecycle within a run. Backs the `strategy_positions`
/// table. JSONB signature lists are `serde_json::Value`; the Postgres `TEXT[]`
/// `submitted_buy_signatures` maps to `Vec<String>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyPosition {
    pub id: Uuid,
    pub run_id: Uuid,
    pub strategy_id: String,
    pub rule_id: Option<Uuid>,
    /// Execution mode: `real` or `paper`.
    pub mode: String,
    pub mint: String,
    pub wallet: String,
    pub token_program_id: Option<String>,
    // Target (arming) snapshot.
    pub target_price: Option<f64>,
    pub target_token_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    // Entry fill.
    pub entry_price: Option<f64>,
    pub entry_token_amount: Option<f64>,
    pub entry_sol: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub entry_tx_signatures: Value,
    // Exit fill.
    pub exit_price: Option<f64>,
    pub exit_token_amount: Option<f64>,
    pub exit_sol: Option<f64>,
    pub exit_time: Option<DateTime<Utc>>,
    pub exit_tx_signatures: Value,
    /// Raw submitted buy signatures (`TEXT[]`).
    pub submitted_buy_signatures: Vec<String>,
    /// `Arming` | `BuySubmitted` | `Holding` | `ExitPending` | `End` | `ExitFailed`.
    pub status: String,
    pub exit_reason: Option<String>,
    pub extra: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StrategyPosition {
    /// In the runtime holding index: still arming, buy in flight, or held. These
    /// are the states the exit gate and fill-adopt path scan by mint.
    pub fn is_in_holding_index(&self) -> bool {
        matches!(self.status.as_str(), "Arming" | "BuySubmitted" | "Holding")
    }

    /// Fully entered and currently held (SOL deployed, not yet exiting).
    pub fn is_holding(&self) -> bool {
        self.status == "Holding"
    }

    /// Terminally closed — either a clean exit or a failed one.
    pub fn is_closed(&self) -> bool {
        matches!(self.status.as_str(), "End" | "ExitFailed")
    }

    /// A real entry landed (SOL deployed) — the gate for the cap counters.
    pub fn is_entered(&self) -> bool {
        self.entry_price.is_some()
    }

    /// Realized SOL PnL (`exit_sol − entry_sol`), once both fills are recorded.
    /// Mirrors `strategy_position_pnl.realized_pnl_sol`.
    pub fn realized_pnl_sol(&self) -> Option<f64> {
        match (self.entry_sol, self.exit_sol) {
            (Some(entry), Some(exit)) => Some(exit - entry),
            _ => None,
        }
    }

    /// Realized PnL % off the entry price. Mirrors `strategy_position_pnl.pnl_pct`.
    pub fn pnl_pct(&self) -> Option<f64> {
        match (self.entry_price, self.exit_price) {
            (Some(entry), Some(exit)) if entry > 0.0 => Some((exit - entry) / entry * 100.0),
            _ => None,
        }
    }

    /// A clean `End` exit that realized positive SOL — the win/loss classifier the
    /// per-rule closed-stats counters use (everything else is a loss).
    pub fn is_win(&self) -> bool {
        self.status == "End" && self.realized_pnl_sol().map(|p| p > 0.0).unwrap_or(false)
    }
}
