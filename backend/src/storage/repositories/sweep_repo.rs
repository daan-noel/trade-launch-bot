//! Persistence for strategy param-sweep results (`sweep_runs` / `sweep_results`).
//!
//! Written once per `sweep` CLI run (a run row + its bounded set of per-combo
//! rows, in one transaction); read by the dashboard's per-strategy sweep page.
//! The per-run result set is `combos` rows (hundreds to low thousands), so it is
//! listed whole — the table sorts/filters client-side.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::sweep::{SweepResult, SweepRun};

pub struct SweepRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct SweepRunDbRow {
    id: Uuid,
    strategy: String,
    rule_id: Option<Uuid>,
    source: String,
    method: String,
    token_count: i32,
    combo_count: i32,
    corpus_hash: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<SweepRunDbRow> for SweepRun {
    fn from(r: SweepRunDbRow) -> Self {
        Self {
            id: r.id,
            strategy: r.strategy,
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
struct SweepResultDbRow {
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
    profit_factor: Option<f64>,
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

impl From<SweepResultDbRow> for SweepResult {
    fn from(r: SweepResultDbRow) -> Self {
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
            profit_factor: r.profit_factor,
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

impl SweepRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a run and all its per-combo result rows atomically. Results are
    /// bulk-inserted in chunks to stay under Postgres' bind-parameter limit.
    pub async fn save_run(&self, run: &SweepRun, results: &[SweepResult]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO sweep_runs \
             (id, strategy, rule_id, source, method, token_count, combo_count, corpus_hash, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(run.id)
        .bind(&run.strategy)
        .bind(run.rule_id)
        .bind(&run.source)
        .bind(&run.method)
        .bind(run.token_count)
        .bind(run.combo_count)
        .bind(&run.corpus_hash)
        .bind(run.created_at)
        .execute(&mut tx)
        .await?;

        // 24 binds/row → chunk well under the 65535 param ceiling.
        for chunk in results.chunks(2000) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO sweep_results \
                 (run_id, combo_id, params, n_fired, n_open, n_closed, win_rate, \
                  total_pnl_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
                  worst_pnl_pct, profit_factor, expectancy_sol, avg_holding_secs, \
                  median_holding_secs, exit_take_profit, exit_stop_loss, exit_trailing, \
                  exit_stall, exit_time, exit_liquidity, exit_cohort, exit_open) ",
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
                    .push_bind(r.profit_factor)
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

    /// Runs for a strategy, newest first, bounded by `limit`.
    pub async fn list_runs(&self, strategy: &str, limit: i64) -> anyhow::Result<Vec<SweepRun>> {
        let rows = sqlx::query_as::<_, SweepRunDbRow>(
            "SELECT id, strategy, rule_id, source, method, token_count, combo_count, \
                    corpus_hash, created_at \
             FROM sweep_runs WHERE strategy = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(strategy)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(SweepRun::from).collect())
    }

    /// Every per-combo result row for a run, ordered by combo id. Bounded by the
    /// run's combo count (the table paginates/sorts client-side).
    pub async fn list_results(&self, run_id: Uuid) -> anyhow::Result<Vec<SweepResult>> {
        let rows = sqlx::query_as::<_, SweepResultDbRow>(
            "SELECT combo_id, params, n_fired, n_open, n_closed, win_rate, total_pnl_sol, \
                    mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct, \
                    profit_factor, expectancy_sol, avg_holding_secs, median_holding_secs, \
                    exit_take_profit, exit_stop_loss, exit_trailing, exit_stall, exit_time, \
                    exit_liquidity, exit_cohort, exit_open \
             FROM sweep_results WHERE run_id = $1 ORDER BY combo_id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(SweepResult::from).collect())
    }
}
