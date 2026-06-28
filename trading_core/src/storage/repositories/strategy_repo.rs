use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use uuid::Uuid;

use crate::models::strategy::{
    StrategyPosition, StrategyRule, StrategyRun, StrategyRunMetrics,
};

/// Repo spanning the unified strategy schema: `strategy_rules`,
/// `strategy_runs`, `strategy_run_metrics`, `strategy_positions`.
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

    pub async fn find_open_positions(&self) -> anyhow::Result<Vec<StrategyPosition>> {
        let rows = sqlx::query_as::<_, StrategyPositionDbRow>(&format!(
            "SELECT {POSITION_COLS} FROM strategy_positions \
             WHERE status NOT IN ('End', 'ExitFailed') ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(StrategyPosition::from).collect())
    }
}
