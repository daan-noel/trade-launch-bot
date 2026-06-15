//! Persistence for grouped param-sweeps — **generic, table-name-driven**.
//!
//! Each strategy keeps its own `<strategy>_grouped_sweep_runs` / `_groups` /
//! `_results` triple (see the registry's [`GroupedSweepTables`] map). This one
//! repo serves all of them: it's constructed with the resolved table names and
//! interpolates them into otherwise-static SQL. The names come only from fixed
//! internal consts in the registry — never client input — so the interpolation
//! is injection-safe.
//!
//! `save_run` writes a run + its groups + each group's bounded combo rows in one
//! transaction (combo rows bulk-inserted in `chunks(2000)` to stay under the
//! 65535 bind-parameter ceiling). Reads serve the grouped-sweep page: runs list,
//! per-run group summaries, and a group's ranked combo rows.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::grouped_sweep::{
    GroupedSweepGroupSummary, GroupedSweepGroupWrite, GroupedSweepResult, GroupedSweepRun,
};

/// The per-strategy table triple a grouped sweep reads/writes. Field values come
/// from fixed internal consts in [`crate::sweep::registry`], so the repo can
/// safely format them into SQL.
#[derive(Clone, Copy, Debug)]
pub struct GroupedSweepTables {
    pub runs: &'static str,
    pub groups: &'static str,
    pub results: &'static str,
}

pub struct GroupedSweepRepo {
    pool: PgPool,
    tables: GroupedSweepTables,
}

// ---------------------------------------------------------------------------
// DB rows
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct RunDbRow {
    id: Uuid,
    strategy_id: String,
    rule_id: Option<Uuid>,
    source: String,
    method: String,
    created_after: Option<DateTime<Utc>>,
    created_before: Option<DateTime<Utc>>,
    curve_only: bool,
    grouping_spec: sqlx::types::Json<Value>,
    axes_spec: sqlx::types::Json<Value>,
    min_tokens: i32,
    token_count: i32,
    group_count: i32,
    combo_count: i32,
    corpus_hash: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<RunDbRow> for GroupedSweepRun {
    fn from(r: RunDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            source: r.source,
            method: r.method,
            created_after: r.created_after,
            created_before: r.created_before,
            curve_only: r.curve_only,
            grouping_spec: r.grouping_spec.0,
            axes_spec: r.axes_spec.0,
            min_tokens: r.min_tokens,
            token_count: r.token_count,
            group_count: r.group_count,
            combo_count: r.combo_count,
            corpus_hash: r.corpus_hash,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct GroupDbRow {
    id: Uuid,
    group_index: i32,
    group_key: sqlx::types::Json<Value>,
    token_count: i32,
    fired_count: i64,
    best_combo_id: i32,
    best_expectancy_sol: f64,
    best_params: sqlx::types::Json<Value>,
}

impl From<GroupDbRow> for GroupedSweepGroupSummary {
    fn from(r: GroupDbRow) -> Self {
        Self {
            id: r.id,
            group_index: r.group_index,
            group_key: r.group_key.0,
            token_count: r.token_count,
            fired_count: r.fired_count,
            best_combo_id: r.best_combo_id,
            best_expectancy_sol: r.best_expectancy_sol,
            best_params: r.best_params.0,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ResultDbRow {
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

impl From<ResultDbRow> for GroupedSweepResult {
    fn from(r: ResultDbRow) -> Self {
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

impl GroupedSweepRepo {
    pub fn new(pool: PgPool, tables: GroupedSweepTables) -> Self {
        Self { pool, tables }
    }

    /// Persist a run, its groups, and each group's ranked combo rows atomically.
    /// A fresh `group_id` links each group's result rows. `run.group_count` /
    /// `run.combo_count` are taken as given (set by the caller).
    pub async fn save_run(
        &self,
        run: &GroupedSweepRun,
        groups: &[GroupedSweepGroupWrite],
    ) -> anyhow::Result<()> {
        let t = self.tables;
        let mut tx = self.pool.begin().await?;

        let run_sql = format!(
            "INSERT INTO {} \
             (id, strategy_id, rule_id, source, method, created_after, created_before, \
              curve_only, grouping_spec, axes_spec, min_tokens, token_count, group_count, \
              combo_count, corpus_hash, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
            t.runs
        );
        sqlx::query(&run_sql)
            .bind(run.id)
            .bind(&run.strategy_id)
            .bind(run.rule_id)
            .bind(&run.source)
            .bind(&run.method)
            .bind(run.created_after)
            .bind(run.created_before)
            .bind(run.curve_only)
            .bind(sqlx::types::Json(&run.grouping_spec))
            .bind(sqlx::types::Json(&run.axes_spec))
            .bind(run.min_tokens)
            .bind(run.token_count)
            .bind(run.group_count)
            .bind(run.combo_count)
            .bind(&run.corpus_hash)
            .bind(run.created_at)
            .execute(&mut tx)
            .await?;

        let group_sql = format!(
            "INSERT INTO {} \
             (id, run_id, group_index, group_key, token_count, fired_count, \
              best_combo_id, best_expectancy_sol, best_params) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            t.groups
        );
        for g in groups {
            let group_id = Uuid::new_v4();
            sqlx::query(&group_sql)
                .bind(group_id)
                .bind(run.id)
                .bind(g.group_index)
                .bind(sqlx::types::Json(&g.group_key))
                .bind(g.token_count)
                .bind(g.fired_count)
                .bind(g.best_combo_id)
                .bind(g.best_expectancy_sol)
                .bind(sqlx::types::Json(&g.best_params))
                .execute(&mut tx)
                .await?;

            // 28 binds/row → chunk well under the 65535 param ceiling.
            for chunk in g.results.chunks(2000) {
                let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
                    "INSERT INTO {} \
                     (run_id, group_id, combo_id, params, n_fired, n_open, n_closed, win_rate, \
                      total_pnl_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
                      worst_pnl_pct, std_pnl_pct, profit_factor, score, expectancy_sol, \
                      avg_holding_secs, median_holding_secs, exit_take_profit, exit_stop_loss, \
                      exit_trailing, exit_stall, exit_time, exit_liquidity, exit_cohort, exit_open) ",
                    t.results
                ));
                qb.push_values(chunk, |mut b, r| {
                    b.push_bind(run.id)
                        .push_bind(group_id)
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
        }

        tx.commit().await?;
        Ok(())
    }

    /// Runs newest first, bounded by `limit`. (The table is already per-strategy.)
    pub async fn list_runs(&self, limit: i64) -> anyhow::Result<Vec<GroupedSweepRun>> {
        let sql = format!(
            "SELECT id, strategy_id, rule_id, source, method, created_after, created_before, \
                    curve_only, grouping_spec, axes_spec, min_tokens, token_count, group_count, \
                    combo_count, corpus_hash, created_at \
             FROM {} ORDER BY created_at DESC LIMIT $1",
            self.tables.runs
        );
        let rows = sqlx::query_as::<_, RunDbRow>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepRun::from).collect())
    }

    /// Group summaries for a run, best opportunity first (highest expectancy).
    pub async fn list_groups(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Vec<GroupedSweepGroupSummary>> {
        let sql = format!(
            "SELECT id, group_index, group_key, token_count, fired_count, \
                    best_combo_id, best_expectancy_sol, best_params \
             FROM {} WHERE run_id = $1 \
             ORDER BY best_expectancy_sol DESC, group_index ASC",
            self.tables.groups
        );
        let rows = sqlx::query_as::<_, GroupDbRow>(&sql)
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepGroupSummary::from).collect())
    }

    /// Every ranked combo row for one group, ordered by combo id (the table
    /// sorts/filters client-side). Scoped by `run_id` too as a safety guard.
    pub async fn list_results(
        &self,
        run_id: Uuid,
        group_id: Uuid,
    ) -> anyhow::Result<Vec<GroupedSweepResult>> {
        let sql = format!(
            "SELECT combo_id, params, n_fired, n_open, n_closed, win_rate, total_pnl_sol, \
                    mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct, \
                    std_pnl_pct, profit_factor, score, expectancy_sol, avg_holding_secs, \
                    median_holding_secs, exit_take_profit, exit_stop_loss, exit_trailing, \
                    exit_stall, exit_time, exit_liquidity, exit_cohort, exit_open \
             FROM {} WHERE run_id = $1 AND group_id = $2 ORDER BY combo_id",
            self.tables.results
        );
        let rows = sqlx::query_as::<_, ResultDbRow>(&sql)
            .bind(run_id)
            .bind(group_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepResult::from).collect())
    }
}
