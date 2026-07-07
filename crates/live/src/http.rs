//! Thin HTTP surface for the LIVE box: health + data-layer reads + launch trigger.

use actix_web::{web, HttpResponse};
use launcher::{execute_bundle, execute_launch, LaunchRequest, LauncherSettings};
use platform_core::models::{Bundle, Launch, TradePriced};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, LaunchTemplateRepo, LaunchpadRepo, ManagedWalletRepo, QuoteAssetRepo,
    TokenRepo, TradeRepo,
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
        .route("/api/launch_templates", web::get().to(launch_templates_list))
        .route("/api/managed_wallets", web::get().to(managed_wallets_list))
        .route("/api/wallet_pool", web::get().to(wallet_pool_list))
        .route("/api/wallet_pool/generate", web::post().to(wallet_pool_generate))
        .route("/api/launches/{id}", web::get().to(launch_get))
        .route("/api/launches/{id}/status", web::get().to(launch_status))
        .route("/api/launches/execute", web::post().to(launch_execute))
        .route("/api/bundles/{id}", web::get().to(bundle_get))
        .route("/api/bundles/{id}/execute", web::post().to(bundle_execute))
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

async fn launch_templates_list(
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = LaunchTemplateRepo::all(pool.get_ref()).await.map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(serde::Deserialize)]
struct WalletsQuery {
    role: Option<String>,
}

async fn managed_wallets_list(
    pool: web::Data<PgPool>,
    q: web::Query<WalletsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = ManagedWalletRepo::list(pool.get_ref(), q.role.as_deref())
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

/// Full pool (every lifecycle status, including `retired`) for the Wallet
/// Management page — `ManagedWallet.key_ref` is `#[serde(skip_serializing)]`, so
/// no raw key material or keystore path ever reaches the frontend.
async fn wallet_pool_list(
    pool: web::Data<PgPool>,
    q: web::Query<WalletsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = ManagedWalletRepo::list_all(pool.get_ref(), q.role.as_deref())
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(serde::Deserialize)]
struct GenerateWalletsBody {
    role: String,
    count: u32,
    #[serde(default)]
    label_prefix: Option<String>,
}

/// Batch-generate N fresh wallets for a role (envelope-encrypted keystore write +
/// `generated` DB rows) — `launcher::generate_wallets` (wallet-pool Phase 1).
async fn wallet_pool_generate(
    pool: web::Data<PgPool>,
    body: web::Json<GenerateWalletsBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = LauncherSettings::from_env().map_err(|e| {
        actix_web::error::ErrorServiceUnavailable(format!("launcher not configured: {e}"))
    })?;
    let wallets = launcher::generate_wallets(
        pool.get_ref(),
        &settings,
        &body.role,
        body.count,
        body.label_prefix.as_deref(),
    )
    .await
    .map_err(e500)?;

    // Best-effort backup after each generation batch (wallet-pool Phase 4) —
    // fire-and-forget so a backup problem never fails the generate response;
    // no-op when WALLET_BACKUP_DIR isn't configured.
    if let Some(backup_dir) = settings.backup_dir.clone() {
        let backup_pool = pool.get_ref().clone();
        tokio::spawn(async move {
            if let Err(e) = launcher::run_backup(&backup_pool, &settings, &backup_dir).await {
                tracing::warn!(%e, "wallet-pool backup failed after generation batch");
            }
        });
    }

    Ok(HttpResponse::Ok().json(wallets))
}

#[derive(Debug, Serialize)]
struct LaunchStatusResponse {
    launch: Launch,
    bundle: Option<Bundle>,
    trade_count: i64,
    trades: Vec<TradePriced>,
}

async fn launch_status(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let launch = match LaunchRepo::get(pool.get_ref(), *id).await.map_err(e500)? {
        Some(row) => row,
        None => {
            return Ok(HttpResponse::NotFound().json(serde_json::json!({ "error": "not found" })));
        }
    };

    let bundle = if let Some(bundle_id) = launch.bundle_id {
        BundleRepo::get(pool.get_ref(), bundle_id).await.map_err(e500)?
    } else {
        None
    };

    let trade_count = TradeRepo::count_by_mint(pool.get_ref(), &launch.mint_address)
        .await
        .map_err(e500)?;
    let trades = TradeRepo::find_priced_by_mint(pool.get_ref(), &launch.mint_address, 50)
        .await
        .map_err(e500)?;

    Ok(HttpResponse::Ok().json(LaunchStatusResponse {
        launch,
        bundle,
        trade_count,
        trades,
    }))
}

#[derive(serde::Deserialize)]
struct LaunchExecuteBody {
    template_id: Uuid,
    dev_wallet_id: Uuid,
    /// Metadata editing panel overrides — unset falls back to the template.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    /// "Use N bundlers" override — unset falls back to the template's
    /// `bundle_leg_count` default.
    #[serde(default)]
    bundler_count: Option<u32>,
}

async fn launch_execute(
    pool: web::Data<PgPool>,
    body: web::Json<LaunchExecuteBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = LauncherSettings::from_env().map_err(|e| {
        actix_web::error::ErrorServiceUnavailable(format!("launcher not configured: {e}"))
    })?;
    let body = body.into_inner();
    let result = execute_launch(
        pool.get_ref(),
        &settings,
        LaunchRequest {
            template_id: body.template_id,
            dev_wallet_id: body.dev_wallet_id,
            name: body.name,
            symbol: body.symbol,
            uri: body.uri,
            bundler_count: body.bundler_count,
        },
    )
    .await
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(result))
}

async fn bundle_execute(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = LauncherSettings::from_env().map_err(|e| {
        actix_web::error::ErrorServiceUnavailable(format!("launcher not configured: {e}"))
    })?;
    let result = execute_bundle(pool.get_ref(), &settings, *id)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(result))
}

async fn bundle_get(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    match BundleRepo::get(pool.get_ref(), *id).await.map_err(e500)? {
        Some(row) => Ok(HttpResponse::Ok().json(row)),
        None => Ok(HttpResponse::NotFound().json(serde_json::json!({ "error": "not found" }))),
    }
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
