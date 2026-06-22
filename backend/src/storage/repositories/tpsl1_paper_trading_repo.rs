use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{PaperRun, PaperRunStatus, Position, PositionStatus};

/// Repository for the paper-trading tables (`tpsl1_paper_test_run` + `tpsl1_paper_positions`),
/// kept entirely separate from the real `positions` table. Positions are mapped
/// to/from the shared [`Position`] model (the `run_id` binding lives only on the
/// row); runs use [`PaperRun`].
pub struct Tpsl1PaperTradingRepo {
    pool: PgPool,
}

impl Clone for Tpsl1PaperTradingRepo {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// DB rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct PaperRunDbRow {
    id: Uuid,
    rule_id: Uuid,
    run_seq: i64,
    status: String,
    max_total_tokens: Option<i64>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl TryFrom<PaperRunDbRow> for PaperRun {
    type Error = anyhow::Error;

    fn try_from(r: PaperRunDbRow) -> Result<Self, Self::Error> {
        let status: PaperRunStatus = r.status.parse().map_err(|e: String| anyhow::anyhow!(e))?;
        Ok(Self {
            id: r.id,
            rule_id: r.rule_id,
            run_seq: r.run_seq,
            status,
            max_total_tokens: r.max_total_tokens.map(|v| v as u64),
            started_at: r.started_at,
            finished_at: r.finished_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct PaperPositionDbRow {
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

impl TryFrom<PaperPositionDbRow> for Position {
    type Error = anyhow::Error;

    fn try_from(r: PaperPositionDbRow) -> Result<Self, Self::Error> {
        let status = match r.status.as_str() {
            "Holding" => PositionStatus::Holding,
            "Arming" => PositionStatus::Arming,
            "BuySubmitted" => PositionStatus::BuySubmitted,
            "ExitPending" => PositionStatus::ExitPending,
            "End" => PositionStatus::End,
            "ExitFailed" => PositionStatus::ExitFailed,
            other => anyhow::bail!("Unknown paper position status in DB: {other}"),
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
            // Paper sends no buys, so paper tables carry no submitted-buy column —
            // always empty for a paper position (real-only durable marker).
            submitted_buy_signatures: Vec::new(),
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
        PositionStatus::Arming => "Arming",
        PositionStatus::BuySubmitted => "BuySubmitted",
        PositionStatus::ExitPending => "ExitPending",
        PositionStatus::End => "End",
        PositionStatus::ExitFailed => "ExitFailed",
    }
}

const POSITION_COLS: &str = "id, mint, wallet, entry_price, exit_price, token_program_id, entry_tx_signatures, \
     exit_tx_signatures, status, strategy, rule_id, entry_token_amount, exit_token_amount, entry_time, exit_time, \
     exit_reason, created_at, updated_at";

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl Tpsl1PaperTradingRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ---- runs -------------------------------------------------------------

    /// Start a fresh run for a rule. Deletes the rule's prior run (and, via
    /// CASCADE, its positions) so only the latest run is ever retained, then
    /// inserts a new `Running` row with a monotonically incremented `run_seq`.
    pub async fn start_run(
        &self,
        rule_id: Uuid,
        max_total_tokens: Option<u64>,
    ) -> anyhow::Result<PaperRun> {
        let mut tx = self.pool.begin().await?;

        let prev_seq: Option<i64> =
            sqlx::query_scalar("SELECT MAX(run_seq) FROM tpsl1_paper_test_run WHERE rule_id = $1")
                .bind(rule_id)
                .fetch_one(&mut *tx)
                .await?;
        let run_seq = prev_seq.unwrap_or(0) + 1;

        sqlx::query("DELETE FROM tpsl1_paper_test_run WHERE rule_id = $1")
            .bind(rule_id)
            .execute(&mut *tx)
            .await?;

        let id = Uuid::new_v4();
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO tpsl1_paper_test_run (id, rule_id, run_seq, status, max_total_tokens, started_at, finished_at)
            VALUES ($1, $2, $3, 'Running', $4, $5, NULL)
            "#,
        )
        .bind(id)
        .bind(rule_id)
        .bind(run_seq)
        .bind(max_total_tokens.map(|v| v as i64))
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(PaperRun {
            id,
            rule_id,
            run_seq,
            status: PaperRunStatus::Running,
            max_total_tokens,
            started_at: now,
            finished_at: None,
        })
    }

    /// The rule's latest run (the only one retained), if any.
    pub async fn current_run(&self, rule_id: Uuid) -> anyhow::Result<Option<PaperRun>> {
        let row = sqlx::query_as::<_, PaperRunDbRow>(
            r#"
            SELECT id, rule_id, run_seq, status, max_total_tokens, started_at, finished_at
            FROM tpsl1_paper_test_run
            WHERE rule_id = $1
            ORDER BY run_seq DESC
            LIMIT 1
            "#,
        )
        .bind(rule_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(PaperRun::try_from).transpose()
    }

    /// The latest run for every rule that has one (used to warm the cache).
    pub async fn find_all_runs(&self) -> anyhow::Result<Vec<PaperRun>> {
        let rows = sqlx::query_as::<_, PaperRunDbRow>(
            r#"
            SELECT DISTINCT ON (rule_id)
                   id, rule_id, run_seq, status, max_total_tokens, started_at, finished_at
            FROM tpsl1_paper_test_run
            ORDER BY rule_id, run_seq DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(PaperRun::try_from).collect()
    }

    /// Set a run's status, optionally stamping `finished_at = now()`.
    pub async fn mark_run_status(
        &self,
        run_id: Uuid,
        status: PaperRunStatus,
        finished: bool,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE tpsl1_paper_test_run
            SET status = $2,
                finished_at = CASE WHEN $3 THEN now() ELSE finished_at END
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(status.as_str())
        .bind(finished)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Resume a run: set it back to `Running` and clear `finished_at` (the manual
    /// "continue" path after a pause or finish). Recorded positions and counters
    /// are left untouched so the run picks up exactly where it left off.
    pub async fn resume_run(&self, run_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE tpsl1_paper_test_run SET status = 'Running', finished_at = NULL WHERE id = $1",
        )
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete every paper run for a rule (and, via CASCADE, all their recorded
    /// positions) — the "Clear results" action. The caller gates this to idle
    /// paper rules, so no in-flight run is wiped under itself. Returns runs
    /// deleted.
    pub async fn clear_runs(&self, rule_id: Uuid) -> anyhow::Result<u64> {
        let result = sqlx::query("DELETE FROM tpsl1_paper_test_run WHERE rule_id = $1")
            .bind(rule_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ---- positions --------------------------------------------------------

    /// Create a paper position bound to a run.
    pub async fn insert(&self, position: &Position, run_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tpsl1_paper_positions
                (id, run_id, mint, wallet, token_program_id, entry_price, exit_price, entry_tx_signatures, exit_tx_signatures,
                 status, strategy, rule_id, entry_token_amount, exit_token_amount,
                 entry_time, exit_time, exit_reason, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            "#,
        )
        .bind(position.id)
        .bind(run_id)
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

    /// Update entry fields after a (simulated) fill is recorded.
    /// Update entry fields and return the updated row in one round-trip, so the
    /// caller can use the fresh `Position` directly instead of a follow-up
    /// `find_by_id` to read back what it just wrote.
    pub async fn update_entry(
        &self,
        position_id: Uuid,
        entry_tx: &str,
        entry_token_amount: f64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<Position> {
        let row = sqlx::query_as::<_, PaperPositionDbRow>(&format!(
            r#"
            UPDATE tpsl1_paper_positions
            SET entry_tx_signatures = $2, entry_token_amount = $3, entry_price = $4, entry_time = $5,
                status = 'Holding', updated_at = $6
            WHERE id = $1
            RETURNING {POSITION_COLS}
            "#
        ))
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

    /// Close a position with the (simulated) exit fill, recording the exit
    /// reason that fired (`"TakeProfit"`, `"TrailingStop"`, …). Returns the
    /// updated row via `RETURNING` so the caller can sync runtime state without
    /// a follow-up `find_by_id` read-back.
    pub async fn update_exit(
        &self,
        position_id: Uuid,
        exit_tx: &str,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        exit_reason: &str,
    ) -> anyhow::Result<Position> {
        let row = sqlx::query_as::<_, PaperPositionDbRow>(&format!(
            r#"
            UPDATE tpsl1_paper_positions
            SET exit_tx_signatures = $2, exit_price = $3, exit_time = $4, exit_reason = $5,
                status = 'End', updated_at = $6
            WHERE id = $1
            RETURNING {POSITION_COLS}
            "#
        ))
        .bind(position_id)
        .bind(Json(vec![exit_tx]))
        .bind(exit_price)
        .bind(exit_time)
        .bind(exit_reason)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await?;
        Position::try_from(row)
    }

    /// Persist status/exit fields of an existing position (e.g. mark ExitPending).
    pub async fn update(&self, position: &Position) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE tpsl1_paper_positions
            SET exit_price = $1, exit_tx_signatures = $2, status = $3, exit_token_amount = $4,
                exit_time = $5, updated_at = $6
            WHERE id = $7
            "#,
        )
        .bind(position.exit_price)
        .bind(Json(&position.exit_tx_signatures))
        .bind(position_status_str(position.status))
        .bind(position.exit_token_amount)
        .bind(position.exit_time)
        .bind(Utc::now())
        .bind(position.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Terminally mark an ExitPending position as ExitFailed (no confirming exit
    /// trade was indexed within the poll window). Records the price/time at which
    /// the exit condition was met (the trigger price) so the row carries a
    /// hypothetical exit. The position is never re-evaluated for exit again.
    pub async fn mark_exit_failed(
        &self,
        position_id: Uuid,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        exit_reason: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE tpsl1_paper_positions \
             SET status = 'ExitFailed', exit_price = $2, exit_time = $3, exit_reason = $4, updated_at = $5 \
             WHERE id = $1",
        )
        .bind(position_id)
        .bind(exit_price)
        .bind(exit_time)
        .bind(exit_reason)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a paper position (e.g. a 0-entry row that never filled).
    pub async fn delete_position(&self, position_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tpsl1_paper_positions WHERE id = $1")
            .bind(position_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, position_id: Uuid) -> anyhow::Result<Option<Position>> {
        let row = sqlx::query_as::<_, PaperPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM tpsl1_paper_positions WHERE id = $1"
        ))
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(Position::try_from).transpose()
    }

    /// All positions in a run, oldest first (for the run result aggregation).
    pub async fn find_by_run(
        &self,
        run_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PaperPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM tpsl1_paper_positions WHERE run_id = $1 ORDER BY created_at ASC LIMIT $2 OFFSET $3"
        ))
        .bind(run_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Position::try_from).collect()
    }

    /// All Holding paper positions across every (current) run — warms the cache.
    pub async fn find_all_holding(&self) -> anyhow::Result<Vec<Position>> {
        let rows = sqlx::query_as::<_, PaperPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM tpsl1_paper_positions WHERE status IN ('Holding','Arming','BuySubmitted') ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Position::try_from).collect()
    }

    /// Position counts for every run in a single GROUP BY, so the cache load can
    /// look counts up by run id instead of issuing one count-per-run query
    /// (the previous N+1). Runs with zero positions are absent from the map.
    pub async fn count_by_run_all(&self) -> anyhow::Result<std::collections::HashMap<Uuid, i64>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT run_id, COUNT(*) FROM tpsl1_paper_positions WHERE entry_price IS NOT NULL GROUP BY run_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// Per-run realized-performance aggregate (paper) — same shape and semantics
    /// as the real repo's `closed_stats_by_rule`, but keyed by `run_id` so the
    /// cache warm-up attributes each paper rule's stats to its current run.
    /// `(wins, losses, sum_pnl_sol, sum_pnl_pct)` per run; runs with no closed
    /// positions are absent from the map.
    pub async fn closed_stats_by_run_all(
        &self,
    ) -> anyhow::Result<std::collections::HashMap<Uuid, (i64, i64, f64, f64)>> {
        let rows: Vec<(Uuid, i64, i64, f64, f64)> = sqlx::query_as(
            r#"
            SELECT
                run_id,
                COUNT(*) FILTER (WHERE status = 'End' AND exit_price > entry_price)::bigint,
                COUNT(*) FILTER (WHERE NOT (status = 'End' AND exit_price > entry_price))::bigint,
                COALESCE(SUM(exit_price * COALESCE(exit_token_amount, 0) - entry_price * entry_token_amount), 0)::double precision,
                COALESCE(SUM(((exit_price - entry_price) / NULLIF(entry_price, 0)) * 100.0), 0)::double precision
            FROM tpsl1_paper_positions
            WHERE entry_price IS NOT NULL AND exit_price IS NOT NULL
              AND status IN ('End', 'ExitFailed')
            GROUP BY run_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, w, l, s, p)| (id, (w, l, s, p)))
            .collect())
    }

    /// Terminally fail paper positions stuck in ExitPending past the staleness
    /// window (mirrors the real-position safety net). Such a row was orphaned —
    /// the exit task normally resolves within seconds — so fail it rather than
    /// re-arm it. Returns rows failed.
    pub async fn fail_stale_exit_pending(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let result = sqlx::query(
            r#"
            UPDATE tpsl1_paper_positions
            SET status = 'ExitFailed', updated_at = $1
            WHERE status = 'ExitPending' AND updated_at < $2
            "#,
        )
        .bind(Utc::now())
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Delete orphaned **unentered** Holding positions older than the cutoff: rows
    /// with no recorded fill (`entry_price IS NULL`, or `<= 0` for legacy rows) that
    /// have outlived any plausible arming window. The `spawn_entry_fill_poll` task
    /// normally enters or deletes such a row within its arming window, but that task
    /// lives only in memory — a backend restart or paper-run resume reloads the row
    /// from the DB without re-arming it, leaving a zombie that is never entered (no
    /// poll), never exited (the clock sweep skips unentered rows), and never deleted.
    /// This reaps them. The cutoff must exceed the largest real arming window so a
    /// live poll is never raced. Returns rows deleted.
    ///
    /// NOTE: the predicate must match `NULL` explicitly — unentered rows are inserted
    /// with `entry_price = NULL`, and `NULL <= 0` is `NULL` (not true) in SQL, so a
    /// bare `<= 0` test silently skips every orphan (mirrors the real-side reaper,
    /// which tests `entry_price IS NULL`).
    pub async fn delete_stale_unentered(
        &self,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let result = sqlx::query(
            r#"
            DELETE FROM tpsl1_paper_positions
            WHERE status = 'Arming' AND (entry_price IS NULL OR entry_price <= 0) AND created_at < $1
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}
