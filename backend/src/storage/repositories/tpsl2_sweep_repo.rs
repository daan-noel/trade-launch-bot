//! Persistence for TPSL2 param-sweep results (`tpsl2_sweep_runs` /
//! `tpsl2_sweep_results`).
//!
//! Written once per `tpsl2-sweep` CLI run (a run row + its bounded set of
//! per-combo rows, in one transaction); read by the dashboard's TPSL2 sweep
//! page. The per-run result set is `combos` rows (hundreds to low thousands), so
//! it is listed whole — the table sorts/filters client-side.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::tpsl2_sweep::{Tpsl2SweepResult, Tpsl2SweepRun};

pub struct Tpsl2SweepRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct Tpsl2SweepRunDbRow {
    id: Uuid,
    rule_id: Option<Uuid>,
    source: String,
    method: String,
    token_count: i32,
    combo_count: i32,
    corpus_hash: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<Tpsl2SweepRunDbRow> for Tpsl2SweepRun {
    fn from(r: Tpsl2SweepRunDbRow) -> Self {
        Self {
            id: r.id,
            rule_id: r.rule_id,
            source: r.source,
            method: r.method,
            token_count: r.token_count,
            combo_count: r.combo_count,
            corpus_hash: r.corpus_hash,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct Tpsl2SweepResultDbRow {
    combo_id: i32,
    params: sqlx::types::Json<Value>,
    n_fired: i64,
    n_open: i64,
    n_closed: i64,
    win_rate: f64,
    total_pnl_sol: f64,
    mean_pnl_pct: f64,
    median_pnl_pct: f64,
    p90_pnl_pct: f64,
    best_pnl_pct: f64,
    worst_pnl_pct: f64,
    std_pnl_pct: f64,
    profit_factor: Option<f64>,
    score: Option<f64>,
    expectancy_sol: f64,
    avg_holding_secs: f64,
    median_holding_secs: f64,
    exit_take_profit: i32,
    exit_stop_loss: i32,
    exit_trailing: i32,
    exit_stall: i32,
    exit_time: i32,
    exit_liquidity: i32,
    exit_cohort: i32,
    exit_open: i32,
}

impl From<Tpsl2SweepResultDbRow> for Tpsl2SweepResult {
    fn from(r: Tpsl2SweepResultDbRow) -> Self {
        Self {
            combo_id: r.combo_id,
            params: r.params.0,
            n_fired: r.n_fired,
            n_open: r.n_open,
            n_closed: r.n_closed,
            win_rate: r.win_rate,
            total_pnl_sol: r.total_pnl_sol,
            mean_pnl_pct: r.mean_pnl_pct,
            median_pnl_pct: r.median_pnl_pct,
            p90_pnl_pct: r.p90_pnl_pct,
            best_pnl_pct: r.best_pnl_pct,
            worst_pnl_pct: r.worst_pnl_pct,
            std_pnl_pct: r.std_pnl_pct,
            profit_factor: r.profit_factor,
            score: r.score,
            expectancy_sol: r.expectancy_sol,
            avg_holding_secs: r.avg_holding_secs,
            median_holding_secs: r.median_holding_secs,
            exit_take_profit: r.exit_take_profit,
            exit_stop_loss: r.exit_stop_loss,
            exit_trailing: r.exit_trailing,
            exit_stall: r.exit_stall,
            exit_time: r.exit_time,
            exit_liquidity: r.exit_liquidity,
            exit_cohort: r.exit_cohort,
            exit_open: r.exit_open,
        }
    }
}

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl Tpsl2SweepRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a run and all its per-combo result rows atomically. Results are
    /// bulk-inserted in chunks to stay under Postgres' bind-parameter limit.
    pub async fn save_run(
        &self,
        run: &Tpsl2SweepRun,
        results: &[Tpsl2SweepResult],
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO tpsl2_sweep_runs \
             (id, rule_id, source, method, token_count, combo_count, corpus_hash, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(run.id)
        .bind(run.rule_id)
        .bind(&run.source)
        .bind(&run.method)
        .bind(run.token_count)
        .bind(run.combo_count)
        .bind(&run.corpus_hash)
        .bind(run.created_at)
        .execute(&mut tx)
        .await?;

        // 26 binds/row → chunk well under the 65535 param ceiling.
        for chunk in results.chunks(2000) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO tpsl2_sweep_results \
                 (run_id, combo_id, params, n_fired, n_open, n_closed, win_rate, \
                  total_pnl_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
                  worst_pnl_pct, std_pnl_pct, profit_factor, score, expectancy_sol, \
                  avg_holding_secs, median_holding_secs, exit_take_profit, exit_stop_loss, \
                  exit_trailing, exit_stall, exit_time, exit_liquidity, exit_cohort, exit_open) ",
            );
            qb.push_values(chunk, |mut b, r| {
                b.push_bind(run.id)
                    .push_bind(r.combo_id)
                    .push_bind(sqlx::types::Json(&r.params))
                    .push_bind(r.n_fired)
                    .push_bind(r.n_open)
                    .push_bind(r.n_closed)
                    .push_bind(r.win_rate)
                    .push_bind(r.total_pnl_sol)
                    .push_bind(r.mean_pnl_pct)
                    .push_bind(r.median_pnl_pct)
                    .push_bind(r.p90_pnl_pct)
                    .push_bind(r.best_pnl_pct)
                    .push_bind(r.worst_pnl_pct)
                    .push_bind(r.std_pnl_pct)
                    .push_bind(r.profit_factor)
                    .push_bind(r.score)
                    .push_bind(r.expectancy_sol)
                    .push_bind(r.avg_holding_secs)
                    .push_bind(r.median_holding_secs)
                    .push_bind(r.exit_take_profit)
                    .push_bind(r.exit_stop_loss)
                    .push_bind(r.exit_trailing)
                    .push_bind(r.exit_stall)
                    .push_bind(r.exit_time)
                    .push_bind(r.exit_liquidity)
                    .push_bind(r.exit_cohort)
                    .push_bind(r.exit_open);
            });
            qb.build().execute(&mut tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Runs newest first, bounded by `limit`.
    pub async fn list_runs(&self, limit: i64) -> anyhow::Result<Vec<Tpsl2SweepRun>> {
        let rows = sqlx::query_as::<_, Tpsl2SweepRunDbRow>(
            "SELECT id, rule_id, source, method, token_count, combo_count, \
                    corpus_hash, created_at \
             FROM tpsl2_sweep_runs ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Tpsl2SweepRun::from).collect())
    }

    /// Every per-combo result row for a run, ordered by combo id. Bounded by the
    /// run's combo count (the table paginates/sorts client-side).
    pub async fn list_results(&self, run_id: Uuid) -> anyhow::Result<Vec<Tpsl2SweepResult>> {
        let rows = sqlx::query_as::<_, Tpsl2SweepResultDbRow>(
            "SELECT combo_id, params, n_fired, n_open, n_closed, win_rate, total_pnl_sol, \
                    mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct, \
                    std_pnl_pct, profit_factor, score, expectancy_sol, avg_holding_secs, \
                    median_holding_secs, exit_take_profit, exit_stop_loss, exit_trailing, \
                    exit_stall, exit_time, exit_liquidity, exit_cohort, exit_open \
             FROM tpsl2_sweep_results WHERE run_id = $1 ORDER BY combo_id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Tpsl2SweepResult::from).collect())
    }
}
