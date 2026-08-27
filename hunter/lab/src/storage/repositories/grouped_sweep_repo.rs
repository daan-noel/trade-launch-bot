//! Persistence for grouped param-sweeps — **generic, table-name-driven**.
//!
//! Each strategy keeps its own `<strategy>_grouped_sweep_runs` / `_groups` /
//! `_results` triple (see the registry's [`GroupedSweepTables`] map). This one
//! repo serves all of them: it's constructed with the resolved table names and
//! interpolates them into otherwise-static SQL. The names come only from fixed
//! internal consts in the registry — never client input — so the interpolation
//! is injection-safe.
//!
//! Writes are **incremental** (incremental persistence): `insert_run` writes
//! the run header up front (`status = 'running'`), `append_group` commits each
//! completed group + its bounded combo rows in its own transaction (combo rows
//! bulk-inserted in `chunks(2000)` to stay under the 65535 bind-parameter
//! ceiling) and bumps `groups_done`, and `finalize_run` stamps the terminal
//! status — so a cancel/crash mid-sweep keeps whatever already committed.
//! `reconcile_orphaned_runs` marks runs left at `running` by a killed process as
//! `cancelled` at startup. Reads serve the grouped-sweep page: runs list, per-run
//! group summaries, and a group's ranked combo rows.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::grouped_sweep::{
    GroupedSweepGroupSummary, GroupedSweepGroupWrite, GroupedSweepResult, GroupedSweepRun,
};
use trading_core::config::constants::tidy_sol_decimal;

/// The per-strategy table triple a grouped sweep reads/writes. Field values come
/// from fixed internal consts in [`crate::sweep::registry`], so the repo can
/// safely format them into SQL.
#[derive(Clone, Copy, Debug)]
pub struct GroupedSweepTables {
    pub runs: &'static str,
    pub groups: &'static str,
    pub results: &'static str,
    /// Per-run combo→`params` dictionary (migration `0007`): `params` is identical
    /// for a `(run_id, combo_id)` across every group, so it's stored once here
    /// instead of on every `(group, combo)` results row, and JOINed back on read.
    pub combos: &'static str,
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
    corpus_last_trade_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    status: String,
    groups_done: i32,
    ix_labels_filter: Option<sqlx::types::Json<Value>>,
    field_filters: Option<sqlx::types::Json<Value>>,
    fingerprint_id: Option<Uuid>,
    token_cap: Option<i32>,
    max_combos: Option<i32>,
    label: Option<String>,
    buy_amount_sol: Option<f64>,
    partition: sqlx::types::Json<hunter_engine::grouping::GroupPlan>,
    volume_ix_patterns: Option<sqlx::types::Json<Value>>,
    fill_model: Option<String>,
    cost_model: Option<String>,
    scale_out: Option<sqlx::types::Json<Value>>,
    scale_out_top_k: Option<i32>,
}

impl From<RunDbRow> for GroupedSweepRun {
    fn from(r: RunDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
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
            corpus_last_trade_at: r.corpus_last_trade_at,
            created_at: r.created_at,
            status: r.status,
            groups_done: r.groups_done,
            ix_labels_filter: r.ix_labels_filter.map(|j| j.0),
            field_filters: r.field_filters.map(|j| j.0),
            fingerprint_id: r.fingerprint_id,
            token_cap: r.token_cap,
            max_combos: r.max_combos,
            label: r.label,
            buy_amount_sol: r.buy_amount_sol.map(tidy_sol_decimal),
            partition: r.partition.0,
            volume_ix_patterns: r.volume_ix_patterns.map(|j| j.0),
            fill_model: r.fill_model,
            cost_model: r.cost_model,
            scale_out: r.scale_out.map(|j| j.0),
            scale_out_top_k: r.scale_out_top_k,
        }
    }
}

// The `best_*_pnl`/`best_win_rate`/`best_*_holding_secs` fields are LEFT-JOINed in
// from the winning combo's `_results` row (`0007` narrows those to `real`/f32);
// `Option` because the join is LEFT (a missing `_results` row must never hide a
// group — though retention always keeps `best_combo`, so in practice they're
// present). Widened back to the model's f64 in the `From` impl below.
#[derive(sqlx::FromRow)]
struct GroupDbRow {
    id: Uuid,
    group_index: i32,
    group_key: sqlx::types::Json<Value>,
    token_count: i32,
    fired_count: i64,
    best_combo_id: i32,
    best_score: Option<f64>,
    best_expectancy_sol: f64,
    best_win_rate: Option<f32>,
    best_total_pnl_sol: Option<f32>,
    best_open_pnl_sol: Option<f32>,
    best_n_open: Option<i32>,
    best_n_closed: Option<i32>,
    best_profit_factor: Option<f32>,
    best_mean_pnl_pct: Option<f32>,
    best_median_pnl_pct: Option<f32>,
    best_p90_pnl_pct: Option<f32>,
    best_std_pnl_pct: Option<f32>,
    best_avg_holding_secs: Option<f32>,
    best_median_holding_secs: Option<f32>,
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
            best_score: r.best_score,
            best_expectancy_sol: r.best_expectancy_sol,
            best_win_rate: r.best_win_rate.unwrap_or(0.0) as f64,
            best_total_pnl_sol: r.best_total_pnl_sol.unwrap_or(0.0) as f64,
            best_open_pnl_sol: r.best_open_pnl_sol.unwrap_or(0.0) as f64,
            best_n_open: r.best_n_open.unwrap_or(0) as i64,
            best_n_closed: r.best_n_closed.unwrap_or(0) as i64,
            best_profit_factor: r.best_profit_factor.map(|v| v as f64),
            best_mean_pnl_pct: r.best_mean_pnl_pct.unwrap_or(0.0) as f64,
            best_median_pnl_pct: r.best_median_pnl_pct.unwrap_or(0.0) as f64,
            best_p90_pnl_pct: r.best_p90_pnl_pct.unwrap_or(0.0) as f64,
            best_std_pnl_pct: r.best_std_pnl_pct.unwrap_or(0.0) as f64,
            best_avg_holding_secs: r.best_avg_holding_secs.unwrap_or(0.0) as f64,
            best_median_holding_secs: r.best_median_holding_secs.unwrap_or(0.0) as f64,
            best_params: r.best_params.0,
            // Derived, not stored — filled in by the groups-list handler from
            // `sweep::selection::GroupSelection` (see the field's docs).
            selection: None,
        }
    }
}

// Storage types are narrowed (migration `0007`): the PnL/score floats are `real`
// (f32) and the fired/open/closed counts are `integer` (i32). `params` is JOINed
// in from the per-run `_combos` dictionary, not stored on this row. The `From`
// impl below widens back to the model's f64/i64 so the API/JSON shape is unchanged.
#[derive(sqlx::FromRow)]
struct ResultDbRow {
    combo_id: i32,
    params: sqlx::types::Json<Value>,
    n_fired: i32,
    n_open: i32,
    n_closed: i32,
    win_rate: f32,
    total_pnl_sol: f32,
    open_pnl_sol: f32,
    mean_pnl_pct: f32,
    median_pnl_pct: f32,
    p90_pnl_pct: f32,
    best_pnl_pct: f32,
    worst_pnl_pct: f32,
    std_pnl_pct: f32,
    profit_factor: Option<f32>,
    score: Option<f32>,
    expectancy_sol: f32,
    avg_holding_secs: f32,
    median_holding_secs: f32,
    n_exit_take_profit: i32,
    n_exit_stop_loss: i32,
    n_exit_trailing: i32,
    n_exit_stall: i32,
    n_exit_time: i32,
    n_exit_liquidity: i32,
    n_exit_dead: i32,
    n_exit_metrics: i32,
    n_exit_metrics_by_slot: Vec<i32>,
    n_exit_open: i32,
}

impl From<ResultDbRow> for GroupedSweepResult {
    fn from(r: ResultDbRow) -> Self {
        Self {
            combo_id: r.combo_id,
            params: r.params.0,
            n_fired: r.n_fired as i64,
            n_open: r.n_open as i64,
            n_closed: r.n_closed as i64,
            win_rate: r.win_rate as f64,
            total_pnl_sol: r.total_pnl_sol as f64,
            open_pnl_sol: r.open_pnl_sol as f64,
            mean_pnl_pct: r.mean_pnl_pct as f64,
            median_pnl_pct: r.median_pnl_pct as f64,
            p90_pnl_pct: r.p90_pnl_pct as f64,
            best_pnl_pct: r.best_pnl_pct as f64,
            worst_pnl_pct: r.worst_pnl_pct as f64,
            std_pnl_pct: r.std_pnl_pct as f64,
            profit_factor: r.profit_factor.map(|v| v as f64),
            score: r.score.map(|v| v as f64),
            expectancy_sol: r.expectancy_sol as f64,
            avg_holding_secs: r.avg_holding_secs as f64,
            median_holding_secs: r.median_holding_secs as f64,
            n_exit_take_profit: r.n_exit_take_profit,
            n_exit_stop_loss: r.n_exit_stop_loss,
            n_exit_trailing: r.n_exit_trailing,
            n_exit_stall: r.n_exit_stall,
            n_exit_time: r.n_exit_time,
            n_exit_liquidity: r.n_exit_liquidity,
            n_exit_dead: r.n_exit_dead,
            n_exit_metrics: r.n_exit_metrics,
            n_exit_metrics_by_slot: r.n_exit_metrics_by_slot,
            n_exit_open: r.n_exit_open,
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

    /// write the run header up front with `status = 'running'` (before
    /// any group is swept) so a cancel/crash mid-sweep leaves a recoverable row.
    /// `group_count` / `combo_count` are placeholders here (0); they're filled by
    /// [`update_run_counts`] once the engine knows them and re-affirmed by
    /// [`finalize_run`]. `groups_done` starts at 0 and is bumped per
    /// [`append_group`].
    pub async fn insert_run(&self, run: &GroupedSweepRun) -> anyhow::Result<()> {
        let run_sql = format!(
            "INSERT INTO {} \
             (id, strategy_id, source, method, created_after, created_before, \
              curve_only, grouping_spec, axes_spec, min_tokens, token_count, group_count, \
              combo_count, corpus_hash, corpus_last_trade_at, created_at, status, groups_done, \
              ix_labels_filter, field_filters, fingerprint_id, token_cap, max_combos, label, \
              buy_amount_sol, partition, volume_ix_patterns, fill_model, cost_model, \
              scale_out, scale_out_top_k) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,\
                     $18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31)",
            self.tables.runs
        );
        sqlx::query(&run_sql)
            .bind(run.id)
            .bind(&run.strategy_id)
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
            .bind(run.corpus_last_trade_at)
            .bind(run.created_at)
            .bind(&run.status)
            .bind(run.groups_done)
            .bind(run.ix_labels_filter.as_ref().map(sqlx::types::Json))
            .bind(run.field_filters.as_ref().map(sqlx::types::Json))
            .bind(run.fingerprint_id)
            .bind(run.token_cap)
            .bind(run.max_combos)
            .bind(&run.label)
            .bind(run.buy_amount_sol.map(tidy_sol_decimal))
            .bind(sqlx::types::Json(&run.partition))
            .bind(run.volume_ix_patterns.as_ref().map(sqlx::types::Json))
            .bind(&run.fill_model)
            .bind(&run.cost_model)
            .bind(run.scale_out.as_ref().map(sqlx::types::Json))
            .bind(run.scale_out_top_k)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Set the surviving-group + per-group-combo counts once the engine has
    /// partitioned the corpus and fixed the (possibly refine-expanded) combo set.
    /// Lets the run picker render "`groups_done` / `group_count`" while the sweep
    /// is still in flight.
    pub async fn update_run_counts(
        &self,
        run_id: Uuid,
        group_count: i32,
        combo_count: i32,
    ) -> anyhow::Result<()> {
        let sql = format!(
            "UPDATE {} SET group_count = $2, combo_count = $3 WHERE id = $1",
            self.tables.runs
        );
        sqlx::query(&sql)
            .bind(run_id)
            .bind(group_count)
            .bind(combo_count)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Write the per-run combo→`params` dictionary once, up front (migration `0007`
    /// dedup). `combo_params[i]` is the param JSON for `combo_id = i`. Prefer
    /// [`insert_combos_indexed`] for retention-sized sparse inserts.
    pub async fn insert_combos(
        &self,
        run_id: Uuid,
        combo_params: &[Value],
    ) -> anyhow::Result<()> {
        if combo_params.is_empty() {
            return Ok(());
        }
        let rows: Vec<(i32, Value)> = combo_params
            .iter()
            .enumerate()
            .map(|(i, p)| (i as i32, p.clone()))
            .collect();
        self.insert_combos_indexed(run_id, &rows).await
    }

    /// Insert `(combo_id, params)` rows (retention survivors). Idempotent via
    /// `ON CONFLICT DO NOTHING` so overlapping groups sharing a combo_id are fine.
    pub async fn insert_combos_indexed(
        &self,
        run_id: Uuid,
        rows: &[(i32, Value)],
    ) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let t = self.tables;
        for chunk in rows.chunks(20_000) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
                "INSERT INTO {} (run_id, combo_id, params) ",
                t.combos
            ));
            qb.push_values(chunk.iter(), |mut bind, (id, p)| {
                bind.push_bind(run_id)
                    .push_bind(*id)
                    .push_bind(sqlx::types::Json(p));
            });
            qb.push(" ON CONFLICT (run_id, combo_id) DO NOTHING");
            qb.build().execute(&self.pool).await?;
        }
        Ok(())
    }

    /// Persist one **completed** group (its summary + ranked combo rows) and bump
    /// the run's `groups_done`, all in one transaction — so whatever committed
    /// survives a crash. Called per group as the engine finalizes it, serialized
    /// through a single writer task so concurrent small-group folds don't race the
    /// connection. A half-folded group is never passed here (only fully-folded
    /// groups are honest — see the engine's cancel checks).
    pub async fn append_group(
        &self,
        run_id: Uuid,
        g: &GroupedSweepGroupWrite,
    ) -> anyhow::Result<()> {
        let t = self.tables;
        let mut tx = self.pool.begin().await?;

        let group_id = Uuid::new_v4();
        let group_sql = format!(
            "INSERT INTO {} \
             (id, run_id, group_index, group_key, token_count, fired_count, \
              best_combo_id, best_score, best_expectancy_sol, best_params, mints) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            t.groups
        );
        sqlx::query(&group_sql)
            .bind(group_id)
            .bind(run_id)
            .bind(g.group_index)
            .bind(sqlx::types::Json(&g.group_key))
            .bind(g.token_count)
            .bind(g.fired_count)
            .bind(g.best_combo_id)
            .bind(g.best_score)
            .bind(g.best_expectancy_sol)
            .bind(sqlx::types::Json(&g.best_params))
            .bind(&g.mints[..])
            .execute(&mut tx)
            .await?;

        // No `params` here (deduped into `_combos`, written once per run by
        // `insert_combos`); the narrowed columns are bound as i32/f32 to match the
        // `0007` storage types. 30 binds/row (`n_exit_metrics_by_slot` is ONE
        // `INTEGER[]` bind, not one column per slot — 8 more scalar columns would
        // have pushed this over budget) → 30 × 2000 = 60k, still under the 65535
        // bind ceiling — but the margin is thinner now; adding more columns here
        // means lowering the chunk size.
        for chunk in g.results.chunks(2000) {
            let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(format!(
                "INSERT INTO {} \
                 (run_id, group_id, combo_id, n_fired, n_open, n_closed, win_rate, \
                  total_pnl_sol, open_pnl_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
                  worst_pnl_pct, std_pnl_pct, profit_factor, score, expectancy_sol, \
                  avg_holding_secs, median_holding_secs, n_exit_take_profit, n_exit_stop_loss, \
                  n_exit_trailing, n_exit_stall, n_exit_time, n_exit_liquidity, \
                  n_exit_dead, n_exit_metrics, n_exit_metrics_by_slot, n_exit_open) ",
                t.results
            ));
            qb.push_values(chunk, |mut b, r| {
                b.push_bind(run_id)
                    .push_bind(group_id)
                    .push_bind(r.combo_id)
                    .push_bind(r.n_fired as i32)
                    .push_bind(r.n_open as i32)
                    .push_bind(r.n_closed as i32)
                    .push_bind(r.win_rate as f32)
                    .push_bind(r.total_pnl_sol as f32)
                    .push_bind(r.open_pnl_sol as f32)
                    .push_bind(r.mean_pnl_pct as f32)
                    .push_bind(r.median_pnl_pct as f32)
                    .push_bind(r.p90_pnl_pct as f32)
                    .push_bind(r.best_pnl_pct as f32)
                    .push_bind(r.worst_pnl_pct as f32)
                    .push_bind(r.std_pnl_pct as f32)
                    .push_bind(r.profit_factor.map(|v| v as f32))
                    .push_bind(r.score.map(|v| v as f32))
                    .push_bind(r.expectancy_sol as f32)
                    .push_bind(r.avg_holding_secs as f32)
                    .push_bind(r.median_holding_secs as f32)
                    .push_bind(r.n_exit_take_profit)
                    .push_bind(r.n_exit_stop_loss)
                    .push_bind(r.n_exit_trailing)
                    .push_bind(r.n_exit_stall)
                    .push_bind(r.n_exit_time)
                    .push_bind(r.n_exit_liquidity)
                    .push_bind(r.n_exit_dead)
                    .push_bind(r.n_exit_metrics)
                    .push_bind(r.n_exit_metrics_by_slot.clone())
                    .push_bind(r.n_exit_open);
            });
            qb.build().execute(&mut tx).await?;
        }

        let bump_sql = format!(
            "UPDATE {} SET groups_done = groups_done + 1 WHERE id = $1",
            t.runs
        );
        sqlx::query(&bump_sql).bind(run_id).execute(&mut tx).await?;

        tx.commit().await?;
        Ok(())
    }

    /// Stamp a run's terminal `status` (`'completed'`, or `'partial'` when the
    /// engine finished but not every group actually committed — a DB write error
    /// mid-run) plus its authoritative final counts + the resolved
    /// (post-defaults/dedup) axes, which are only known once the sweep returns.
    /// `groups_done` is left as the tally [`append_group`] maintained, so a
    /// `partial` run honestly reads as "`groups_done` / `group_count`".
    pub async fn finalize_run(
        &self,
        run_id: Uuid,
        status: &str,
        group_count: i32,
        combo_count: i32,
        axes_spec: &Value,
    ) -> anyhow::Result<()> {
        let sql = format!(
            "UPDATE {} SET status = $2, group_count = $3, combo_count = $4, \
             axes_spec = $5 WHERE id = $1",
            self.tables.runs
        );
        sqlx::query(&sql)
            .bind(run_id)
            .bind(status)
            .bind(group_count)
            .bind(combo_count)
            .bind(sqlx::types::Json(axes_spec))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Mark a run `cancelled` (user cancel mid-sweep), leaving its already-persisted
    /// groups and the running counts [`update_run_counts`]/[`append_group`] set —
    /// so the partial run reads as "`groups_done` / `group_count`", honestly partial.
    pub async fn mark_cancelled(&self, run_id: Uuid) -> anyhow::Result<()> {
        self.mark_status(run_id, "cancelled").await
    }

    /// Stamp a terminal `status` on a run, leaving its already-persisted groups and
    /// running counts intact. The general form behind [`mark_cancelled`]; the engine
    /// error path uses it to record `partial` (some groups folded before the failure)
    /// or `failed` (none did) rather than deleting the run.
    ///
    /// A grouped sweep is a multi-minute job whose groups are streamed to the DB as
    /// they fold, so a failure at group 380/400 must leave 380 queryable groups — a
    /// `DELETE` throws away real, correct work the user already waited for.
    pub async fn mark_status(&self, run_id: Uuid, status: &str) -> anyhow::Result<()> {
        let sql = format!("UPDATE {} SET status = $2 WHERE id = $1", self.tables.runs);
        sqlx::query(&sql)
            .bind(run_id)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Crash recovery: mark every run still stuck at `status = 'running'` as
    /// `cancelled`. The single-flight gate allows one sweep at a time, and no
    /// sweep can be live at process start, so any `running` row at boot is an
    /// orphan from a killed process — its already-persisted groups stay, but it
    /// stops masquerading as live. Returns the number of rows reconciled.
    pub async fn reconcile_orphaned_runs(&self) -> anyhow::Result<u64> {
        let sql = format!(
            "UPDATE {} SET status = 'cancelled' WHERE status = 'running'",
            self.tables.runs
        );
        let res = sqlx::query(&sql).execute(&self.pool).await?;
        Ok(res.rows_affected())
    }

    /// Look up a single run by id. Returns `None` when the id is unknown.
    pub async fn get_run(&self, run_id: Uuid) -> anyhow::Result<Option<GroupedSweepRun>> {
        let sql = format!(
            "SELECT id, strategy_id, source, method, created_after, created_before, \
                    curve_only, grouping_spec, axes_spec, min_tokens, token_count, group_count, \
                    combo_count, corpus_hash, created_at, status, groups_done, \
                    ix_labels_filter, field_filters, fingerprint_id, token_cap, max_combos, label, \
                    buy_amount_sol, partition, volume_ix_patterns, \
                    fill_model, cost_model, scale_out, scale_out_top_k, corpus_last_trade_at \
             FROM {} WHERE id = $1",
            self.tables.runs
        );
        let row = sqlx::query_as::<_, RunDbRow>(&sql)
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(GroupedSweepRun::from))
    }

    /// Look up a single group by id. Returns `None` when the id is unknown.
    pub async fn get_group(&self, group_id: Uuid) -> anyhow::Result<Option<GroupedSweepGroupSummary>> {
        let sql = format!(
            "SELECT g.id, g.group_index, g.group_key, g.token_count, g.fired_count, \
                    g.best_combo_id, g.best_score, g.best_expectancy_sol, \
                    r.win_rate AS best_win_rate, r.total_pnl_sol AS best_total_pnl_sol, \
                    r.open_pnl_sol AS best_open_pnl_sol, \
                    r.n_open AS best_n_open, r.n_closed AS best_n_closed, \
                    r.profit_factor AS best_profit_factor, r.mean_pnl_pct AS best_mean_pnl_pct, \
                    r.median_pnl_pct AS best_median_pnl_pct, r.p90_pnl_pct AS best_p90_pnl_pct, \
                    r.std_pnl_pct AS best_std_pnl_pct, r.avg_holding_secs AS best_avg_holding_secs, \
                    r.median_holding_secs AS best_median_holding_secs, \
                    g.best_params \
             FROM {groups} g \
             LEFT JOIN {results} r ON r.group_id = g.id AND r.combo_id = g.best_combo_id \
             WHERE g.id = $1",
            groups = self.tables.groups,
            results = self.tables.results,
        );
        let row = sqlx::query_as::<_, GroupDbRow>(&sql)
            .bind(group_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(GroupedSweepGroupSummary::from))
    }

    /// Option C: fetch the stored mint list for a group. Returns `None` when the
    /// group is unknown or was created before migration 0006 (no mints column).
    pub async fn get_group_mints(&self, group_id: Uuid) -> anyhow::Result<Option<Vec<String>>> {
        let sql = format!("SELECT mints FROM {} WHERE id = $1", self.tables.groups);
        let row: Option<(Option<Vec<String>>,)> = sqlx::query_as(&sql)
            .bind(group_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|(mints,)| mints))
    }

    /// Fetch the `params` JSON for one combo in a run, from the per-run `_combos`
    /// dictionary (migration `0007` — `params` is run-level, not per-group).
    /// Returns `None` when the `(run_id, combo_id)` pair is unknown.
    pub async fn get_combo_params(
        &self,
        run_id: Uuid,
        combo_id: i32,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let sql = format!(
            "SELECT params FROM {} WHERE run_id = $1 AND combo_id = $2",
            self.tables.combos
        );
        let row: Option<(sqlx::types::Json<serde_json::Value>,)> =
            sqlx::query_as(&sql)
                .bind(run_id)
                .bind(combo_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(j,)| j.0))
    }

    /// Runs newest first, bounded by `limit`. (The table is already per-strategy.)
    pub async fn list_runs(&self, limit: i64) -> anyhow::Result<Vec<GroupedSweepRun>> {
        let sql = format!(
            "SELECT id, strategy_id, source, method, created_after, created_before, \
                    curve_only, grouping_spec, axes_spec, min_tokens, token_count, group_count, \
                    combo_count, corpus_hash, created_at, status, groups_done, \
                    ix_labels_filter, field_filters, fingerprint_id, token_cap, max_combos, label, \
                    buy_amount_sol, partition, volume_ix_patterns, \
                    fill_model, cost_model, scale_out, scale_out_top_k, corpus_last_trade_at \
             FROM {} ORDER BY created_at DESC LIMIT $1",
            self.tables.runs
        );
        let rows = sqlx::query_as::<_, RunDbRow>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepRun::from).collect())
    }

    /// Rename a run (or clear its name with `None`). Returns the number of rows
    /// updated (0 = unknown id, so the handler can 404).
    pub async fn update_label(
        &self,
        run_id: Uuid,
        label: Option<&str>,
    ) -> anyhow::Result<u64> {
        let sql = format!("UPDATE {} SET label = $2 WHERE id = $1", self.tables.runs);
        let res = sqlx::query(&sql)
            .bind(run_id)
            .bind(label)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Delete one run (groups + results cascade via FK `ON DELETE CASCADE`).
    /// Returns the number of run rows removed (0 = unknown id).
    pub async fn delete_run(&self, run_id: Uuid) -> anyhow::Result<u64> {
        let sql = format!("DELETE FROM {} WHERE id = $1", self.tables.runs);
        let res = sqlx::query(&sql).bind(run_id).execute(&self.pool).await?;
        Ok(res.rows_affected())
    }

    /// Prune runs created strictly before `cutoff` (groups + results cascade).
    /// Returns the number of run rows removed.
    pub async fn delete_runs_before(&self, cutoff: DateTime<Utc>) -> anyhow::Result<u64> {
        let sql = format!("DELETE FROM {} WHERE created_at < $1", self.tables.runs);
        let res = sqlx::query(&sql).bind(cutoff).execute(&self.pool).await?;
        Ok(res.rows_affected())
    }

    /// Group summaries for a run, best opportunity first (highest robust score;
    /// groups with no scoreable winner sort last, then by group index).
    pub async fn list_groups(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Vec<GroupedSweepGroupSummary>> {
        let sql = format!(
            "SELECT g.id, g.group_index, g.group_key, g.token_count, g.fired_count, \
                    g.best_combo_id, g.best_score, g.best_expectancy_sol, \
                    r.win_rate AS best_win_rate, r.total_pnl_sol AS best_total_pnl_sol, \
                    r.open_pnl_sol AS best_open_pnl_sol, \
                    r.n_open AS best_n_open, r.n_closed AS best_n_closed, \
                    r.profit_factor AS best_profit_factor, r.mean_pnl_pct AS best_mean_pnl_pct, \
                    r.median_pnl_pct AS best_median_pnl_pct, r.p90_pnl_pct AS best_p90_pnl_pct, \
                    r.std_pnl_pct AS best_std_pnl_pct, r.avg_holding_secs AS best_avg_holding_secs, \
                    r.median_holding_secs AS best_median_holding_secs, \
                    g.best_params \
             FROM {groups} g \
             LEFT JOIN {results} r ON r.group_id = g.id AND r.combo_id = g.best_combo_id \
             WHERE g.run_id = $1 \
             ORDER BY g.best_score DESC NULLS LAST, g.group_index ASC",
            groups = self.tables.groups,
            results = self.tables.results,
        );
        let rows = sqlx::query_as::<_, GroupDbRow>(&sql)
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepGroupSummary::from).collect())
    }

    /// The run's `buy_amount_sol` (the sim notional), or `None` when unset. Cheap PK
    /// lookup used by the results endpoint only when a sort/filter references the
    /// derived `mtm_pnl_pct` column (which is computed from this notional).
    pub async fn run_buy_amount_sol(&self, run_id: Uuid) -> anyhow::Result<Option<f64>> {
        let sql = format!(
            "SELECT buy_amount_sol FROM {} WHERE id = $1",
            self.tables.runs
        );
        let v: Option<Option<f64>> = sqlx::query_scalar(&sql)
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(v.flatten())
    }

    /// Combo count for a group **after applying the same per-column filters** as the
    /// page query — so `X-Total-Count` matches the filtered row set the pager walks.
    ///
    /// `filter_sql` is the pre-validated ` AND (…) …` fragment from the handler
    /// (every column/op allowlisted there; operands parameterized). Its placeholders
    /// start at `$3`, so `filter_binds` bind in order right after `run_id`/`group_id`.
    /// The `_combos` JOIN is present so a filter on a `p_*` param column resolves; it
    /// is a 1:1 inner join on `(run_id, combo_id)`, so it never changes the count.
    pub async fn count_results(
        &self,
        run_id: Uuid,
        group_id: Uuid,
        filter_sql: &str,
        filter_binds: &[f64],
    ) -> anyhow::Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM {} r JOIN {} c ON c.run_id = r.run_id AND c.combo_id = r.combo_id \
             WHERE r.run_id = $1 AND r.group_id = $2{}",
            self.tables.results, self.tables.combos, filter_sql
        );
        let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(run_id).bind(group_id);
        for v in filter_binds {
            q = q.bind(*v);
        }
        Ok(q.fetch_one(&self.pool).await?)
    }

    /// One page of ranked combo rows for a group.
    ///
    /// `order_by` is the full, pre-validated `ORDER BY` body (one or more
    /// comma-joined sort levels plus a stable tiebreak) built by the handler from
    /// the frontend `TableQuery` — every column name/expression is allowlisted
    /// there, so it is safe to interpolate. Empty falls back to `score DESC`.
    ///
    /// `filter_sql`/`filter_binds` are the same pre-validated per-column filter the
    /// count applies (placeholders from `$3`); `limit`/`offset` bind *after* the
    /// filter operands, so their placeholder numbers shift by `filter_binds.len()`.
    pub async fn list_results_paged(
        &self,
        run_id: Uuid,
        group_id: Uuid,
        limit: i64,
        offset: i64,
        order_by: &str,
        filter_sql: &str,
        filter_binds: &[f64],
    ) -> anyhow::Result<Vec<GroupedSweepResult>> {
        // `params` lives on the per-run `_combos` dictionary (migration `0007`),
        // JOINed back on `(run_id, combo_id)`. Order columns are table-qualified:
        // metric columns are on the results row (`r`), `params` on the combos row (`c`).
        let order = if order_by.is_empty() {
            "r.score DESC NULLS LAST, r.combo_id ASC".to_string()
        } else {
            order_by.to_string()
        };
        // Filter operands are $3..; LIMIT/OFFSET come after them.
        let lim_idx = 3 + filter_binds.len();
        let off_idx = lim_idx + 1;
        let sql = format!(
            "SELECT r.combo_id, c.params, r.n_fired, r.n_open, r.n_closed, r.win_rate, \
                    r.total_pnl_sol, r.open_pnl_sol, r.mean_pnl_pct, r.median_pnl_pct, r.p90_pnl_pct, \
                    r.best_pnl_pct, r.worst_pnl_pct, r.std_pnl_pct, r.profit_factor, r.score, \
                    r.expectancy_sol, r.avg_holding_secs, r.median_holding_secs, \
                    r.n_exit_take_profit, r.n_exit_stop_loss, r.n_exit_trailing, r.n_exit_stall, \
                    r.n_exit_time, r.n_exit_liquidity, \
                    r.n_exit_dead, r.n_exit_metrics, r.n_exit_metrics_by_slot, r.n_exit_open \
             FROM {} r JOIN {} c ON c.run_id = r.run_id AND c.combo_id = r.combo_id \
             WHERE r.run_id = $1 AND r.group_id = $2{} \
             ORDER BY {} LIMIT ${} OFFSET ${}",
            self.tables.results, self.tables.combos, filter_sql, order, lim_idx, off_idx
        );
        let mut q = sqlx::query_as::<_, ResultDbRow>(&sql).bind(run_id).bind(group_id);
        for v in filter_binds {
            q = q.bind(*v);
        }
        let rows = q
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(GroupedSweepResult::from).collect())
    }
}
