use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Strategy rule for **tpsl_sniper_2** — the scalp-continuation strategy.
/// Fully independent of tpsl1: it carries the common token-filter / exit-ladder
/// fields **plus** the scalp-continuation entry gates that only tpsl2
/// evaluates. Persisted in the tpsl2-only `tpsl2_strategy_rules` table via its
/// own repo; `new()` leaves the scalp gates `None`, with the tpsl2 API setting
/// them post-construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tpsl2Rule {
    pub id: Uuid,
    /// Human-readable name for this rule.
    pub rule_name: String,
    /// Initial buy amount in SOL for filtering token creation events.
    pub p_token_initial_buy_sol: Option<f64>,
    /// Compute-unit limit constraint (optional).
    pub p_token_cu_limit: Option<u64>,
    /// Compute-unit price constraint (optional), in micro-lamports per CU.
    pub p_token_cu_price: Option<u64>,
    /// Filter: match the token's creation-instruction max_sol_cost (optional).
    pub p_token_max_sol_cost: Option<f64>,
    /// Filter: match the token's creation-instruction spendable_sol_in (optional).
    pub p_token_spendable_sol_in: Option<f64>,
    /// Concurrency cap: max tokens held open at the same time.
    pub p_max_concurrent_tokens: Option<u64>,
    /// Total cap: max tokens this rule may trade over the whole run.
    pub p_max_total_tokens: Option<u64>,
    /// Instruction labels filter (optional JSON array).
    pub p_token_ix_labels: Value,
    /// Trade mode: "paper" (paper test) or "real" (real trading)
    pub trade_mode: String,
    /// Amount of SOL to allocate per buy.
    pub buy_amount_sol: f64,
    /// Take profit percentage (e.g., 50 for 50% gain).
    pub p_exit_take_profit: f64,
    /// Stop loss percentage (e.g., 20 for 20% loss).
    pub p_exit_stop_loss: f64,
    /// E1 · Trailing stop percentage: exit when price falls this far below the
    /// peak-since-entry. `None`/`0` disables (per `ignore_zero_f64`).
    pub p_exit_trailing_stop_pct: Option<f64>,
    /// E2 · Time stop (seconds): exit at the first trade at least this many
    /// seconds after entry. `None`/`0` disables (per `ignore_zero_u64`).
    pub p_exit_time_stop_secs: Option<u64>,
    /// E3 · Stall stop (seconds): exit once no new higher-high has printed for at
    /// least this many seconds, selling into the flatline. `None`/`0` disables
    /// (per `ignore_zero_u64`).
    pub p_exit_stall_secs: Option<u64>,
    /// E4 · Liquidity-death exit (percent): exit when **real** SOL reserves fall
    /// this far below the peak-since-entry. `None`/`0` disables (per
    /// `ignore_zero_f64`).
    pub p_exit_liquidity_drop_pct: Option<f64>,

    // ── Scalp-continuation entry gates ────────────────────────────────────────
    // Evaluated over a token's trade window — "buy on the first trade where ALL
    // configured gates hold". All inert at `None`/`0`.
    /// Skip the launch spike: the entry trade must be at least this many seconds
    /// after the token's first trade. `None`/`0` disables.
    pub p_entry_min_age_secs: Option<u64>,
    /// Entry-window ceiling: the entry trade must be at most this many seconds
    /// after the token's first trade — the symmetric upper bound to
    /// `p_entry_min_age_secs` (window = `[min_age, max_age]`). A **ceiling, not a
    /// positive gate**: it never makes a rule enter on its own (excluded from
    /// `rule_configures_any_scalp_gate`). `None`/`0` disables ⇒ no ceiling ⇒ the
    /// live watcher arms until the token dies (capped by the concurrent-armer
    /// limit), matching the backtest's full-history walk.
    pub p_entry_max_age_secs: Option<u64>,
    /// Liveness: total SOL traded (buys + sells, any wallet) in the trailing
    /// window ending at the candidate trade must be ≥ this. `None`/`0` disables.
    pub p_entry_min_alive_sol: Option<f64>,
    /// Real demand: net SOL bought so far (buys − sells, any wallet) must be ≥
    /// this. `None`/`0` disables.
    pub p_entry_min_organic_sol: Option<f64>,
    /// Higher-low shape gate — minimum dip off the local high (percent) for a dip
    /// to count. Paired with `p_entry_higher_low_secs`. `None`/`0` disables the gate.
    pub p_entry_pullback_pct: Option<f64>,
    /// Higher-low shape gate — minimum seconds the higher-low pattern must form
    /// over (filters sub-second fakes). Only active alongside `p_entry_pullback_pct`.
    pub p_entry_higher_low_secs: Option<u64>,
    /// Minimum **real** SOL reserves at entry. `None`/`0` disables.
    pub p_entry_min_liquidity_sol: Option<f64>,
    /// A second, independently tunable real-reserves floor — reads the same
    /// `real_sol_reserves` snapshot as `p_entry_min_liquidity_sol`. `None`/`0`
    /// disables.
    pub p_entry_min_organic_liq: Option<f64>,

    /// Price tolerance percent when matching p_token_initial_buy_sol.
    pub tolerance_pct: f64,
    /// Whether this rule is currently active.
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Tpsl2Rule {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rule_name: String,
        p_token_initial_buy_sol: Option<f64>,
        p_token_cu_limit: Option<u64>,
        p_token_cu_price: Option<u64>,
        p_token_ix_labels: Value,
        trade_mode: String,
        buy_amount_sol: f64,
        p_exit_take_profit: f64,
        p_exit_stop_loss: f64,
        p_token_max_sol_cost: Option<f64>,
        p_token_spendable_sol_in: Option<f64>,
        p_max_concurrent_tokens: Option<u64>,
        p_max_total_tokens: Option<u64>,
        tolerance_pct: Option<f64>,
        p_exit_trailing_stop_pct: Option<f64>,
        p_exit_time_stop_secs: Option<u64>,
        p_exit_stall_secs: Option<u64>,
        p_exit_liquidity_drop_pct: Option<f64>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            rule_name,
            p_token_initial_buy_sol,
            p_token_cu_limit,
            p_token_cu_price,
            p_token_ix_labels,
            trade_mode,
            buy_amount_sol,
            p_exit_take_profit,
            p_exit_stop_loss,
            p_exit_trailing_stop_pct,
            p_exit_time_stop_secs,
            p_exit_stall_secs,
            p_exit_liquidity_drop_pct,
            // Scalp-continuation gates default to disabled; the tpsl2 API sets
            // them post-construction.
            p_entry_min_age_secs: None,
            p_entry_max_age_secs: None,
            p_entry_min_alive_sol: None,
            p_entry_min_organic_sol: None,
            p_entry_pullback_pct: None,
            p_entry_higher_low_secs: None,
            p_entry_min_liquidity_sol: None,
            p_entry_min_organic_liq: None,
            p_token_max_sol_cost,
            p_token_spendable_sol_in,
            p_max_concurrent_tokens,
            p_max_total_tokens,
            tolerance_pct: tolerance_pct.unwrap_or(0.0),
            is_active: false,
            created_at: now,
            updated_at: now,
        }
    }
}
