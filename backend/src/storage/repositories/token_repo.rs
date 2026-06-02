use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use tracing::info;
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
    token_program_id: Option<String>,
    bonding_curve_address: Option<String>,
    initial_supply_token: Option<i64>,
    initial_buy_sol: Option<f64>,
    initial_buy_instruction: Option<Json<Value>>,
    cu_limit: Option<i64>,
    cu_price: Option<i64>,
    is_mayhem_mode: bool,
    is_cashback_enabled: bool,
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
            token_program_id: r.token_program_id,
            bonding_curve_address: r.bonding_curve_address,
            initial_supply_token: r.initial_supply_token.map(|v| v as u64),
            initial_buy_sol: r.initial_buy_sol,
            initial_buy_instruction: r.initial_buy_instruction.map(|v| v.0),
            cu_limit: r.cu_limit.map(|v| v as u64),
            cu_price: r.cu_price.map(|v| v as u64),
            is_mayhem_mode: r.is_mayhem_mode,
            is_cashback_enabled: r.is_cashback_enabled,
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
                (id, mint_address, creator_wallet, name, symbol, token_program_id,
                    bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, ix_labels,
                    creation_tx_signature, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (mint_address) DO NOTHING
            "#,
        )
        .bind(token.id)
        .bind(&token.mint_address)
        .bind(&token.creator_wallet)
        .bind(&token.name)
        .bind(&token.symbol)
        .bind(token.token_program_id.as_ref())
        .bind(&token.bonding_curve_address)
        .bind(token.initial_supply_token.map(|v| v as i64))
        .bind(token.initial_buy_sol)
        .bind(token.initial_buy_instruction.as_ref().map(|v| Json(v)))
        .bind(token.cu_limit.map(|v| v as i64))
        .bind(token.cu_price.map(|v| v as i64))
        .bind(token.is_mayhem_mode)
        .bind(token.is_cashback_enabled)
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

}
