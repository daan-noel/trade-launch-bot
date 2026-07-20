//! `RuleRepo` — CRUD over the `strategy_rules` table: generic fingerprint +
//! metrics rules ([`crate::models::StrategyRule`]). Runs, run metrics, and
//! positions stay on `StrategyRepo`. (The pre-0004 tpsl/swing rule table was
//! retired in Phase 7.)

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::StrategyRule;

#[derive(Clone)]
pub struct RuleRepo {
    pool: PgPool,
}

// DB row — keeps sqlx derives out of the domain model.
#[derive(sqlx::FromRow)]
struct StrategyRuleDbRow {
    id: Uuid,
    rule_name: String,
    fingerprint_id: Uuid,
    trade_mode: String,
    is_active: bool,
    is_enabled: bool,
    buy_amount_lamports: i64,
    max_concurrent_tokens: i64,
    max_total_tokens: i64,
    params: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyRuleDbRow> for StrategyRule {
    fn from(r: StrategyRuleDbRow) -> Self {
        Self {
            id: r.id,
            rule_name: r.rule_name,
            fingerprint_id: r.fingerprint_id,
            trade_mode: r.trade_mode,
            is_active: r.is_active,
            is_enabled: r.is_enabled,
            buy_amount_lamports: r.buy_amount_lamports,
            max_concurrent_tokens: r.max_concurrent_tokens,
            max_total_tokens: r.max_total_tokens,
            params: r.params.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// Explicit column list (struct order) — not `SELECT *`.
const RULE_COLS: &str = "id, rule_name, fingerprint_id, trade_mode, is_active, is_enabled, \
    buy_amount_lamports, max_concurrent_tokens, max_total_tokens, params, \
    created_at, updated_at";

impl RuleRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_rules
                (id, rule_name, fingerprint_id, trade_mode, is_active, is_enabled, buy_amount_lamports,
                 max_concurrent_tokens, max_total_tokens, params, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(rule.id)
        .bind(&rule.rule_name)
        .bind(rule.fingerprint_id)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.is_enabled)
        .bind(rule.buy_amount_lamports)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_rules SET
                rule_name = $2,
                fingerprint_id = $3,
                trade_mode = $4,
                is_active = $5,
                is_enabled = $6,
                buy_amount_lamports = $7,
                max_concurrent_tokens = $8,
                max_total_tokens = $9,
                params = $10,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(rule.id)
        .bind(&rule.rule_name)
        .bind(rule.fingerprint_id)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.is_enabled)
        .bind(rule.buy_amount_lamports)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find(&self, id: Uuid) -> anyhow::Result<Option<StrategyRule>> {
        let row = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRule::from))
    }

    pub async fn list(&self) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    /// Every rule armed on a fingerprint (any mode/active/enabled state) — the
    /// fingerprint editor's "used by" view and the delete guard.
    pub async fn list_by_fingerprint(
        &self,
        fingerprint_id: Uuid,
    ) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE fingerprint_id = $1 \
             ORDER BY created_at DESC"
        ))
        .bind(fingerprint_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    /// The active rule set — what the live runtime cache loads (both modes;
    /// the caller partitions paper/real). Disabled rules never arm.
    pub async fn list_active(&self) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules \
             WHERE is_active AND is_enabled ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    /// Identity-identical rule (same fingerprint + trade knobs + canonical
    /// `params`). `rule_name` / `is_active` / `is_enabled` are labels/lifecycle,
    /// not identity — mirrors fingerprint `find_or_create` (name excluded).
    /// Optional `exclude_id` skips that row (edit-self).
    pub async fn find_identical(
        &self,
        fingerprint_id: Uuid,
        trade_mode: &str,
        buy_amount_lamports: i64,
        max_concurrent_tokens: i64,
        max_total_tokens: i64,
        params: &Value,
        exclude_id: Option<Uuid>,
    ) -> anyhow::Result<Option<StrategyRule>> {
        let row = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules \
             WHERE fingerprint_id = $1 \
               AND trade_mode = $2 \
               AND buy_amount_lamports = $3 \
               AND max_concurrent_tokens = $4 \
               AND max_total_tokens = $5 \
               AND params = $6::jsonb \
               AND ($7::uuid IS NULL OR id <> $7) \
             LIMIT 1"
        ))
        .bind(fingerprint_id)
        .bind(trade_mode)
        .bind(buy_amount_lamports)
        .bind(max_concurrent_tokens)
        .bind(max_total_tokens)
        .bind(Json(params))
        .bind(exclude_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRule::from))
    }

    pub async fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
