use chrono::{DateTime, Utc};
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::Tpsl2StrategyRule;

pub struct Tpsl2StrategyRuleRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct Tpsl2StrategyRuleDbRow {
    id: Uuid,
    rule_name: String,
    p_initial_buy_sol: Option<f64>,
    p_cu_limit: Option<i64>,
    p_cu_price: Option<i64>,
    p_max_sol_cost: Option<f64>,
    p_spendable_sol_in: Option<f64>,
    p_max_concurrent_tokens: Option<i64>,
    p_max_total_tokens: Option<i64>,
    p_ix_labels: Json<serde_json::Value>,
    trade_mode: String,
    buy_amount: f64,
    take_profit: f64,
    stop_loss: f64,
    p_trailing_stop_pct: Option<f64>,
    p_time_stop_secs: Option<i64>,
    p_stall_secs: Option<i64>,
    p_liquidity_drop_pct: Option<f64>,
    // Scalp-continuation gates (migration 0008).
    p_min_age_secs: Option<i64>,
    p_min_alive_sol: Option<f64>,
    p_min_organic_sol: Option<f64>,
    p_pullback_pct: Option<f64>,
    p_higher_low_secs: Option<i64>,
    p_max_cohort_held: Option<f64>,
    p_min_liquidity_sol: Option<f64>,
    p_min_organic_liq: Option<f64>,
    p_cohort_exit_ratio: Option<f64>,
    tolerance_pct: f64,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Tpsl2StrategyRuleDbRow> for Tpsl2StrategyRule {
    fn from(r: Tpsl2StrategyRuleDbRow) -> Self {
        Self {
            id: r.id,
            rule_name: r.rule_name,
            p_initial_buy_sol: r.p_initial_buy_sol,
            p_cu_limit: r.p_cu_limit.map(|v| v as u64),
            p_cu_price: r.p_cu_price.map(|v| v as u64),
            p_max_sol_cost: r.p_max_sol_cost,
            p_spendable_sol_in: r.p_spendable_sol_in,
            p_max_concurrent_tokens: r.p_max_concurrent_tokens.map(|v| v as u64),
            p_max_total_tokens: r.p_max_total_tokens.map(|v| v as u64),
            p_ix_labels: r.p_ix_labels.0,
            trade_mode: r.trade_mode,
            buy_amount: r.buy_amount,
            take_profit: r.take_profit,
            stop_loss: r.stop_loss,
            p_trailing_stop_pct: r.p_trailing_stop_pct,
            p_time_stop_secs: r.p_time_stop_secs.map(|v| v as u64),
            p_stall_secs: r.p_stall_secs.map(|v| v as u64),
            p_liquidity_drop_pct: r.p_liquidity_drop_pct,
            p_min_age_secs: r.p_min_age_secs.map(|v| v as u64),
            p_min_alive_sol: r.p_min_alive_sol,
            p_min_organic_sol: r.p_min_organic_sol,
            p_pullback_pct: r.p_pullback_pct,
            p_higher_low_secs: r.p_higher_low_secs.map(|v| v as u64),
            p_max_cohort_held: r.p_max_cohort_held,
            p_min_liquidity_sol: r.p_min_liquidity_sol,
            p_min_organic_liq: r.p_min_organic_liq,
            p_cohort_exit_ratio: r.p_cohort_exit_ratio,
            tolerance_pct: r.tolerance_pct,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl Tpsl2StrategyRuleRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new TPSL rule.
    pub async fn insert(&self, rule: &Tpsl2StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tpsl2_strategy_rules
                (id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                 trade_mode, buy_amount, take_profit, stop_loss, tolerance_pct, is_active, created_at, updated_at, p_trailing_stop_pct, p_time_stop_secs, p_stall_secs, p_liquidity_drop_pct,
                 p_min_age_secs, p_min_alive_sol, p_min_organic_sol, p_pullback_pct, p_higher_low_secs, p_max_cohort_held, p_min_liquidity_sol, p_min_organic_liq, p_cohort_exit_ratio)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22,
                    $23, $24, $25, $26, $27, $28, $29, $30, $31)
            "#,
        )
        .bind(rule.id)
        .bind(&rule.rule_name)
        .bind(rule.p_initial_buy_sol)
        .bind(rule.p_cu_limit.map(|v| v as i64))
        .bind(rule.p_cu_price.map(|v| v as i64))
        .bind(rule.p_max_sol_cost)
        .bind(rule.p_spendable_sol_in)
        .bind(rule.p_max_concurrent_tokens.map(|v| v as i64))
        .bind(rule.p_max_total_tokens.map(|v| v as i64))
        .bind(Json(&rule.p_ix_labels))
        .bind(rule.trade_mode.clone())
        .bind(rule.buy_amount)
        .bind(rule.take_profit)
        .bind(rule.stop_loss)
        .bind(rule.tolerance_pct)
        .bind(rule.is_active)
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .bind(rule.p_trailing_stop_pct)
        .bind(rule.p_time_stop_secs.map(|v| v as i64))
        .bind(rule.p_stall_secs.map(|v| v as i64))
        .bind(rule.p_liquidity_drop_pct)
        .bind(rule.p_min_age_secs.map(|v| v as i64))
        .bind(rule.p_min_alive_sol)
        .bind(rule.p_min_organic_sol)
        .bind(rule.p_pullback_pct)
        .bind(rule.p_higher_low_secs.map(|v| v as i64))
        .bind(rule.p_max_cohort_held)
        .bind(rule.p_min_liquidity_sol)
        .bind(rule.p_min_organic_liq)
        .bind(rule.p_cohort_exit_ratio)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all TPSL rules (active and inactive).
    pub async fn find_all(&self) -> anyhow::Result<Vec<Tpsl2StrategyRule>> {
        let rows = sqlx::query_as::<_, Tpsl2StrategyRuleDbRow>(
            r#"
                 SELECT id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                     trade_mode, buy_amount, take_profit, stop_loss, p_trailing_stop_pct, p_time_stop_secs, p_stall_secs, p_liquidity_drop_pct,
                     p_min_age_secs, p_min_alive_sol, p_min_organic_sol, p_pullback_pct, p_higher_low_secs, p_max_cohort_held, p_min_liquidity_sol, p_min_organic_liq, p_cohort_exit_ratio,
                     tolerance_pct, is_active, created_at, updated_at
            FROM tpsl2_strategy_rules
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Tpsl2StrategyRule::from).collect())
    }

    /// Get a specific rule by ID.
    pub async fn find_by_id(&self, rule_id: Uuid) -> anyhow::Result<Option<Tpsl2StrategyRule>> {
        let row = sqlx::query_as::<_, Tpsl2StrategyRuleDbRow>(
            r#"
                 SELECT id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                     trade_mode, buy_amount, take_profit, stop_loss, p_trailing_stop_pct, p_time_stop_secs, p_stall_secs, p_liquidity_drop_pct,
                     p_min_age_secs, p_min_alive_sol, p_min_organic_sol, p_pullback_pct, p_higher_low_secs, p_max_cohort_held, p_min_liquidity_sol, p_min_organic_liq, p_cohort_exit_ratio,
                     tolerance_pct, is_active, created_at, updated_at
            FROM tpsl2_strategy_rules
            WHERE id = $1
            "#,
        )
        .bind(rule_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Tpsl2StrategyRule::from))
    }

    /// Update an existing TPSL rule.
    pub async fn update(&self, rule: &Tpsl2StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE tpsl2_strategy_rules
            SET rule_name = $1, p_initial_buy_sol = $2, p_cu_limit = $3, p_cu_price = $4,
                p_ix_labels = $5, trade_mode = $6, buy_amount = $7, take_profit = $8, stop_loss = $9,
                p_max_sol_cost = $10, p_spendable_sol_in = $11, p_max_concurrent_tokens = $12, p_max_total_tokens = $13, tolerance_pct = $14, is_active = $15, updated_at = $16, p_trailing_stop_pct = $17, p_time_stop_secs = $18, p_stall_secs = $19, p_liquidity_drop_pct = $20,
                p_min_age_secs = $21, p_min_alive_sol = $22, p_min_organic_sol = $23, p_pullback_pct = $24, p_higher_low_secs = $25, p_max_cohort_held = $26, p_min_liquidity_sol = $27, p_min_organic_liq = $28, p_cohort_exit_ratio = $29
            WHERE id = $30
            "#,
        )
        .bind(&rule.rule_name)
        .bind(rule.p_initial_buy_sol)
        .bind(rule.p_cu_limit.map(|v| v as i64))
        .bind(rule.p_cu_price.map(|v| v as i64))
        .bind(Json(&rule.p_ix_labels))
        .bind(rule.trade_mode.clone())
        .bind(rule.buy_amount)
        .bind(rule.take_profit)
        .bind(rule.stop_loss)
        .bind(rule.p_max_sol_cost)
        .bind(rule.p_spendable_sol_in)
        .bind(rule.p_max_concurrent_tokens.map(|v| v as i64))
        .bind(rule.p_max_total_tokens.map(|v| v as i64))
        .bind(rule.tolerance_pct)
        .bind(rule.is_active)
        .bind(Utc::now())
        .bind(rule.p_trailing_stop_pct)
        .bind(rule.p_time_stop_secs.map(|v| v as i64))
        .bind(rule.p_stall_secs.map(|v| v as i64))
        .bind(rule.p_liquidity_drop_pct)
        .bind(rule.p_min_age_secs.map(|v| v as i64))
        .bind(rule.p_min_alive_sol)
        .bind(rule.p_min_organic_sol)
        .bind(rule.p_pullback_pct)
        .bind(rule.p_higher_low_secs.map(|v| v as i64))
        .bind(rule.p_max_cohort_held)
        .bind(rule.p_min_liquidity_sol)
        .bind(rule.p_min_organic_liq)
        .bind(rule.p_cohort_exit_ratio)
        .bind(rule.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete a rule by ID.
    pub async fn delete(&self, rule_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tpsl2_strategy_rules WHERE id = $1")
            .bind(rule_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
