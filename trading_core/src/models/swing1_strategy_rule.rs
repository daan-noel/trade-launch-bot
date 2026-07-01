use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Strategy rule for **swing_1** — the kill→volume swing-phase strategy.
///
/// Unlike tpsl1/tpsl2 there is **no token-creation gate**: swing1 watches the
/// trade stream and decides per-token from the swing chain (the static dev
/// fingerprint already filters the candidate set upstream). The rule carries the
/// common exit-ladder fields **plus** four swing1-specific axis groups:
///   • swing detection (reversal thresholds + min leg trades),
///   • kill-low profile (deep + short),
///   • volume-low profile + count-free transition floor,
///   • entry confirmation (higher-low) and the symmetric next-kill exit.
///
/// `new()` fills only the common knobs; the swing1 axes default to inert
/// (`None`/`0`) and are set post-construction (mirrors the tpsl2 API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Swing1Rule {
    pub id: Uuid,
    pub rule_name: String,
    // Token-filter fingerprint fields are retained for symmetry with the unified
    // schema (and so the lab UI can author them), but swing1 does NOT gate token
    // creation on them — the fingerprint filter is applied upstream.
    pub p_token_initial_buy_sol: Option<f64>,
    pub p_token_cu_limit: Option<u64>,
    pub p_token_cu_price: Option<u64>,
    pub p_token_max_sol_cost: Option<f64>,
    pub p_token_spendable_sol_in: Option<f64>,
    pub p_max_concurrent_tokens: Option<u64>,
    pub p_max_total_tokens: Option<u64>,
    pub p_token_ix_labels: Value,
    pub trade_mode: String,
    pub buy_amount: f64,

    // ── Exit ladder (reused semantics) ────────────────────────────────────────
    pub p_exit_take_profit: f64,
    pub p_exit_stop_loss: f64,
    /// E1 · trailing stop %. `None`/`0` disables.
    pub p_exit_trailing_stop_pct: Option<f64>,
    /// E2 · time stop (secs) — backstop. `None`/`0` disables.
    pub p_exit_time_stop_secs: Option<u64>,
    /// E3 · stall stop (secs). `None`/`0` disables.
    pub p_exit_stall_secs: Option<u64>,
    /// E4 · liquidity-death exit % (real-reserves variant). `None`/`0` disables.
    pub p_exit_liquidity_drop_pct: Option<f64>,

    // ── Swing detection (reversal thresholds + leg quality floor) ──────────────
    pub p_swing_high_to_low_sol: Option<f64>,
    pub p_swing_high_to_low_pct: Option<f64>,
    pub p_swing_low_to_high_sol: Option<f64>,
    pub p_swing_low_to_high_pct: Option<f64>,
    pub p_swing_min_leg_trades: Option<u32>,
    /// Dynamic dust floor (fraction): a trade is dropped during leg detection if
    /// its SOL is `< p_dust_frac * active_leg_max_sol`, i.e. trivially small
    /// relative to that leg's own activity. Scale-free. `None`/`0` = off.
    pub p_dust_frac: Option<f64>,

    // ── Kill-low profile (deep + short) ───────────────────────────────────────
    pub p_kill_depth_min_pct: Option<f64>,
    pub p_kill_max_duration_ms: Option<i64>,
    pub p_kill_min_net_flow_per_sec: Option<f64>,

    // ── Volume-low profile + count-free transition floor ──────────────────────
    pub p_vol_depth_max_pct: Option<f64>,
    pub p_vol_min_duration_ms: Option<i64>,
    pub p_vol_min_up_duration_ms: Option<i64>,
    pub p_min_kills_before_volume: Option<u32>,

    // ── Entry confirmation (reuse higher_low_confirmed_index) ─────────────────
    pub p_entry_pullback_pct: Option<f64>,
    pub p_entry_higher_low_secs: Option<u64>,
    /// Armer/window ceiling — entry must confirm within this many secs of the
    /// volume-phase latch. `None`/`0` = no ceiling (arm until dead).
    pub p_entry_max_age_secs: Option<u64>,
    /// Optional entry guard: min real SOL liquidity at entry. `None`/`0` disables.
    pub p_entry_min_liquidity_sol: Option<f64>,
    /// Optional entry guard: max launch-cohort held ratio (%). `None`/`0` disables.
    pub p_entry_max_cohort_held: Option<f64>,

    // ── Symmetric next-kill exit (separate from entry kill_* thresholds) ──────
    pub p_exit_next_kill_depth_min_pct: Option<f64>,
    pub p_exit_next_kill_max_duration_ms: Option<i64>,

    pub tolerance_pct: f64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Swing1Rule {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rule_name: String,
        p_token_initial_buy_sol: Option<f64>,
        p_token_cu_limit: Option<u64>,
        p_token_cu_price: Option<u64>,
        p_token_ix_labels: Value,
        trade_mode: String,
        buy_amount: f64,
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
            buy_amount,
            p_exit_take_profit,
            p_exit_stop_loss,
            p_exit_trailing_stop_pct,
            p_exit_time_stop_secs,
            p_exit_stall_secs,
            p_exit_liquidity_drop_pct,
            // swing1 axes default to inert; the params API sets them post-construction.
            p_swing_high_to_low_sol: None,
            p_swing_high_to_low_pct: None,
            p_swing_low_to_high_sol: None,
            p_swing_low_to_high_pct: None,
            p_swing_min_leg_trades: None,
            p_dust_frac: None,
            p_kill_depth_min_pct: None,
            p_kill_max_duration_ms: None,
            p_kill_min_net_flow_per_sec: None,
            p_vol_depth_max_pct: None,
            p_vol_min_duration_ms: None,
            p_vol_min_up_duration_ms: None,
            p_min_kills_before_volume: None,
            p_entry_pullback_pct: None,
            p_entry_higher_low_secs: None,
            p_entry_max_age_secs: None,
            p_entry_min_liquidity_sol: None,
            p_entry_max_cohort_held: None,
            p_exit_next_kill_depth_min_pct: None,
            p_exit_next_kill_max_duration_ms: None,
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
