use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::token::Token;

pub struct TokenRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row — keeps sqlx derives out of domain models
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TokenDbRow {
    id: Uuid,
    mint_address: String,
    creator_wallet: String,
    name: String,
    symbol: String,
    bonding_curve_address: Option<String>,
    initial_supply_token: Option<i64>,
    initial_buy_sol: Option<f64>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    ix_labels: Json<Value>,
    creation_tx_signature: String,
    created_at: DateTime<Utc>,
}

impl From<TokenDbRow> for Token {
    fn from(r: TokenDbRow) -> Self {
        Self {
            id: r.id,
            mint_address: r.mint_address,
            creator_wallet: r.creator_wallet,
            name: r.name,
            symbol: r.symbol,
            bonding_curve_address: r.bonding_curve_address,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            initial_buy_sol: r.initial_buy_sol,
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            instruction_labels: r.ix_labels.0,
            creation_tx_signature: r.creation_tx_signature,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl TokenRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a new token. Silently ignores duplicates (idempotent on replay).
    pub async fn insert(&self, token: &Token) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tokens
                (id, mint_address, creator_wallet, name, symbol,
                  bonding_curve_address, initial_supply_token, initial_buy_sol, cu_limit, cu_price, ix_labels,
                  creation_tx_signature, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (mint_address) DO NOTHING
            "#,
        )
        .bind(token.id)
        .bind(&token.mint_address)
        .bind(&token.creator_wallet)
        .bind(&token.name)
        .bind(&token.symbol)
        .bind(&token.bonding_curve_address)
        .bind(token.initial_supply_token.map(|v| v as i64))
        .bind(token.initial_buy_sol)
        .bind(token.cu_limit.map(|v| v as i64))
        .bind(token.cu_price.map(|v| v as i64))
        .bind(Json(&token.instruction_labels))
        .bind(&token.creation_tx_signature)
        .bind(token.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn find_by_mint(&self, mint: &str) -> anyhow::Result<Option<Token>> {
        let row = sqlx::query_as::<_, TokenDbRow>("SELECT * FROM tokens WHERE mint_address = $1")
            .bind(mint)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(Token::from))
    }

    #[allow(dead_code)]
    pub async fn exists(&self, mint: &str) -> anyhow::Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(1) FROM tokens WHERE mint_address = $1")
            .bind(mint)
            .fetch_one(&self.pool)
            .await?;

        Ok(count > 0)
    }

    /// Load every token from the database (used for cache seeding on startup).
    pub async fn find_all(&self) -> anyhow::Result<Vec<Token>> {
        let rows = sqlx::query_as::<_, TokenDbRow>("SELECT * FROM tokens ORDER BY created_at ASC")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(Token::from).collect())
    }

    /// Find all tokens whose creation parameters match a TPSL rule's entry criteria.
    ///
    /// Each parameter is optional — `None` skips that filter entirely so partial rules
    /// still match. An empty-array `p_ix_labels` is also treated as skipped.
    ///
    /// - `p_initial_buy_sol` — within ±1% tolerance; skipped if `None`
    /// - `p_cu_limit`        — exact match; skipped if `None`
    /// - `p_cu_price`        — exact match; skipped if `None`
    /// - `p_ix_labels`       — jsonb containment (`@>`); skipped if `None` or empty array
    /// - `limit`             — max rows returned; `None` = no limit
    pub async fn find_by_rule_criteria(
        &self,
        p_initial_buy_sol: Option<f64>,
        p_cu_limit: Option<u64>,
        p_cu_price: Option<u64>,
        p_ix_labels: Option<&serde_json::Value>,
        limit: Option<i64>,
    ) -> anyhow::Result<Vec<Token>> {
        // Tolerance for initial_buy_sol comparison (1% + epsilon).
        let tol = p_initial_buy_sol.map(|v| v.abs() * 0.01 + 1e-9);

        // Build the label filter: only apply when Some and non-empty array.
        let labels_filter = match p_ix_labels.and_then(|v| v.as_array()) {
            Some(arr) if !arr.is_empty() => p_ix_labels.cloned(),
            _ => None,
        };
        let apply_labels = labels_filter.is_some();
        let labels_json = labels_filter.unwrap_or(serde_json::Value::Null);

        // Bind cu_limit / cu_price as Option<i64> for nullable SQL params.
        let cu_limit_i64 = p_cu_limit.map(|v| v as i64);
        let cu_price_i64 = p_cu_price.map(|v| v as i64);

        // $1 = p_initial_buy_sol (NULL → skip),  $2 = tolerance
        // $3 = cu_limit,  $4 = cu_price
        // $5 = apply_labels,  $6 = labels_filter
        // $7 = limit (NULL → no limit)
        let rows = sqlx::query_as::<_, TokenDbRow>(
            r#"
            SELECT *
            FROM   tokens
            WHERE  ($1::float8 IS NULL OR (
                       initial_buy_sol IS NOT NULL
                   AND ABS(initial_buy_sol - $1) <= $2
                   ))
              AND  ($3::bigint IS NULL OR cu_limit  = $3)
              AND  ($4::bigint IS NULL OR cu_price  = $4)
              AND  ($5 = false   OR ix_labels @> $6::jsonb)
            ORDER BY created_at DESC
            LIMIT $7
            "#,
        )
        .bind(p_initial_buy_sol)
        .bind(tol.unwrap_or(0.0))
        .bind(cu_limit_i64)
        .bind(cu_price_i64)
        .bind(apply_labels)
        .bind(sqlx::types::Json(&labels_json))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Token::from).collect())
    }
}
