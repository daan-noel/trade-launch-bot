use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    /// The wallet's token account address for `mint` (base58). Persisted after the
    /// entry fill so subsequent buys reuse one account and the sell reads it from
    /// the row — no in-memory-cache dependency, survives restarts. `None` until the
    /// first fill, or on legacy rows predating this column (callers fall back to the
    /// cache-first resolver).
    pub token_account: Option<String>,
    // Target (arming) snapshot.
    pub target_price: Option<f64>,
    /// Raw token units (exact integer).
    pub target_token_amount: Option<u64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    // Entry fill.
    pub entry_price: Option<f64>,
    /// Raw token units (exact integer).
    pub entry_token_amount: Option<u64>,
    /// Human SOL (f64) in the model; stored as exact lamports (BIGINT) in the column.
    pub entry_sol: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub entry_tx_signatures: Value,
    // Exit fill.
    pub exit_price: Option<f64>,
    /// Raw token units (exact integer).
    pub exit_token_amount: Option<u64>,
    /// Human SOL (f64) in the model; stored as exact lamports (BIGINT) in the column.
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

    // ── Lifecycle ctor + mutators (the unified-schema twin of the old `Position`
    //    in-memory API; pure, no DB) ───────────────────────────────────────────

    /// A fresh `Arming` position within `run_id` (no fills yet). Mode/strategy are
    /// copied from the owning rule; `wallet` is the bot wallet (real) or a sentinel
    /// (paper).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: Uuid,
        strategy_id: String,
        rule_id: Uuid,
        mode: String,
        mint: String,
        wallet: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            run_id,
            strategy_id,
            rule_id: Some(rule_id),
            mode,
            mint,
            wallet,
            token_program_id: None,
            token_account: None,
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            entry_price: None,
            entry_token_amount: None,
            entry_sol: None,
            entry_time: None,
            entry_tx_signatures: json!([]),
            exit_price: None,
            exit_token_amount: None,
            exit_sol: None,
            exit_time: None,
            exit_tx_signatures: json!([]),
            submitted_buy_signatures: Vec::new(),
            status: "Arming".to_string(),
            exit_reason: None,
            extra: json!({}),
            created_at: now,
            updated_at: now,
        }
    }

    /// Record the target (trigger-trade) snapshot that armed this position.
    pub fn set_target(&mut self, price: f64, amount: u64, time: DateTime<Utc>, tx: String) {
        self.target_price = Some(price);
        self.target_token_amount = Some(amount);
        self.target_time = Some(time);
        self.target_tx = Some(tx);
        self.updated_at = Utc::now();
    }

    /// Append a submitted snipe-buy signature and flip to `BuySubmitted` (the
    /// durable "buy in flight" marker; every attempt is recoverable).
    pub fn mark_buy_submitted(&mut self, signature: String) {
        self.submitted_buy_signatures.push(signature);
        self.status = "BuySubmitted".to_string();
        self.updated_at = Utc::now();
    }

    /// Record the entry fill + flip to `Holding`.
    pub fn set_entry(
        &mut self,
        price: f64,
        token_amount: u64,
        sol: f64,
        time: DateTime<Utc>,
        tx_signatures: Vec<String>,
    ) {
        self.entry_price = Some(price);
        self.entry_token_amount = Some(token_amount);
        self.entry_sol = Some(sol);
        self.entry_time = Some(time);
        self.entry_tx_signatures = json!(tx_signatures);
        self.status = "Holding".to_string();
        self.updated_at = Utc::now();
    }

    /// Flip to `Holding` (fill adopted/stamped elsewhere).
    pub fn mark_entry_filled(&mut self) {
        self.status = "Holding".to_string();
        self.updated_at = Utc::now();
    }

    /// Flip to `ExitPending` while the sell is in flight.
    pub fn mark_exit_pending(&mut self) {
        self.status = "ExitPending".to_string();
        self.updated_at = Utc::now();
    }

    /// Terminally mark the exit failed, recording the hypothetical exit price/time
    /// (so the row still carries a PnL for analysis).
    pub fn mark_exit_failed(&mut self, exit_price: f64, exit_time: DateTime<Utc>) {
        self.exit_price = Some(exit_price);
        self.exit_time = Some(exit_time);
        self.status = "ExitFailed".to_string();
        self.updated_at = Utc::now();
    }

    /// Close with a confirmed exit fill (`End`).
    #[allow(clippy::too_many_arguments)]
    pub fn close(
        &mut self,
        exit_price: f64,
        exit_sol: f64,
        exit_token_amount: u64,
        exit_tx_signatures: Vec<String>,
        exit_time: DateTime<Utc>,
        reason: &str,
    ) {
        self.exit_price = Some(exit_price);
        self.exit_sol = Some(exit_sol);
        self.exit_token_amount = Some(exit_token_amount);
        self.exit_tx_signatures = json!(exit_tx_signatures);
        self.exit_time = Some(exit_time);
        self.exit_reason = Some(reason.to_string());
        self.status = "End".to_string();
        self.updated_at = Utc::now();
    }

    /// Entry fill signatures (the JSONB array decoded to a `Vec`).
    pub fn entry_tx_sigs(&self) -> Vec<String> {
        json_str_array(&self.entry_tx_signatures)
    }

    /// Exit fill signatures (the JSONB array decoded to a `Vec`).
    pub fn exit_tx_sigs(&self) -> Vec<String> {
        json_str_array(&self.exit_tx_signatures)
    }
}

/// Decode a JSON string-array `Value` into a `Vec<String>` (non-strings skipped).
fn json_str_array(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
