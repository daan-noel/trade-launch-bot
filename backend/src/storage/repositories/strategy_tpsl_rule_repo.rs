use chrono::{DateTime, Utc};
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::StrategyTPSLRule;

pub struct StrategyTPSLRuleRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct StrategyTPSLRuleDbRow {
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
    tolerance_pct: f64,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyTPSLRuleDbRow> for StrategyTPSLRule {
    fn from(r: StrategyTPSLRuleDbRow) -> Self {
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

impl StrategyTPSLRuleRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new TPSL rule.
    pub async fn insert(&self, rule: &StrategyTPSLRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_TPSL_rules
                (id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                 trade_mode, buy_amount, take_profit, stop_loss, tolerance_pct, is_active, created_at, updated_at, p_trailing_stop_pct)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
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
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all TPSL rules (active and inactive).
    pub async fn find_all(&self) -> anyhow::Result<Vec<StrategyTPSLRule>> {
        let rows = sqlx::query_as::<_, StrategyTPSLRuleDbRow>(
            r#"
                 SELECT id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                     trade_mode, buy_amount, take_profit, stop_loss, p_trailing_stop_pct, tolerance_pct, is_active, created_at, updated_at
            FROM strategy_TPSL_rules
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(StrategyTPSLRule::from).collect())
    }

    /// Get a specific rule by ID.
    pub async fn find_by_id(&self, rule_id: Uuid) -> anyhow::Result<Option<StrategyTPSLRule>> {
        let row = sqlx::query_as::<_, StrategyTPSLRuleDbRow>(
            r#"
                 SELECT id, rule_name, p_initial_buy_sol, p_cu_limit, p_cu_price, p_max_sol_cost, p_spendable_sol_in, p_max_concurrent_tokens, p_max_total_tokens, p_ix_labels,
                     trade_mode, buy_amount, take_profit, stop_loss, p_trailing_stop_pct, tolerance_pct, is_active, created_at, updated_at
            FROM strategy_TPSL_rules
            WHERE id = $1
            "#,
        )
        .bind(rule_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(StrategyTPSLRule::from))
    }

    /// Update an existing TPSL rule.
    pub async fn update(&self, rule: &StrategyTPSLRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_TPSL_rules
            SET rule_name = $1, p_initial_buy_sol = $2, p_cu_limit = $3, p_cu_price = $4,
                p_ix_labels = $5, trade_mode = $6, buy_amount = $7, take_profit = $8, stop_loss = $9,
                p_max_sol_cost = $10, p_spendable_sol_in = $11, p_max_concurrent_tokens = $12, p_max_total_tokens = $13, tolerance_pct = $14, is_active = $15, updated_at = $16, p_trailing_stop_pct = $17
            WHERE id = $18
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
        .bind(rule.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete a rule by ID.
    pub async fn delete(&self, rule_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_TPSL_rules WHERE id = $1")
            .bind(rule_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
