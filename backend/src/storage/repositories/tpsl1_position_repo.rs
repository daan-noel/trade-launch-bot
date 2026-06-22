impl Tpsl1PositionRepo {
    /// Delete a position by ID.
    pub async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tpsl1_real_positions WHERE id = $1")
            .bind(position_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Position, PositionStatus};

pub struct Tpsl1PositionRepo {
    pool: PgPool,
}

impl Clone for Tpsl1PositionRepo {
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
    entry_price: Option<f64>,
    exit_price: Option<f64>,
    token_program_id: Option<String>,
    entry_tx_signatures: Json<Vec<String>>,
    exit_tx_signatures: Json<Vec<String>>,
    status: String,
    strategy: String,
    rule_id: Uuid,
    entry_token_amount: Option<f64>,
    exit_token_amount: Option<f64>,
    entry_time: Option<DateTime<Utc>>,
    exit_time: Option<DateTime<Utc>>,
    exit_reason: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PositionDbRow> for Position {
    type Error = anyhow::Error;

    fn try_from(r: PositionDbRow) -> Result<Self, Self::Error> {
        let status = match r.status.as_str() {
            "Holding" => PositionStatus::Holding,
            "PendingEntry" => PositionStatus::PendingEntry,
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
            entry_tx_signatures: r.entry_tx_signatures.0,
            exit_tx_signatures: r.exit_tx_signatures.0,
            status,
            strategy: r.strategy,
            rule_id: r.rule_id,
            entry_token_amount: r.entry_token_amount,
            exit_token_amount: r.exit_token_amount,
            entry_time: r.entry_time,
            exit_time: r.exit_time,
            exit_reason: r.exit_reason,
            // TPSL1 has no target (trigger-trade) columns — TPSL2-only feature.
            target_price: None,
            target_token_amount: None,
            target_time: None,
            target_tx: None,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn position_status_str(s: PositionStatus) -> &'static str {
    match s {
        PositionStatus::Holding => "Holding",
        PositionStatus::PendingEntry => "PendingEntry",
        PositionStatus::ExitPending => "ExitPending",
        PositionStatus::End => "End",
        PositionStatus::ExitFailed => "ExitFailed",
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl Tpsl1PositionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Update entry fields for an existing position (entry_tx, entry_token_amount, entry_price, entry_time).
    /// Update entry fields and return the updated row in one round-trip. The
    /// `RETURNING *` lets the caller use the fresh `Position` directly instead of
    /// issuing a follow-up `find_by_id` to read back what it just wrote.
    pub async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_token_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<Position> {
        let row = sqlx::query_as::<_, PositionDbRow>(
            r#"
            UPDATE tpsl1_real_positions
            SET entry_tx_signatures = $2, entry_token_amount = $3, entry_price = $4, entry_time = $5,
                status = 'Holding', updated_at = $6
            WHERE id = $1
            RETURNING id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                      status, strategy, rule_id, entry_token_amount, exit_token_amount,
                      entry_time, exit_time, exit_reason, created_at, updated_at
            "#,
        )
        .bind(position_id)
        .bind(Json(vec![entry_tx]))
        .bind(entry_token_amount)
        .bind(entry_price)
        .bind(entry_time)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await?;

        Position::try_from(row)
    }

    /// Create a new position.
    pub async fn insert(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tpsl1_real_positions
                (id, mint, wallet, token_program_id, entry_price, exit_price, entry_tx_signatures, exit_tx_signatures,
                 status, strategy, rule_id, entry_token_amount, exit_token_amount,
                 entry_time, exit_time, exit_reason, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            "#,
        )
        .bind(position.id)
        .bind(&position.mint)
        .bind(&position.wallet)
        .bind(position.token_program_id.as_ref())
        .bind(position.entry_price)
        .bind(position.exit_price)
        .bind(Json(&position.entry_tx_signatures))
        .bind(Json(&position.exit_tx_signatures))
        .bind(position_status_str(position.status))
        .bind(&position.strategy)
        .bind(position.rule_id)
        .bind(position.entry_token_amount)
        .bind(position.exit_token_amount)
        .bind(position.entry_time)
        .bind(position.exit_time)
        .bind(position.exit_reason.as_ref())
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
            UPDATE tpsl1_real_positions
            SET exit_price = $1, exit_tx_signatures = $2, status = $3, exit_token_amount = $4,
                exit_time = $5, exit_reason = $6, updated_at = $7
            WHERE id = $8
            "#,
        )
        .bind(position.exit_price)
        .bind(Json(&position.exit_tx_signatures))
        .bind(position_status_str(position.status))
        .bind(position.exit_token_amount)
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
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
            WHERE mint = $1 AND status IN ('Holding','PendingEntry')
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
                 SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
            WHERE wallet = $1 AND status IN ('Holding','PendingEntry')
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
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
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
                SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                        status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
                FROM tpsl1_real_positions
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
            UPDATE tpsl1_real_positions
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

    /// Delete real positions left **Holding with no entry fill** (`entry_price IS
    /// NULL`) past the timeout. The in-process buy task deletes its own unentered
    /// row when the buy isn't found (`on_token_created`), so under normal operation
    /// nothing reaches here. But that cleanup lives **inside** the spawned task — if
    /// the process restarts or the task panics mid-buy, the row is reloaded as
    /// `Holding` with no buy task and (being 0-entry) gated out of every exit path:
    /// a permanent zombie that also pins its dead token in `token_cache`
    /// (`is_held`). This is the real-side mirror of the paper reaper. `stale_after`
    /// must comfortably exceed the buy-retry window (`UNENTERED_STALE_MS`) so it
    /// only ever catches orphans, never a buy still legitimately in flight. Returns
    /// rows deleted.
    pub async fn delete_stale_unentered(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let result = sqlx::query(
            r#"
            DELETE FROM tpsl1_real_positions
            WHERE status = 'PendingEntry' AND entry_price IS NULL AND created_at < $1
            "#,
        )
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
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
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

    /// All positions stuck in ExitPending — for the exit-recovery reaper, which
    /// re-drives a sell whose task panicked or was lost to a process restart (the
    /// holding cache only loads `Holding`, so these are otherwise invisible). Small
    /// result set under normal operation; index-served by the `status` predicate.
    pub async fn find_all_exit_pending(&self) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
            WHERE status = 'ExitPending'
            ORDER BY updated_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// All positions with status Holding (for TPSL runtime cache warm-up).
    pub async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PositionDbRow>(
            r#"
            SELECT id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, exit_tx_signatures,
                   status, strategy, rule_id, entry_token_amount, exit_token_amount,
                   entry_time, exit_time, exit_reason, created_at, updated_at
            FROM tpsl1_real_positions
            WHERE status IN ('Holding','PendingEntry')
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(Position::try_from).collect()
    }

    /// Distinct mints with an unsettled (non-`End`) real position. The cache seed
    /// always tracks these regardless of the recency window — the live path can't
    /// re-track an existing mint (a trade for an untracked mint is dropped), so an
    /// open exit would otherwise strand once its token aged out of the seed set.
    pub async fn distinct_unsettled_mints(&self) -> anyhow::Result<Vec<String>> {
        // Positive `IN` over the non-`End` statuses (`End` is the only settled
        // one) so the predicate can be index-served — a negated `status <> 'End'`
        // degrades toward a full scan as the table grows.
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT mint FROM tpsl1_real_positions \
             WHERE status IN ('Holding', 'PendingEntry', 'ExitPending', 'ExitFailed')",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }

    /// Total position count per rule (all statuses).
    pub async fn count_all_by_rule(&self) -> anyhow::Result<Vec<(Uuid, i64)>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            r#"
            SELECT rule_id, COUNT(*)::bigint
            FROM tpsl1_real_positions
            WHERE entry_price IS NOT NULL
            GROUP BY rule_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Per-rule realized-performance aggregate over terminally-closed positions
    /// (`End` / `ExitFailed`, with both an entry and an exit price). Returns
    /// `(rule_id, wins, losses, sum_pnl_sol, sum_pnl_pct)`. A win is a clean
    /// `End` exit sold above entry; every other closed row (breakeven, loss,
    /// failed exit) is a loss. `exit_token_amount` falls back to `0` for a
    /// failed exit that sold nothing, booking the lost bag as a SOL loss —
    /// mirrors [`Position::pnl_sol`] / [`Position::is_win`] so this boot warm-up
    /// agrees exactly with the live counters accumulated in `sync_position`.
    /// Time-range bounding (a future per-rule window selector) drops in as an
    /// extra `created_at` predicate here.
    pub async fn closed_stats_by_rule(&self) -> anyhow::Result<Vec<(Uuid, i64, i64, f64, f64)>> {
        let rows: Vec<(Uuid, i64, i64, f64, f64)> = sqlx::query_as(
            r#"
            SELECT
                rule_id,
                COUNT(*) FILTER (WHERE status = 'End' AND exit_price > entry_price)::bigint,
                COUNT(*) FILTER (WHERE NOT (status = 'End' AND exit_price > entry_price))::bigint,
                COALESCE(SUM(exit_price * COALESCE(exit_token_amount, 0) - entry_price * entry_token_amount), 0)::double precision,
                COALESCE(SUM(((exit_price - entry_price) / NULLIF(entry_price, 0)) * 100.0), 0)::double precision
            FROM tpsl1_real_positions
            WHERE entry_price IS NOT NULL AND exit_price IS NOT NULL
              AND status IN ('End', 'ExitFailed')
            GROUP BY rule_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// ExitPending recovery selection — what the boot/reaper re-drive (`redrive_
// orphaned_exit_pending`) and the stale-failer act on. `find_all_exit_pending`
// must surface exactly the orphaned-mid-exit rows (so a panicked/crashed sell
// is re-armed), and `fail_stale_exit_pending` must terminally fail ONLY the
// rows older than the timeout (re-arming a half-done real exit risks a
// double-sell). DB-integration, so `#[ignore]`d; run against a local Postgres:
//   $env:DATABASE_URL = "postgres://postgres:1220@localhost:5432/meme_bot"
//   cargo test --bin backend tpsl1_position_repo:: -- --ignored --nocapture
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Position;
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;

    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(2).connect(&url).await.ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// Insert a position with the given status, age (`updated_at = now − age`), and
    /// whether it recorded an entry — under its own FK-satisfying rule. Returns the
    /// row id.
    async fn seed_position(
        repo: &Tpsl1PositionRepo,
        mint: &str,
        status: PositionStatus,
        age: Duration,
        entered: bool,
    ) -> Uuid {
        let mut pos = Position::new(mint.to_string(), unique("W"), "TPSL1".to_string(), Uuid::new_v4());
        pos.status = status;
        if entered {
            pos.entry_price = Some(0.001);
            pos.entry_token_amount = Some(1000.0);
        }
        let stamp = Utc::now() - chrono::Duration::from_std(age).unwrap();
        pos.created_at = stamp;
        pos.updated_at = stamp;
        // FK: the position references a strategy rule (rule_id NOT NULL + FK).
        sqlx::query(
            "INSERT INTO tpsl1_strategy_rules (id, rule_name, buy_amount, p_exit_take_profit, p_exit_stop_loss) \
             VALUES ($1, $2, 0.1, 1.5, 0.5)",
        )
        .bind(pos.rule_id)
        .bind(unique("rule-"))
        .execute(&repo.pool)
        .await
        .expect("insert rule");
        repo.insert(&pos).await.expect("insert position");
        pos.id
    }

    async fn cleanup(pool: &PgPool, mint: &str) {
        let _ = sqlx::query(
            "DELETE FROM tpsl1_strategy_rules WHERE id IN \
             (SELECT rule_id FROM tpsl1_real_positions WHERE mint = $1)",
        )
        .bind(mint)
        .execute(pool)
        .await;
        let _ = sqlx::query("DELETE FROM tpsl1_real_positions WHERE mint = $1")
            .bind(mint)
            .execute(pool)
            .await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn find_all_exit_pending_surfaces_only_orphaned_exits() {
        let Some(pool) = test_pool().await else { return };
        let repo = Tpsl1PositionRepo::new(pool.clone());
        let mint = unique("M");

        // A live Holding, a cleanly-ended exit, and one stranded in ExitPending —
        // only the last is invisible to the holding cache and needs re-driving.
        seed_position(&repo, &mint, PositionStatus::Holding, Duration::ZERO, true).await;
        seed_position(&repo, &mint, PositionStatus::End, Duration::ZERO, true).await;
        let pending_id =
            seed_position(&repo, &mint, PositionStatus::ExitPending, Duration::ZERO, true).await;

        let pending: Vec<_> = repo
            .find_all_exit_pending()
            .await
            .expect("query")
            .into_iter()
            .filter(|p| p.mint == mint)
            .collect();
        assert_eq!(pending.len(), 1, "only the ExitPending row is surfaced");
        assert_eq!(pending[0].id, pending_id);
        assert!(pending[0].entry_price.is_some(), "carries the entry the reaper needs to sell");

        cleanup(&pool, &mint).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn fail_stale_exit_pending_terminates_only_aged_rows() {
        let Some(pool) = test_pool().await else { return };
        let repo = Tpsl1PositionRepo::new(pool.clone());
        let mint = unique("M");

        // Fresh ExitPending (sell likely still in flight) vs. one aged past the
        // timeout (orphaned). The reaper re-drives the fresh one; the stale-failer
        // terminally fails only the aged one — re-arming it risks a double-sell.
        let fresh = seed_position(&repo, &mint, PositionStatus::ExitPending, Duration::ZERO, true).await;
        let stale = seed_position(
            &repo,
            &mint,
            PositionStatus::ExitPending,
            Duration::from_secs(3600),
            true,
        )
        .await;

        let failed = repo
            .fail_stale_exit_pending(Duration::from_secs(600))
            .await
            .expect("fail stale");
        assert_eq!(failed, 1, "exactly the aged ExitPending row is failed");

        let fresh_row = repo.find_by_id(fresh).await.unwrap().unwrap();
        let stale_row = repo.find_by_id(stale).await.unwrap().unwrap();
        assert_eq!(
            fresh_row.status,
            PositionStatus::ExitPending,
            "fresh row left ExitPending for the reaper to re-drive"
        );
        assert_eq!(stale_row.status, PositionStatus::ExitFailed, "aged row terminally failed");

        cleanup(&pool, &mint).await;
    }

    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn delete_stale_unentered_reaps_only_aged_zero_entry_holdings() {
        let Some(pool) = test_pool().await else { return };
        let repo = Tpsl1PositionRepo::new(pool.clone());
        let mint = unique("M");

        // Four rows: a freshly-created unentered Holding (buy still in flight), an
        // aged unentered Holding (orphaned zombie — restart/panic lost its task), an
        // aged but ENTERED Holding (a live bag — must survive), and an aged unentered
        // End (already settled). Only the aged 0-entry Holding is a zombie.
        let fresh = seed_position(&repo, &mint, PositionStatus::Holding, Duration::ZERO, false).await;
        let zombie =
            seed_position(&repo, &mint, PositionStatus::Holding, Duration::from_secs(3600), false).await;
        let entered =
            seed_position(&repo, &mint, PositionStatus::Holding, Duration::from_secs(3600), true).await;
        let ended =
            seed_position(&repo, &mint, PositionStatus::End, Duration::from_secs(3600), false).await;

        let reaped = repo
            .delete_stale_unentered(Duration::from_secs(600))
            .await
            .expect("reap stale unentered");
        assert_eq!(reaped, 1, "exactly the aged 0-entry Holding is deleted");

        assert!(repo.find_by_id(zombie).await.unwrap().is_none(), "zombie deleted");
        assert!(repo.find_by_id(fresh).await.unwrap().is_some(), "fresh buy row survives");
        assert!(repo.find_by_id(entered).await.unwrap().is_some(), "entered bag survives");
        assert!(repo.find_by_id(ended).await.unwrap().is_some(), "settled row survives");

        cleanup(&pool, &mint).await;
    }
}
