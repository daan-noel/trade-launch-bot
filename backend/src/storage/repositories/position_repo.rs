use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Position, PositionStatus};

pub struct PositionRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct PositionDbRow {
    id: Uuid,
    mint: String,
    wallet: String,
    entry_price: f64,
    exit_price: Option<f64>,
    entry_tx: String,
    exit_tx: Option<String>,
    status: String,
    strategy: String,
    rule_id: Uuid,
    entry_amount: f64,
    exit_amount: Option<f64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PositionDbRow> for Position {
    type Error = anyhow::Error;

    fn try_from(r: PositionDbRow) -> Result<Self, Self::Error> {
        let status = match r.status.as_str() {
            "Holding" => PositionStatus::Holding,
            "End" => PositionStatus::End,
            other => anyhow::bail!("Unknown position status in DB: {other}"),
        };

        Ok(Self {
            id: r.id,
            mint: r.mint,
            wallet: r.wallet,
            entry_price: r.entry_price,
            exit_price: r.exit_price,
            entry_tx: r.entry_tx,
            exit_tx: r.exit_tx,
            status,
            strategy: r.strategy,
            rule_id: r.rule_id,
            entry_amount: r.entry_amount,
            exit_amount: r.exit_amount,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn position_status_str(s: PositionStatus) -> &'static str {
    match s {
        PositionStatus::Holding => "Holding",
        PositionStatus::End => "End",
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl PositionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new position.
    pub async fn insert(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO positions
                (id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                 status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
        )
        .bind(position.id)
        .bind(&position.mint)
        .bind(&position.wallet)
        .bind(position.entry_price)
        .bind(position.exit_price)
        .bind(&position.entry_tx)
        .bind(&position.exit_tx)
        .bind(position_status_str(position.status))
        .bind(&position.strategy)
        .bind(position.rule_id)
        .bind(position.entry_amount)
        .bind(position.exit_amount)
        .bind(position.created_at)
        .bind(position.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update an existing position (e.g., close it).
    pub async fn update(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE positions
            SET exit_price = $1, exit_tx = $2, status = $3, exit_amount = $4, updated_at = $5
            WHERE id = $6
            "#,
        )
        .bind(position.exit_price)
        .bind(&position.exit_tx)
        .bind(position_status_str(position.status))
        .bind(position.exit_amount)
        .bind(Utc::now())
        .bind(position.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all holding positions for a specific token.
    pub async fn find_holding_by_mint(&self, mint: &str) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE mint = $1 AND status = 'Holding'
            ORDER BY created_at DESC
            "#,
        )
        .bind(mint)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Get all holding positions for a wallet.
    pub async fn find_holding_by_wallet(&self, wallet: &str) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE wallet = $1 AND status = 'Holding'
            ORDER BY created_at DESC
            "#,
        )
        .bind(wallet)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Count active holding positions for a specific rule.
    pub async fn count_holding_by_rule(&self, rule_id: Uuid) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM positions
            WHERE rule_id = $1 AND status = 'Holding'
            "#,
        )
        .bind(rule_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Count all positions created for a specific rule.
    pub async fn count_by_rule(&self, rule_id: Uuid) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM positions
            WHERE rule_id = $1
            "#,
        )
        .bind(rule_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Get a specific position by ID.
    pub async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE id = $1
            "#,
        )
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Position::try_from).transpose()?)
    }

    /// Get a position by entry transaction signature.
    pub async fn find_by_entry_tx(&self, tx_sig: &str) -> anyhow::Result<Option<Position>> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE entry_tx = $1
            "#,
        )
        .bind(tx_sig)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Position::try_from).transpose()?)
    }

    /// Get positions by strategy.
    pub async fn find_by_strategy(&self, strategy: &str) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE strategy = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(strategy)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Get positions by strategy and status.
    pub async fn find_by_strategy_and_status(
        &self,
        strategy: &str,
        status: PositionStatus,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount, created_at, updated_at
            FROM positions
            WHERE strategy = $1 AND status = $2
            ORDER BY created_at DESC
            "#,
        )
        .bind(strategy)
        .bind(position_status_str(status))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }
}
