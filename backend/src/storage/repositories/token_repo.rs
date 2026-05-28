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
                    bonding_curve_address, initial_supply_token, initial_buy_sol, initial_buy_instruction, cu_limit, cu_price, is_mayhem_mode, ix_labels,
                    creation_tx_signature, created_at)
              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
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
    /// - `p_initial_buy_sol` — within ±`p_tolerance_pct` tolerance; skipped if `None`
    /// - `p_tolerance_pct`   — percent tolerance of `p_initial_buy_sol`; defaults to 1 if `None`
    /// - `p_cu_limit`        — exact match; skipped if `None`
    /// - `p_cu_price`        — exact match; skipped if `None`
    /// - `p_ix_labels`       — jsonb containment (`@>`); skipped if `None` or empty array
    /// - `limit`             — max rows returned; `None` = no limit
    pub async fn find_by_rule_criteria(
        &self,
        p_initial_buy_sol: Option<f64>,
        p_tolerance_pct: Option<f64>,
        p_cu_limit: Option<u64>,
        p_cu_price: Option<u64>,
        p_max_sol_cost: Option<f64>,
        p_spendable_sol_in: Option<f64>,
        p_ix_labels: Option<&serde_json::Value>,
        limit: Option<i64>,
    ) -> anyhow::Result<Vec<Token>> {


        // Reintroduce tolerance for `initial_buy_sol` only. Use provided
        // `p_tolerance_pct` when set; otherwise use a dynamic default:
        // - 1.0% for values >= 0.001
        // - 5.0% for very small values (< 0.001) so tiny tokens still match
        let tolerance_pct = match p_tolerance_pct {
            Some(v) if v > 0.0 => v,
            _ => match p_initial_buy_sol {
                Some(v) if v.abs() < 0.001 => 5.0,
                _ => 1.0,
            },
        };
        // Small epsilon to avoid binary rounding misses
        let eps = 1e-9_f64;
        let tol_buy = p_initial_buy_sol.map(|v| v.abs() * (tolerance_pct * 0.01) + eps);

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

        // The buy instruction fields are stored in lamports, while the rule
        // criteria are expressed in SOL. Convert rule values to lamports for
        // consistent database comparison.
        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
        // Accept either SOL or lamports: auto-detect values that look like lamports
        // (large integers, e.g. > 1e6) and treat them as lamports instead of SOL.
        let p_max_sol_cost_lamports = p_max_sol_cost.map(|v| {
            if v.abs() >= 1e6 {
                // already lamports
                v.round() as i64
            } else {
                (v * LAMPORTS_PER_SOL).round() as i64
            }
        });
        let p_spendable_sol_in_lamports = p_spendable_sol_in.map(|v| {
            if v.abs() >= 1e6 {
                v.round() as i64
            } else {
                (v * LAMPORTS_PER_SOL).round() as i64
            }
        });

        // $1 = p_initial_buy_sol (NULL → skip)
        // $2 = tolerance for initial_buy_sol
        // $3 = p_cu_limit
        // $4 = p_cu_price
        // $5 = p_max_sol_cost_lamports
        // $6 = p_spendable_sol_in_lamports
        // $7 = apply_labels,  $8 = labels_filter
        // $9 = limit (NULL → no limit)
        let rows = sqlx::query_as::<_, TokenDbRow>(
            r#"
            SELECT *
            FROM   tokens
            WHERE  ($1::float8 IS NULL OR (
                       initial_buy_sol IS NOT NULL
                  AND ABS(initial_buy_sol - $1::float8) <= $2::float8
                  ))
              AND  ($3::bigint IS NULL OR (
                       cu_limit IS NOT NULL
                   AND cu_limit = $3::bigint
                   ))
              AND  ($4::bigint IS NULL OR (
                       cu_price IS NOT NULL
                   AND cu_price = $4::bigint
                   ))
              AND  ($5::bigint IS NULL OR (
                       initial_buy_instruction IS NOT NULL
                   AND (initial_buy_instruction->>'max_sol_cost')::bigint = $5::bigint
                   ))
              AND  ($6::bigint IS NULL OR (
                       initial_buy_instruction IS NOT NULL
                   AND (initial_buy_instruction->>'spendable_sol_in')::bigint = $6::bigint
                   ))
              AND  ($7 = false   OR ix_labels @> $8::jsonb)
            ORDER BY created_at DESC
            LIMIT $9
            "#,
        )
                .bind(p_initial_buy_sol)
                .bind(tol_buy.unwrap_or(0.0_f64))
                .bind(cu_limit_i64)
                .bind(cu_price_i64)
                .bind(p_max_sol_cost_lamports)
                .bind(p_spendable_sol_in_lamports)
                .bind(apply_labels)
                .bind(sqlx::types::Json(&labels_json))
                .bind(limit)
        .fetch_all(&self.pool)
        .await?;
    
        Ok(rows.into_iter().map(Token::from).collect())
    }
}
