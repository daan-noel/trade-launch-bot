//! Thin HTTP surface for the ANALYSIS box (workstation only). Health + read
//! endpoints over the local mirror. Sweeps/backtests/simulate land in later
//! phases (they read the lake, not PG). Intentionally its OWN route config, not
//! shared with `live` — the two composition roots keep disjoint surfaces.

use actix_web::{web, HttpResponse};
use sqlx::PgPool;

use platform_core::storage::repositories::{LaunchpadRepo, QuoteAssetRepo, TokenRepo, TradeRepo};

fn e500<E: std::fmt::Debug>(e: E) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("{e:?}"))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health))
        .route("/api/quote_assets", web::get().to(quote_assets))
        .route("/api/launchpads", web::get().to(launchpads))
        .route("/api/tokens/{mint}/overview", web::get().to(token_overview))
        .route("/api/tokens/{mint}/trades", web::get().to(token_trades));
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok", "service": "lab" }))
}

async fn quote_assets(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    Ok(HttpResponse::Ok().json(QuoteAssetRepo::all(pool.get_ref()).await.map_err(e500)?))
}

async fn launchpads(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    Ok(HttpResponse::Ok().json(LaunchpadRepo::all(pool.get_ref()).await.map_err(e500)?))
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
    #[serde(default)]
    limit: i64, // 0 (default) => full mint-scoped history (analysis reads uncapped)
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
