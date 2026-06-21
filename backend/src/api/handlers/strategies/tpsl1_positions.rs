use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    models::Position,
    state::app_state::AppState,
    storage::repositories::{
        tpsl1_paper_trading_repo::Tpsl1PaperTradingRepo, tpsl1_position_repo::Tpsl1PositionRepo,
        tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo,
    },
};

// ---------------------------------------------------------------------------
// Response Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PositionResponse {
    pub id: Uuid,
    pub mint: String,
    pub wallet: String,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    pub entry_tx: String,
    pub exit_tx: Option<String>,
    pub status: String,
    pub strategy: String,
    pub rule_id: Uuid,
    pub entry_token_amount: Option<f64>,
    pub exit_token_amount: Option<f64>,
    pub pnl_percent: Option<f64>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_time: Option<DateTime<Utc>>,
    /// Why the position exited ("TakeProfit", "StopLoss", "TrailingStop",
    /// "Stall", "TimeStop", "LiquidityExit"); `None` while still open.
    pub exit_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Position> for PositionResponse {
    fn from(p: Position) -> Self {
        let pnl_percent = p.pnl_percentage();
        let exit_reason = p.exit_reason_or_derived();
        Self {
            id: p.id,
            mint: p.mint,
            wallet: p.wallet,
            entry_price: p.entry_price,
            exit_price: p.exit_price,
            entry_tx: p.entry_tx,
            exit_tx: p.exit_tx,
            status: p.status.to_string(),
            strategy: p.strategy,
            rule_id: p.rule_id,
            entry_token_amount: p.entry_token_amount,
            exit_token_amount: p.exit_token_amount,
            pnl_percent,
            entry_time: p.entry_time,
            exit_time: p.exit_time,
            exit_reason,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// Query params for the position list views. Bounds every list query so a
/// growing `tpsl1_real_positions` table can't be fetched whole in one request.
#[derive(serde::Deserialize)]
pub struct PositionListParams {
    #[serde(default = "default_positions_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_positions_limit() -> i64 {
    200
}

impl PositionListParams {
    /// Clamp to a sane window: limit in 1..=1000, offset >= 0.
    fn bounds(&self) -> (i64, i64) {
        (self.limit.clamp(1, 1000), self.offset.max(0))
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Load a rule's positions from the correct table.
///
/// Paper-mode rules record to `tpsl1_paper_positions` (only the latest run is
/// retained), so they're served from the paper repo's current run; real rules
/// use `tpsl1_real_positions`. A paper rule with no run yet yields an empty list.
///
/// Shared by the positions endpoint below and exercised directly in tests so it
/// stays in lock-step with the paper-result endpoint (same run, same rows).
pub(crate) async fn load_rule_positions(
    db: &PgPool,
    rule_id: Uuid,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Position>> {
    let is_paper = match Tpsl1StrategyRuleRepo::new(db.clone()).find_by_id(rule_id).await? {
        Some(rule) => rule.trade_mode == "paper",
        None => false,
    };

    if is_paper {
        let paper_repo = Tpsl1PaperTradingRepo::new(db.clone());
        match paper_repo.current_run(rule_id).await? {
            Some(run) => paper_repo.find_by_run(run.id).await,
            None => Ok(Vec::new()),
        }
    } else {
        Tpsl1PositionRepo::new(db.clone())
            .find_by_rule(rule_id, limit, offset)
            .await
    }
}

/// Get all positions for a specific TPSL rule (by rule_id).
/// GET /api/strategies/tpsl1/rules/{rule_id}/positions
pub async fn get_positions_by_rule(
    app_state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let rule_id = rule_id.into_inner();
    let (limit, offset) = query.bounds();
    match load_rule_positions(&app_state.db, rule_id, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for rule {rule_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// List all positions
pub async fn list_positions(
    app_state: web::Data<Arc<AppState>>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl1_position_repo();
    let (limit, offset) = query.bounds();

    match repo.find_by_strategy("TPSL1", limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to list positions: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to list positions"}))
        }
    }
}

/// Get positions by mint (token)
pub async fn get_positions_by_mint(
    app_state: web::Data<Arc<AppState>>,
    mint: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl1_position_repo();
    let mint = mint.into_inner();
    let (limit, offset) = query.bounds();

    match repo.find_holding_by_mint(&mint, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for mint {mint}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// Get positions by wallet
pub async fn get_positions_by_wallet(
    app_state: web::Data<Arc<AppState>>,
    wallet: web::Path<String>,
    query: web::Query<PositionListParams>,
) -> impl Responder {
    let repo = app_state.tpsl1_position_repo();
    let wallet = wallet.into_inner();
    let (limit, offset) = query.bounds();

    match repo.find_holding_by_wallet(&wallet, limit, offset).await {
        Ok(positions) => {
            let responses: Vec<PositionResponse> =
                positions.into_iter().map(PositionResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => {
            tracing::error!("Failed to get positions for wallet {wallet}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get positions"}))
        }
    }
}

/// Get a specific position
pub async fn get_position(
    app_state: web::Data<Arc<AppState>>,
    position_id: web::Path<Uuid>,
) -> impl Responder {
    let repo = app_state.tpsl1_position_repo();
    let position_id = position_id.into_inner();

    match repo.find_by_id(position_id).await {
        Ok(Some(position)) => HttpResponse::Ok().json(PositionResponse::from(position)),
        Ok(None) => {
            HttpResponse::NotFound().json(serde_json::json!({"error": "Position not found"}))
        }
        Err(e) => {
            tracing::error!("Failed to get position {position_id}: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to get position"}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tpsl1::paper_position_to_sim_result;
    use crate::models::Tpsl1Rule;
    use sqlx::postgres::PgPoolOptions;
    use std::collections::{HashMap, HashSet};

    // DB-backed, like the other repo/network tests — `#[ignore]`d so it only runs
    // against a real local Postgres:
    //   $env:DATABASE_URL = "postgres://..."; cargo test -p backend -- --ignored
    // It creates a paper rule + run with unique ids and cleans up after itself.

    /// Connect to the local test DB, or `None` to skip when `DATABASE_URL` is unset.
    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .ok()
    }

    fn unique(prefix: &str) -> String {
        format!("{prefix}{}", Uuid::new_v4().simple())
    }

    /// The positions endpoint (paper branch, via `load_rule_positions`) and the
    /// paper-result endpoint both funnel through `current_run` + `find_by_run`,
    /// so they must enumerate the SAME rows for a run. This locks that invariant
    /// — including open + closed positions — so the two views can't silently
    /// diverge in count or token set.
    #[tokio::test]
    #[ignore = "requires a local Postgres (DATABASE_URL); run with --ignored"]
    async fn paper_positions_and_paper_result_agree_on_rows() {
        let Some(pool) = test_pool().await else {
            return;
        };

        // A paper rule with an active run (max total = 3).
        let rule = Tpsl1Rule::new(
            unique("paper-rule-"),
            None,
            None,
            None,
            serde_json::json!([]),
            "paper".to_string(),
            0.05,
            50.0,
            20.0,
            None,
            None,
            None,
            Some(3),
            None,
            None,
            None,
            None,
            None,
        );
        let rule_repo = Tpsl1StrategyRuleRepo::new(pool.clone());
        rule_repo.insert(&rule).await.expect("insert rule");

        let paper_repo = Tpsl1PaperTradingRepo::new(pool.clone());
        let run = paper_repo
            .start_run(rule.id, Some(3))
            .await
            .expect("start run");

        // Three positions for this run: one still Holding, one closed win, one
        // closed loss — exercising both open and exited rows.
        let mints = [unique("MINTA"), unique("MINTB"), unique("MINTC")];

        let mut open = Position::new(
            mints[0].clone(),
            unique("W"),
            "TPSL1".into(),
            rule.id,
        );
        open.entry_price = Some(0.001);
        open.entry_tx = unique("tx-");
        open.entry_token_amount = Some(0.05);
        open.entry_time = Some(Utc::now());

        let mut win = Position::new(
            mints[1].clone(),
            unique("W"),
            "TPSL1".into(),
            rule.id,
        );
        win.entry_price = Some(0.001);
        win.entry_tx = unique("tx-");
        win.entry_token_amount = Some(0.05);
        win.entry_time = Some(Utc::now());
        win.close(0.0015, unique("xtx-"), 0.075, Utc::now());

        let mut loss = Position::new(
            mints[2].clone(),
            unique("W"),
            "TPSL1".into(),
            rule.id,
        );
        loss.entry_price = Some(0.001);
        loss.entry_tx = unique("tx-");
        loss.entry_token_amount = Some(0.05);
        loss.entry_time = Some(Utc::now());
        loss.close(0.0008, unique("xtx-"), 0.04, Utc::now());

        for p in [&open, &win, &loss] {
            paper_repo
                .insert(p, run.id)
                .await
                .expect("insert paper position");
        }

        // Endpoint A: positions endpoint (paper rule → paper table, current run).
        let via_positions = load_rule_positions(&pool, rule.id, 200, 0)
            .await
            .expect("load_rule_positions");

        // Endpoint B: paper-result endpoint's selection (current run → its rows).
        let cur = paper_repo
            .current_run(rule.id)
            .await
            .expect("current_run")
            .expect("a run exists");
        let via_result_rows = paper_repo.find_by_run(cur.id).await.expect("find_by_run");

        // Same count, including the still-open position.
        assert_eq!(via_positions.len(), 3, "all three run positions are returned");
        assert_eq!(
            via_positions.len(),
            via_result_rows.len(),
            "both endpoints return the same row count",
        );

        // Same token set after each endpoint's real mapping.
        let symbols: HashMap<String, String> = HashMap::new();
        let a_mints: HashSet<String> = via_positions
            .into_iter()
            .map(PositionResponse::from)
            .map(|r| r.mint)
            .collect();
        let b_mints: HashSet<String> = via_result_rows
            .into_iter()
            .map(|p| paper_position_to_sim_result(p, &symbols))
            .map(|r| r.mint)
            .collect();
        assert_eq!(a_mints, b_mints, "both endpoints return the same token set");

        let expected: HashSet<String> = mints.iter().cloned().collect();
        assert_eq!(a_mints, expected, "the set is exactly the inserted mints");

        // Cleanup: deleting the rule cascades the run and its paper positions.
        rule_repo.delete(rule.id).await.expect("delete rule");
    }
}
