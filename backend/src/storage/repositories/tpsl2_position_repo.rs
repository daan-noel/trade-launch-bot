impl Tpsl2PositionRepo {
    /// Delete a position by ID.
    pub async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tpsl2_real_positions WHERE id = $1")
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

pub struct Tpsl2PositionRepo {
    pool: PgPool,
}

impl Clone for Tpsl2PositionRepo {
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
    exit_reason: Option<String>,
    target_price: Option<f64>,
    target_amount: Option<f64>,
    target_time: Option<DateTime<Utc>>,
    target_tx: Option<String>,
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
            "ExitFailed" => PositionStatus::ExitFailed,
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
            exit_reason: r.exit_reason,
            target_price: r.target_price,
            target_amount: r.target_amount,
            target_time: r.target_time,
            target_tx: r.target_tx,
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
        PositionStatus::ExitFailed => "ExitFailed",
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl Tpsl2PositionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Update entry fields for an existing position (entry_tx, entry_amount, entry_price, entry_time).
    /// Update entry fields and return the updated row in one round-trip. The
    /// `RETURNING *` lets the caller use the fresh `Position` directly instead of
    /// issuing a follow-up `find_by_id` to read back what it just wrote.
    pub async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<Position> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            UPDATE tpsl2_real_positions
            SET entry_tx = $2, entry_amount = $3, entry_price = $4, entry_time = $5, updated_at = $6
            WHERE id = $1
            RETURNING id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                      status, strategy, rule_id, entry_amount, exit_amount,
                      entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            "#,
        )
        .bind(position_id)
        .bind(entry_tx)
        .bind(entry_amount)
        .bind(entry_price)
        .bind(entry_time)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await?;

        Position::try_from(row)
    }

    /// Record the target (trigger-trade) snapshot — the scalp-entry signal trade
    /// that armed this position — and return the updated row in one round-trip.
    /// Mirrors [`update_entry`]; the four `target_*` columns are written
    /// independently of `entry_*` so the gap between the targeted point and the
    /// actual fill can be derived later.
    pub async fn update_target(
        &self,
        position_id: Uuid,
        target_price: f64,
        target_amount: f64,
        target_time: DateTime<Utc>,
        target_tx: &str,
    ) -> anyhow::Result<Position> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            UPDATE tpsl2_real_positions
            SET target_price = $2, target_amount = $3, target_time = $4, target_tx = $5, updated_at = $6
            WHERE id = $1
            RETURNING id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                      status, strategy, rule_id, entry_amount, exit_amount,
                      entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            "#,
        )
        .bind(position_id)
        .bind(target_price)
        .bind(target_amount)
        .bind(target_time)
        .bind(target_tx)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await?;

        Position::try_from(row)
    }

    /// Create a new position.
    pub async fn insert(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tpsl2_real_positions
                (id, mint, wallet, token_program_id, entry_price, exit_price, entry_tx, exit_tx,
                 status, strategy, rule_id, entry_amount, exit_amount,
                 entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
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
        .bind(position.exit_reason.as_ref())
        .bind(position.target_price)
        .bind(position.target_amount)
        .bind(position.target_time)
        .bind(position.target_tx.as_ref())
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
            UPDATE tpsl2_real_positions
            SET exit_price = $1, exit_tx = $2, status = $3, exit_amount = $4,
                exit_time = $5, exit_reason = $6, updated_at = $7
            WHERE id = $8
            "#,
        )
        .bind(position.exit_price)
        .bind(&position.exit_tx)
        .bind(position_status_str(position.status))
        .bind(position.exit_amount)
        .bind(position.exit_time)
        .bind(position.exit_reason.as_ref())
        .bind(Utc::now())
        .bind(position.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get holding positions for a specific token (page-bounded, newest first).
    pub async fn find_holding_by_mint(
        &self,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            FROM tpsl2_real_positions
            WHERE mint = $1 AND status = 'Holding'
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(mint)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Get holding positions for a wallet (page-bounded, newest first).
    pub async fn find_holding_by_wallet(
        &self,
        wallet: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            FROM tpsl2_real_positions
            WHERE wallet = $1 AND status = 'Holding'
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(wallet)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Get a specific position by ID.
    pub async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            FROM tpsl2_real_positions
            WHERE id = $1
            "#,
        )
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Position::try_from).transpose()?)
    }

    /// Get positions for a specific rule (page-bounded, newest first).
    pub async fn find_by_rule(
        &self,
        rule_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
                r#"
                SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                        status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
                FROM tpsl2_real_positions
                WHERE rule_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(rule_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Terminally fail positions stuck in ExitPending past the timeout. Under
    /// normal operation the exit task resolves ExitPending to End/ExitFailed
    /// within seconds; a row lingering this long was orphaned (e.g. the process
    /// restarted mid-exit), so fail it rather than re-arm it — re-arming a
    /// half-done real exit risks a double-sell. Returns rows failed.
    pub async fn fail_stale_exit_pending(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(stale_after)?;
        let result = sqlx::query(
            r#"
            UPDATE tpsl2_real_positions
            SET status = 'ExitFailed', updated_at = $1
                WHERE status = 'ExitPending' AND updated_at < $2
            "#,
        )
        .bind(chrono::Utc::now())
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get positions by strategy (page-bounded, newest first). Grows unbounded
    /// otherwise — this is the HTTP list view, so always paginate.
    pub async fn find_by_strategy(
        &self,
        strategy: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx, exit_tx,
                   status, strategy, rule_id, entry_amount, exit_amount,
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            FROM tpsl2_real_positions
            WHERE strategy = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(strategy)
        .bind(limit)
        .bind(offset)
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
                   entry_time, exit_time, exit_reason, target_price, target_amount, target_time, target_tx, created_at, updated_at
            FROM tpsl2_real_positions
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
            FROM tpsl2_real_positions
            GROUP BY rule_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}
