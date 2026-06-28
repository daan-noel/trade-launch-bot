use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::strategy::{
    StrategyPosition, StrategyRule, StrategyRun, StrategyRunMetrics,
};

/// Repo spanning the unified strategy schema: `strategy_rules`,
/// `strategy_runs`, `strategy_run_metrics`, `strategy_positions`.
#[derive(Clone)]
pub struct StrategyRepo {
    pool: PgPool,
}

// ---------------------------------------------------------------------------
// DB rows — keep sqlx derives out of domain models
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct StrategyRuleDbRow {
    id: Uuid,
    strategy_id: String,
    rule_name: String,
    buy_amount: f64,
    trade_mode: String,
    is_active: bool,
    max_concurrent_tokens: Option<i64>,
    max_total_tokens: Option<i64>,
    params: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyRuleDbRow> for StrategyRule {
    fn from(r: StrategyRuleDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
            rule_name: r.rule_name,
            buy_amount: r.buy_amount,
            trade_mode: r.trade_mode,
            is_active: r.is_active,
            max_concurrent_tokens: r.max_concurrent_tokens,
            max_total_tokens: r.max_total_tokens,
            params: r.params.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyRunDbRow {
    id: Uuid,
    strategy_id: String,
    rule_id: Option<Uuid>,
    mode: String,
    run_seq: i64,
    status: String,
    params_snapshot: Json<Value>,
    max_total_tokens: Option<i64>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<StrategyRunDbRow> for StrategyRun {
    fn from(r: StrategyRunDbRow) -> Self {
        Self {
            id: r.id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            mode: r.mode,
            run_seq: r.run_seq,
            status: r.status,
            params_snapshot: r.params_snapshot.0,
            max_total_tokens: r.max_total_tokens,
            started_at: r.started_at,
            finished_at: r.finished_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyRunMetricsDbRow {
    run_id: Uuid,
    rolled_up_at: DateTime<Utc>,
    n_fired: i32,
    n_open: i32,
    n_closed: i32,
    win_rate: f32,
    total_pnl_sol: f32,
    expectancy_sol: f32,
    mean_pnl_pct: f32,
    median_pnl_pct: f32,
    p90_pnl_pct: f32,
    best_pnl_pct: f32,
    worst_pnl_pct: f32,
    std_pnl_pct: f32,
    profit_factor: Option<f32>,
    avg_holding_secs: f32,
    median_holding_secs: f32,
    n_exit_take_profit: i32,
    n_exit_stop_loss: i32,
    n_exit_trailing: i32,
    n_exit_stall: i32,
    n_exit_time: i32,
    n_exit_liquidity: i32,
    n_exit_cohort: i32,
    n_exit_open: i32,
}

impl From<StrategyRunMetricsDbRow> for StrategyRunMetrics {
    fn from(r: StrategyRunMetricsDbRow) -> Self {
        Self {
            run_id: r.run_id,
            rolled_up_at: r.rolled_up_at,
            n_fired: r.n_fired,
            n_open: r.n_open,
            n_closed: r.n_closed,
            win_rate: r.win_rate,
            total_pnl_sol: r.total_pnl_sol,
            expectancy_sol: r.expectancy_sol,
            mean_pnl_pct: r.mean_pnl_pct,
            median_pnl_pct: r.median_pnl_pct,
            p90_pnl_pct: r.p90_pnl_pct,
            best_pnl_pct: r.best_pnl_pct,
            worst_pnl_pct: r.worst_pnl_pct,
            std_pnl_pct: r.std_pnl_pct,
            profit_factor: r.profit_factor,
            avg_holding_secs: r.avg_holding_secs,
            median_holding_secs: r.median_holding_secs,
            n_exit_take_profit: r.n_exit_take_profit,
            n_exit_stop_loss: r.n_exit_stop_loss,
            n_exit_trailing: r.n_exit_trailing,
            n_exit_stall: r.n_exit_stall,
            n_exit_time: r.n_exit_time,
            n_exit_liquidity: r.n_exit_liquidity,
            n_exit_cohort: r.n_exit_cohort,
            n_exit_open: r.n_exit_open,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StrategyPositionDbRow {
    id: Uuid,
    run_id: Uuid,
    strategy_id: String,
    rule_id: Option<Uuid>,
    mode: String,
    mint: String,
    wallet: String,
    token_program_id: Option<String>,
    target_price: Option<f64>,
    target_token_amount: Option<f64>,
    target_time: Option<DateTime<Utc>>,
    target_tx: Option<String>,
    entry_price: Option<f64>,
    entry_token_amount: Option<f64>,
    entry_sol: Option<f64>,
    entry_time: Option<DateTime<Utc>>,
    entry_tx_signatures: Json<Value>,
    exit_price: Option<f64>,
    exit_token_amount: Option<f64>,
    exit_sol: Option<f64>,
    exit_time: Option<DateTime<Utc>>,
    exit_tx_signatures: Json<Value>,
    submitted_buy_signatures: Vec<String>,
    status: String,
    exit_reason: Option<String>,
    extra: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<StrategyPositionDbRow> for StrategyPosition {
    fn from(r: StrategyPositionDbRow) -> Self {
        Self {
            id: r.id,
            run_id: r.run_id,
            strategy_id: r.strategy_id,
            rule_id: r.rule_id,
            mode: r.mode,
            mint: r.mint,
            wallet: r.wallet,
            token_program_id: r.token_program_id,
            target_price: r.target_price,
            target_token_amount: r.target_token_amount,
            target_time: r.target_time,
            target_tx: r.target_tx,
            entry_price: r.entry_price,
            entry_token_amount: r.entry_token_amount,
            entry_sol: r.entry_sol,
            entry_time: r.entry_time,
            entry_tx_signatures: r.entry_tx_signatures.0,
            exit_price: r.exit_price,
            exit_token_amount: r.exit_token_amount,
            exit_sol: r.exit_sol,
            exit_time: r.exit_time,
            exit_tx_signatures: r.exit_tx_signatures.0,
            submitted_buy_signatures: r.submitted_buy_signatures,
            status: r.status,
            exit_reason: r.exit_reason,
            extra: r.extra.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Explicit column lists (struct order). Not `SELECT *` so a new physical
// column isn't pulled into every read and the wire contract stays decoupled.
// ---------------------------------------------------------------------------

const RULE_COLS: &str = "id, strategy_id, rule_name, buy_amount, trade_mode, is_active, \
    max_concurrent_tokens, max_total_tokens, params, created_at, updated_at";

const RUN_COLS: &str = "id, strategy_id, rule_id, mode, run_seq, status, params_snapshot, \
    max_total_tokens, started_at, finished_at";

const METRICS_COLS: &str = "run_id, rolled_up_at, n_fired, n_open, n_closed, win_rate, \
    total_pnl_sol, expectancy_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct, \
    worst_pnl_pct, std_pnl_pct, profit_factor, avg_holding_secs, median_holding_secs, \
    n_exit_take_profit, n_exit_stop_loss, n_exit_trailing, n_exit_stall, n_exit_time, \
    n_exit_liquidity, n_exit_cohort, n_exit_open";

const POSITION_COLS: &str = "id, run_id, strategy_id, rule_id, mode, mint, wallet, \
    token_program_id, target_price, target_token_amount, target_time, target_tx, \
    entry_price, entry_token_amount, entry_sol, entry_time, entry_tx_signatures, \
    exit_price, exit_token_amount, exit_sol, exit_time, exit_tx_signatures, \
    submitted_buy_signatures, status, exit_reason, extra, created_at, updated_at";

// ---------------------------------------------------------------------------
// Repo
// ---------------------------------------------------------------------------

impl StrategyRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool — for the few callers that need a free-function query
    /// (e.g. `trade_repo::find_tx_by_fill` on the paper fill-recovery path).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // -- Rules ----------------------------------------------------------------

    pub async fn insert_rule(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_rules
                (id, strategy_id, rule_name, buy_amount, trade_mode, is_active,
                 max_concurrent_tokens, max_total_tokens, params, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(rule.id)
        .bind(&rule.strategy_id)
        .bind(&rule.rule_name)
        .bind(rule.buy_amount)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_rule(&self, rule: &StrategyRule) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_rules SET
                rule_name = $2,
                buy_amount = $3,
                trade_mode = $4,
                is_active = $5,
                max_concurrent_tokens = $6,
                max_total_tokens = $7,
                params = $8,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(rule.id)
        .bind(&rule.rule_name)
        .bind(rule.buy_amount)
        .bind(&rule.trade_mode)
        .bind(rule.is_active)
        .bind(rule.max_concurrent_tokens)
        .bind(rule.max_total_tokens)
        .bind(Json(&rule.params))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_rule(&self, id: Uuid) -> anyhow::Result<Option<StrategyRule>> {
        let row = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRule::from))
    }

    pub async fn find_rules_by_strategy(
        &self,
        strategy_id: &str,
    ) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE strategy_id = $1 ORDER BY created_at DESC"
        ))
        .bind(strategy_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    pub async fn find_active_rules(&self) -> anyhow::Result<Vec<StrategyRule>> {
        let rows = sqlx::query_as::<_, StrategyRuleDbRow>(&format!(
            "SELECT {RULE_COLS} FROM strategy_rules WHERE is_active ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyRule::from).collect())
    }

    pub async fn delete_rule(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- Runs -----------------------------------------------------------------

    pub async fn insert_run(&self, run: &StrategyRun) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_runs
                (id, strategy_id, rule_id, mode, run_seq, status, params_snapshot,
                 max_total_tokens, started_at, finished_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(run.id)
        .bind(&run.strategy_id)
        .bind(run.rule_id)
        .bind(&run.mode)
        .bind(run.run_seq)
        .bind(&run.status)
        .bind(Json(&run.params_snapshot))
        .bind(run.max_total_tokens)
        .bind(run.started_at)
        .bind(run.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Next monotonic `run_seq` for `(rule_id, mode)` — `MAX + 1`, starting at 1.
    pub async fn next_run_seq(&self, rule_id: Uuid, mode: &str) -> anyhow::Result<i64> {
        let seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(run_seq), 0) + 1 FROM strategy_runs WHERE rule_id = $1 AND mode = $2",
        )
        .bind(rule_id)
        .bind(mode)
        .fetch_one(&self.pool)
        .await?;
        Ok(seq)
    }

    pub async fn find_run(&self, id: Uuid) -> anyhow::Result<Option<StrategyRun>> {
        let row = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT {RUN_COLS} FROM strategy_runs WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRun::from))
    }

    pub async fn latest_run(
        &self,
        rule_id: Uuid,
        mode: &str,
    ) -> anyhow::Result<Option<StrategyRun>> {
        let row = sqlx::query_as::<_, StrategyRunDbRow>(&format!(
            "SELECT {RUN_COLS} FROM strategy_runs WHERE rule_id = $1 AND mode = $2 \
             ORDER BY run_seq DESC LIMIT 1"
        ))
        .bind(rule_id)
        .bind(mode)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRun::from))
    }

    pub async fn set_run_status(
        &self,
        id: Uuid,
        status: &str,
        finished_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE strategy_runs SET status = $2, finished_at = $3 WHERE id = $1")
            .bind(id)
            .bind(status)
            .bind(finished_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- Metrics --------------------------------------------------------------

    pub async fn upsert_metrics(&self, m: &StrategyRunMetrics) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_run_metrics
                (run_id, rolled_up_at, n_fired, n_open, n_closed, win_rate, total_pnl_sol,
                 expectancy_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct,
                 worst_pnl_pct, std_pnl_pct, profit_factor, avg_holding_secs, median_holding_secs,
                 n_exit_take_profit, n_exit_stop_loss, n_exit_trailing, n_exit_stall, n_exit_time,
                 n_exit_liquidity, n_exit_cohort, n_exit_open)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, $23, $24, $25)
            ON CONFLICT (run_id) DO UPDATE SET
                rolled_up_at = EXCLUDED.rolled_up_at,
                n_fired = EXCLUDED.n_fired,
                n_open = EXCLUDED.n_open,
                n_closed = EXCLUDED.n_closed,
                win_rate = EXCLUDED.win_rate,
                total_pnl_sol = EXCLUDED.total_pnl_sol,
                expectancy_sol = EXCLUDED.expectancy_sol,
                mean_pnl_pct = EXCLUDED.mean_pnl_pct,
                median_pnl_pct = EXCLUDED.median_pnl_pct,
                p90_pnl_pct = EXCLUDED.p90_pnl_pct,
                best_pnl_pct = EXCLUDED.best_pnl_pct,
                worst_pnl_pct = EXCLUDED.worst_pnl_pct,
                std_pnl_pct = EXCLUDED.std_pnl_pct,
                profit_factor = EXCLUDED.profit_factor,
                avg_holding_secs = EXCLUDED.avg_holding_secs,
                median_holding_secs = EXCLUDED.median_holding_secs,
                n_exit_take_profit = EXCLUDED.n_exit_take_profit,
                n_exit_stop_loss = EXCLUDED.n_exit_stop_loss,
                n_exit_trailing = EXCLUDED.n_exit_trailing,
                n_exit_stall = EXCLUDED.n_exit_stall,
                n_exit_time = EXCLUDED.n_exit_time,
                n_exit_liquidity = EXCLUDED.n_exit_liquidity,
                n_exit_cohort = EXCLUDED.n_exit_cohort,
                n_exit_open = EXCLUDED.n_exit_open
            "#,
        )
        .bind(m.run_id)
        .bind(m.rolled_up_at)
        .bind(m.n_fired)
        .bind(m.n_open)
        .bind(m.n_closed)
        .bind(m.win_rate)
        .bind(m.total_pnl_sol)
        .bind(m.expectancy_sol)
        .bind(m.mean_pnl_pct)
        .bind(m.median_pnl_pct)
        .bind(m.p90_pnl_pct)
        .bind(m.best_pnl_pct)
        .bind(m.worst_pnl_pct)
        .bind(m.std_pnl_pct)
        .bind(m.profit_factor)
        .bind(m.avg_holding_secs)
        .bind(m.median_holding_secs)
        .bind(m.n_exit_take_profit)
        .bind(m.n_exit_stop_loss)
        .bind(m.n_exit_trailing)
        .bind(m.n_exit_stall)
        .bind(m.n_exit_time)
        .bind(m.n_exit_liquidity)
        .bind(m.n_exit_cohort)
        .bind(m.n_exit_open)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_metrics(&self, run_id: Uuid) -> anyhow::Result<Option<StrategyRunMetrics>> {
        let row = sqlx::query_as::<_, StrategyRunMetricsDbRow>(&format!(
            "SELECT {METRICS_COLS} FROM strategy_run_metrics WHERE run_id = $1"
        ))
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyRunMetrics::from))
    }

    // -- Positions ------------------------------------------------------------

    pub async fn insert_position(&self, p: &StrategyPosition) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO strategy_positions
                (id, run_id, strategy_id, rule_id, mode, mint, wallet, token_program_id,
                 target_price, target_token_amount, target_time, target_tx,
                 entry_price, entry_token_amount, entry_sol, entry_time, entry_tx_signatures,
                 exit_price, exit_token_amount, exit_sol, exit_time, exit_tx_signatures,
                 submitted_buy_signatures, status, exit_reason, extra, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                    $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint)
        .bind(&p.wallet)
        .bind(p.token_program_id.as_ref())
        .bind(p.target_price)
        .bind(p.target_token_amount)
        .bind(p.target_time)
        .bind(p.target_tx.as_ref())
        .bind(p.entry_price)
        .bind(p.entry_token_amount)
        .bind(p.entry_sol)
        .bind(p.entry_time)
        .bind(Json(&p.entry_tx_signatures))
        .bind(p.exit_price)
        .bind(p.exit_token_amount)
        .bind(p.exit_sol)
        .bind(p.exit_time)
        .bind(Json(&p.exit_tx_signatures))
        .bind(&p.submitted_buy_signatures)
        .bind(&p.status)
        .bind(p.exit_reason.as_ref())
        .bind(Json(&p.extra))
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_position(&self, p: &StrategyPosition) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE strategy_positions SET
                run_id = $2,
                strategy_id = $3,
                rule_id = $4,
                mode = $5,
                mint = $6,
                wallet = $7,
                token_program_id = $8,
                target_price = $9,
                target_token_amount = $10,
                target_time = $11,
                target_tx = $12,
                entry_price = $13,
                entry_token_amount = $14,
                entry_sol = $15,
                entry_time = $16,
                entry_tx_signatures = $17,
                exit_price = $18,
                exit_token_amount = $19,
                exit_sol = $20,
                exit_time = $21,
                exit_tx_signatures = $22,
                submitted_buy_signatures = $23,
                status = $24,
                exit_reason = $25,
                extra = $26,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(p.id)
        .bind(p.run_id)
        .bind(&p.strategy_id)
        .bind(p.rule_id)
        .bind(&p.mode)
        .bind(&p.mint)
        .bind(&p.wallet)
        .bind(p.token_program_id.as_ref())
        .bind(p.target_price)
        .bind(p.target_token_amount)
        .bind(p.target_time)
        .bind(p.target_tx.as_ref())
        .bind(p.entry_price)
        .bind(p.entry_token_amount)
        .bind(p.entry_sol)
        .bind(p.entry_time)
        .bind(Json(&p.entry_tx_signatures))
        .bind(p.exit_price)
        .bind(p.exit_token_amount)
        .bind(p.exit_sol)
        .bind(p.exit_time)
        .bind(Json(&p.exit_tx_signatures))
        .bind(&p.submitted_buy_signatures)
        .bind(&p.status)
        .bind(p.exit_reason.as_ref())
        .bind(Json(&p.extra))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_position(&self, id: Uuid) -> anyhow::Result<Option<StrategyPosition>> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyPosition::from))
    }

    pub async fn find_positions_by_run(
        &self,
        run_id: Uuid,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE run_id = $1 \
             ORDER BY created_at DESC"
        ))
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    pub async fn find_positions_by_rule(
        &self,
        rule_id: Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE rule_id = $1 \
             ORDER BY created_at DESC LIMIT $2"
        ))
        .bind(rule_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    // -- HTTP read views (page-bounded, newest first) -------------------------
    // Back the live position-read endpoints. Every query is `LIMIT/OFFSET`-bound
    // and the list/by-mint/by-wallet views are scoped by `strategy_id` so a
    // growing `strategy_positions` table is never fetched whole.

    /// Page-bounded positions for one run — the by-rule view resolves a paper
    /// rule's latest run to this (paper retains only the current run's bag).
    pub async fn find_positions_by_run_paged(
        &self,
        run_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE run_id = $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        ))
        .bind(run_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded positions for a rule across all its runs — the by-rule view
    /// for a real rule (full lifetime history, newest first).
    pub async fn find_positions_by_rule_paged(
        &self,
        rule_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE rule_id = $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        ))
        .bind(rule_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded positions for a strategy family — the HTTP list view.
    pub async fn find_positions_by_strategy(
        &self,
        strategy_id: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions WHERE strategy_id = $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        ))
        .bind(strategy_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded in-holding-index positions (`Arming`/`BuySubmitted`/`Holding`)
    /// for one mint within a strategy — the HTTP by-mint view.
    pub async fn find_holding_by_mint(
        &self,
        strategy_id: &str,
        mint: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE strategy_id = $1 AND mint = $2 \
               AND status IN ('Holding', 'Arming', 'BuySubmitted') \
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        ))
        .bind(strategy_id)
        .bind(mint)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Page-bounded in-holding-index positions for one wallet within a strategy —
    /// the HTTP by-wallet view.
    pub async fn find_holding_by_wallet(
        &self,
        strategy_id: &str,
        wallet: &str,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE strategy_id = $1 AND wallet = $2 \
               AND status IN ('Holding', 'Arming', 'BuySubmitted') \
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
        ))
        .bind(strategy_id)
        .bind(wallet)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    pub async fn find_open_positions(&self) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status NOT IN ('End', 'ExitFailed') ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    pub async fn delete_position(&self, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM strategy_positions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Append a submitted snipe-buy signature and flip the row to `BuySubmitted`
    /// (the durable write-ahead "buy in flight" marker), returning the updated row.
    /// Guarded by `entry_price IS NULL` so a concurrent fill that already advanced
    /// the row to `Holding` is never clobbered back — returns `None` in that benign
    /// case. Single round-trip (`RETURNING`) so the caller syncs the cache off it.
    pub async fn mark_buy_submitted(
        &self,
        id: Uuid,
        signature: &str,
    ) -> anyhow::Result<Option<StrategyPosition>> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "UPDATE strategy_positions \
             SET status = 'BuySubmitted', \
                 submitted_buy_signatures = array_append(submitted_buy_signatures, $2), \
                 updated_at = now() \
             WHERE id = $1 AND entry_price IS NULL \
             RETURNING {POSITION_COLS}"
        ))
        .bind(id)
        .bind(signature)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(StrategyPosition::from))
    }

    /// Record the entry fill atomically (entry tx/amount/price/sol/time + flip to
    /// `Holding`) and return the fresh row in one round-trip. The single-leg entry
    /// signature is stored as a JSONB array. Mirrors the old per-strategy
    /// `update_entry`; the `RETURNING` lets the caller sync the cache without a
    /// follow-up read.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_entry_fill(
        &self,
        id: Uuid,
        entry_tx: &str,
        entry_token_amount: f64,
        entry_price: f64,
        entry_sol: f64,
        entry_time: DateTime<Utc>,
    ) -> anyhow::Result<StrategyPosition> {
        let row = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "UPDATE strategy_positions \
             SET entry_tx_signatures = $2, entry_token_amount = $3, entry_price = $4, \
                 entry_sol = $5, entry_time = $6, status = 'Holding', updated_at = now() \
             WHERE id = $1 \
             RETURNING {POSITION_COLS}"
        ))
        .bind(id)
        .bind(Json(json!([entry_tx])))
        .bind(entry_token_amount)
        .bind(entry_price)
        .bind(entry_sol)
        .bind(entry_time)
        .fetch_one(&self.pool)
        .await?;
        Ok(StrategyPosition::from(row))
    }

    // -- Recovery reaper queries (mode-scoped) --------------------------------
    // The live service runs these per mode ('real' / 'paper'); they replace the
    // per-strategy real/paper repos' reaper queries. Small result sets in normal
    // operation (index-served by the status predicates).

    /// Positions stranded in `ExitPending` for a mode — the exit-recovery reaper
    /// re-drives a sell whose task panicked / was lost to a restart (the holding
    /// cache only loads open rows, so these are otherwise invisible).
    pub async fn find_all_exit_pending(&self, mode: &str) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status = 'ExitPending' AND mode = $1 ORDER BY updated_at ASC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Positions stuck in `BuySubmitted` for a mode — the buy-recovery reaper
    /// checks each row's submitted signatures against the feed/chain and
    /// adopts/waits/drops (never blindly deletes — tokens may exist on-chain).
    pub async fn find_all_buy_submitted(&self, mode: &str) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status = 'BuySubmitted' AND mode = $1 ORDER BY updated_at ASC"
        ))
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Open (`Holding`, entry-recorded) positions for one mint in a mode — drives
    /// the manual-sell reconcile (an externally-cleared bag closed without a sell).
    pub async fn find_open_by_mint(
        &self,
        mint: &str,
        mode: &str,
    ) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE mint = $1 AND mode = $2 AND status = 'Holding' AND entry_price IS NOT NULL"
        ))
        .bind(mint)
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }

    /// Terminally fail positions stuck in `ExitPending` past `stale_after` (orphaned
    /// mid-exit). Re-arming a half-done real exit risks a double-sell, so fail it.
    /// Returns rows affected.
    pub async fn fail_stale_exit_pending(
        &self,
        mode: &str,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            "UPDATE strategy_positions SET status = 'ExitFailed', updated_at = now() \
             WHERE status = 'ExitPending' AND mode = $1 AND updated_at < $2",
        )
        .bind(mode)
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Delete positions left `Arming` with no entry fill past `stale_after` — they
    /// matched a rule but never sent a buy (no SOL, no tokens), so they're safe to
    /// drop. Scoped to `Arming` only: a `BuySubmitted` row may own tokens and is the
    /// buy-recovery reaper's responsibility. Returns rows deleted.
    pub async fn delete_stale_unentered(
        &self,
        mode: &str,
        stale_after: std::time::Duration,
    ) -> anyhow::Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::from_std(stale_after)?;
        let res = sqlx::query(
            "DELETE FROM strategy_positions \
             WHERE status = 'Arming' AND entry_price IS NULL AND mode = $1 AND created_at < $2",
        )
        .bind(mode)
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Distinct mints with an entry-recorded `Holding` real position whose net traded
    /// balance (Σbuys − Σsells) has fallen to ≤ `threshold_raw` — the bag was cleared
    /// outside the strategy exit path (a manual sell). Drives the boot/maintenance
    /// manual-sell reaper. `threshold_raw` is in **raw token base units** (the new
    /// `trades.token_amount` is BIGINT raw units, not decimal tokens). Joins
    /// `wallet_dict` to resolve the interned `wallet_id`.
    pub async fn find_externally_cleared_holding_mints(
        &self,
        threshold_raw: f64,
    ) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT p.mint
            FROM strategy_positions p
            JOIN wallet_dict w ON w.address = p.wallet
            WHERE p.mode = 'real' AND p.status = 'Holding' AND p.entry_price IS NOT NULL
              -- Require a sell on record: else a position whose BUY merely aged out of
              -- the rolling buffer (net = 0, no sell) would be falsely "cleared".
              AND EXISTS (
                    SELECT 1 FROM trades s
                    WHERE s.wallet_id = w.id AND s.mint_address = p.mint
                      AND s.trade_type = 'sell'
                  )
              AND COALESCE((
                    SELECT SUM(CASE WHEN t.trade_type = 'buy'  THEN t.token_amount
                                    WHEN t.trade_type = 'sell' THEN -t.token_amount
                                    ELSE 0 END)
                    FROM trades t
                    WHERE t.wallet_id = w.id AND t.mint_address = p.mint
                  ), 0)::double precision <= $1
            "#,
        )
        .bind(threshold_raw)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(m,)| m).collect())
    }
}
