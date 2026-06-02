impl PositionRepo {
    /// Update exit fields for an existing position (exit_tx, exit_price, exit_time, status).
    pub async fn update_exit(
        &self,
        position_id: Uuid,
        exit_tx: &str,
        exit_price: f64,
        exit_time: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE positions
            SET exit_tx = $2, exit_price = $3, exit_time = $4, status = 'End', updated_at = $5
            WHERE id = $1
            "#,
        )
        .bind(position_id)
        .bind(exit_tx)
        .bind(exit_price)
        .bind(exit_time)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Revert ExitPending to Holding for a position.
    pub async fn revert_exit_pending(&self, position_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE positions SET status = 'Holding', updated_at = $2 WHERE id = $1
            "#,
        )
        .bind(position_id)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a position by ID.
    pub async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM positions WHERE id = $1")
            .bind(position_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Position, PositionStatus};

pub struct PositionRepo {
    pool: PgPool,
}

impl Clone for PositionRepo {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
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
    token_program_id: Option<String>,
    entry_tx: String,
    exit_tx: Option<String>,
    status: String,
    strategy: String,
    rule_id: Uuid,
    entry_amount: f64,
    exit_amount: Option<f64>,
    entry_time: Option<DateTime<Utc>>,
    exit_time: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PositionDbRow> for Position {
    type Error = anyhow::Error;

    fn try_from(r: PositionDbRow) -> Result<Self, Self::Error> {
        let status = match r.status.as_str() {
            "Holding" => PositionStatus::Holding,
            "ExitPending" => PositionStatus::ExitPending,
            "End" => PositionStatus::End,
            other => anyhow::bail!("Unknown position status in DB: {other}"),
        };

        Ok(Self {
            id: r.id,
            mint: r.mint,
            wallet: r.wallet,
            entry_price: r.entry_price,
            exit_price: r.exit_price,
            token_program_id: r.token_program_id,
            entry_tx: r.entry_tx,
            exit_tx: r.exit_tx,
            status,
            strategy: r.strategy,
            rule_id: r.rule_id,
            entry_amount: r.entry_amount,
            exit_amount: r.exit_amount,
            entry_time: r.entry_time,
            exit_time: r.exit_time,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn position_status_str(s: PositionStatus) -> &'static str {
    match s {
        PositionStatus::Holding => "Holding",
        PositionStatus::ExitPending => "ExitPending",
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

    /// Update entry fields for an existing position (entry_tx, entry_amount, entry_price, entry_time).
    pub async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE positions
            SET entry_tx = $2, entry_amount = $3, entry_price = $4, entry_time = $5, updated_at = $6
            WHERE id = $1
            "#,
        )
        .bind(position_id)
        .bind(entry_tx)
        .bind(entry_amount)
        .bind(entry_price)
        .bind(entry_time)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create a new position.
    pub async fn insert(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO positions
                (id, mint, wallet, token_program_id, entry_price, exit_price, entry_tx, exit_tx,
                 status, strategy, rule_id, entry_amount, exit_amount,
                 entry_time, exit_time, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            "#,
        )
        .bind(position.id)
        .bind(&position.mint)
        .bind(&position.wallet)
        .bind(position.token_program_id.as_ref())
        .bind(position.entry_price)
        .bind(position.exit_price)
        .bind(&position.entry_tx)
        .bind(&position.exit_tx)
        .bind(position_status_str(position.status))
        .bind(&position.strategy)
        .bind(position.rule_id)
        .bind(position.entry_amount)
        .bind(position.exit_amount)
        .bind(position.entry_time)
        .bind(position.exit_time)
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
            SET exit_price = $1, exit_tx = $2, status = $3, exit_amount = $4,
                exit_time = $5, updated_at = $6
            WHERE id = $7
            "#,
        )
        .bind(position.exit_price)
        .bind(&position.exit_tx)
        .bind(position_status_str(position.status))
        .bind(position.exit_amount)
        .bind(position.exit_time)
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
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
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
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
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
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
            FROM positions
            WHERE id = $1
            "#,
        )
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Position::try_from).transpose()?)
    }

    /// Get all positions for a specific rule.
    pub async fn find_by_rule(&self, rule_id: Uuid) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
                r#"
                SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                        status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
                FROM positions
                WHERE rule_id = $1
                ORDER BY created_at DESC
                "#,
            )
            .bind(rule_id)
            .fetch_all(&self.pool)
            .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Get a position by entry transaction signature.
    pub async fn find_by_entry_tx(&self, tx_sig: &str) -> anyhow::Result<Option<Position>> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
            FROM positions
            WHERE entry_tx = $1
            "#,
        )
        .bind(tx_sig)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Position::try_from).transpose()?)
    }

    /// Reopen stale exit-pending positions after the timeout has elapsed.
    pub async fn reopen_stale_exit_pending(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(stale_after)?;
        let result = sqlx::query(
            r#"
            UPDATE positions
            SET status = 'Holding', updated_at = $1
                WHERE status = 'ExitPending' AND updated_at < $2
            "#,
        )
        .bind(chrono::Utc::now())
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get positions by strategy.
    pub async fn find_by_strategy(&self, strategy: &str) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
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
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
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

    /// All positions with status Holding (for TPSL runtime cache warm-up).
    pub async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, created_at, updated_at
            FROM positions
            WHERE status = 'Holding'
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Total position count per rule (all statuses).
    pub async fn count_all_by_rule(&self) -> anyhow::Result<Vec<(Uuid, i64)>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            r#"
            SELECT rule_id, COUNT(*)::bigint
            FROM positions
            GROUP BY rule_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}
