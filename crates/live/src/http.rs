//! Thin HTTP surface for the LIVE box: health + data-layer reads + launch trigger.

use actix_web::{web, HttpResponse};
use launcher::{execute_launch, LaunchRequest, LauncherSettings};
use sqlx::PgPool;
use uuid::Uuid;

use platform_core::storage::repositories::{
    LaunchRepo, LaunchpadRepo, QuoteAssetRepo, TokenRepo, TradeRepo,
};

/// Map any error to a 500 without leaking a panic.
fn e500<E: std::fmt::Debug>(e: E) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("{e:?}"))
}

/// Register the routes. `api` pool is shared via `app_data`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/api/quote_assets", web::get().to(quote_assets))
        .route("/api/launchpads", web::get().to(launchpads))
        .route("/api/launches/{id}", web::get().to(launch_get))
        .route("/api/launches/execute", web::post().to(launch_execute))
        .route("/api/tokens/{mint}/overview", web::get().to(token_overview))
        .route("/api/tokens/{mint}/trades", web::get().to(token_trades));
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok", "service": "live" }))
}

async fn quote_assets(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    let rows = QuoteAssetRepo::all(pool.get_ref()).await.map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

async fn launchpads(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    let rows = LaunchpadRepo::all(pool.get_ref()).await.map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(serde::Deserialize)]
struct LaunchExecuteBody {
    template_id: Uuid,
    dev_wallet_id: Uuid,
}

async fn launch_execute(
    pool: web::Data<PgPool>,
    body: web::Json<LaunchExecuteBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = LauncherSettings::from_env().map_err(|e| {
        actix_web::error::ErrorServiceUnavailable(format!("launcher not configured: {e}"))
    })?;
    let result = execute_launch(
        pool.get_ref(),
        &settings,
        LaunchRequest {
            template_id: body.template_id,
            dev_wallet_id: body.dev_wallet_id,
        },
    )
    .await
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(result))
}

async fn launch_get(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    match LaunchRepo::get(pool.get_ref(), *id).await.map_err(e500)? {
        Some(row) => Ok(HttpResponse::Ok().json(row)),
        None => Ok(HttpResponse::NotFound().json(serde_json::json!({ "error": "not found" }))),
    }
}

async fn token_overview(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    match TokenRepo::overview(pool.get_ref(), &mint).await.map_err(e500)? {
        Some(t) => Ok(HttpResponse::Ok().json(t)),
        None => Ok(HttpResponse::NotFound().json(serde_json::json!({ "error": "not found" }))),
    }
}

#[derive(serde::Deserialize)]
struct TradesQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}
fn default_limit() -> i64 {
    200
}

async fn token_trades(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
    q: web::Query<TradesQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = TradeRepo::find_priced_by_mint(pool.get_ref(), &mint, q.limit)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}
