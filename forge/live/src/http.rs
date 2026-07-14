//! Thin HTTP surface for the LIVE box: health + data-layer reads + launch trigger.

use actix_web::{web, HttpRequest, HttpResponse};
use crate::ingest::IngestHandle;
use launcher::{
    arm_ladder, consolidate_all, create_metadata_template, dev_launch_required_lamports,
    execute_action, execute_bundle, execute_launch, export_wallet_base58, fund_for_launch,
    fund_once, start_volume_bot, sweep_used_and_retired, transfer_between_wallets,
    update_metadata_template, FundMode, FundPlan, FundScope, InsufficientDevBalance, LadderRung,
    LaunchRequest, LauncherSettings, ManageRequest, NewMetadataTemplateRequest,
    PumpfunTemplateParams, TransferAmount, UpdateMetadataTemplateRequest, VolumeConfig,
    WalletSelection,
};
use platform_core::models::{
    Bundle, Launch, NewLaunchTemplate, TradePriced, UpdateLaunchTemplate, WalletRole,
};
use serde::Serialize;
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, LaunchTemplateRepo, LaunchpadRepo, ManageActionRepo, ManagedWalletRepo,
    MetadataTemplateRepo, QuoteAssetRepo, SellLadderRepo, TokenRepo, TradeRepo, VolumeBotRepo,
};

/// Map any error to a 500 with a GENERIC client body. The full detail (which can
/// carry SQL text / anyhow chains / internal paths) is logged server-side, never
/// returned to the caller.
fn e500<E: std::fmt::Debug>(e: E) -> actix_web::Error {
    tracing::error!(error = ?e, "internal error serving request");
    actix_web::error::ErrorInternalServerError("internal server error")
}

/// Map a validation error to a 400. Returns the Display (top-level) message so the
/// operator sees the actionable reason (bad rung, bad config, …) — NOT the `{e:?}`
/// debug chain, which leaks internals. Full detail is logged server-side.
fn e400<E: std::fmt::Display + std::fmt::Debug>(e: E) -> actix_web::Error {
    tracing::warn!(error = ?e, "bad request");
    actix_web::error::ErrorBadRequest(format!("{e}"))
}

/// Borrow the boot-time launcher settings out of `app_data`, or 503 if the box
/// booted without a launcher config (missing RPC/keystore/nonce env). Built once
/// in `main.rs`; handlers no longer re-parse env per request.
fn launcher_settings(
    data: &web::Data<Option<LauncherSettings>>,
) -> Result<&LauncherSettings, actix_web::Error> {
    data.get_ref().as_ref().ok_or_else(|| {
        actix_web::error::ErrorServiceUnavailable(
            "launcher not configured (missing RPC/keystore/nonce env)",
        )
    })
}

/// Register the routes. `api` pool is shared via `app_data`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    // Default actix JSON limit (32KB) is too small for a base64-encoded token
    // image (`POST /api/metadata_templates`) — raised app-wide since no other
    // JSON body here is large enough to make a higher ceiling a real risk.
    cfg.app_data(web::JsonConfig::default().limit(10 * 1024 * 1024));
    cfg.route("/health", web::get().to(health))
        .route("/api/stream", web::get().to(crate::sse::stream_events))
        .route("/api/ingest", web::get().to(ingest_status))
        .route("/api/ingest", web::put().to(ingest_toggle))
        .route("/api/bootstrap", web::get().to(bootstrap))
        .route("/api/quote_assets", web::get().to(quote_assets))
        .route("/api/launchpads", web::get().to(launchpads))
        .route("/api/launch_templates", web::get().to(launch_templates_list))
        .route("/api/launch_templates", web::post().to(launch_templates_create))
        .route(
            "/api/launch_templates/{id}",
            web::put().to(launch_templates_update),
        )
        .route(
            "/api/launch_templates/{id}",
            web::delete().to(launch_templates_delete),
        )
        .route("/api/wallet_pool", web::get().to(wallet_pool_list))
        .route("/api/wallet_pool/generate", web::post().to(wallet_pool_generate))
        .route("/api/wallet_pool/fund", web::post().to(wallet_pool_fund))
        .route(
            "/api/wallet_pool/fund_for_launch",
            web::post().to(wallet_pool_fund_for_launch),
        )
        .route(
            "/api/wallet_pool/refresh_balances",
            web::post().to(wallet_pool_refresh_balances),
        )
        .route("/api/wallet_pool/transfer", web::post().to(wallet_pool_transfer))
        .route("/api/wallet_pool/sweep", web::post().to(wallet_pool_sweep))
        .route(
            "/api/wallet_pool/consolidate",
            web::post().to(wallet_pool_consolidate),
        )
        .route(
            "/api/wallet_pool/{id}/export",
            web::post().to(wallet_pool_export),
        )
        .route("/api/metadata_templates", web::get().to(metadata_templates_list))
        .route(
            "/api/metadata_templates",
            web::post().to(metadata_templates_create),
        )
        .route(
            "/api/metadata_templates/{id}",
            web::put().to(metadata_templates_update),
        )
        .route(
            "/api/metadata_templates/{id}",
            web::delete().to(metadata_templates_delete),
        )
        .route("/api/launches", web::get().to(launches_list))
        // Registered BEFORE `/{id}` so the literal path isn't captured as an id.
        .route("/api/launches/requirement", web::get().to(launch_requirement))
        .route("/api/launches/{id}", web::get().to(launch_get))
        .route("/api/launches/{id}/status", web::get().to(launch_status))
        .route("/api/launches/execute", web::post().to(launch_execute))
        .route("/api/bundles/{id}", web::get().to(bundle_get))
        .route("/api/bundles/{id}/execute", web::post().to(bundle_execute))
        .route("/api/tokens/{mint}/overview", web::get().to(token_overview))
        .route("/api/tokens/{mint}/trades", web::get().to(token_trades))
        .route("/api/tokens/{mint}/positions", web::get().to(token_positions))
        .route(
            "/api/tokens/{mint}/positions/refresh",
            web::post().to(positions_refresh),
        )
        .route(
            "/api/tokens/{mint}/manage/preview",
            web::post().to(manage_preview),
        )
        .route(
            "/api/tokens/{mint}/manage/execute",
            web::post().to(manage_execute),
        )
        .route(
            "/api/tokens/{mint}/manage/actions",
            web::get().to(manage_actions),
        )
        .route(
            "/api/tokens/{mint}/manage/ladders",
            web::get().to(ladders_list),
        )
        .route(
            "/api/tokens/{mint}/manage/ladders",
            web::post().to(ladder_arm),
        )
        .route("/api/manage/ladders/{id}", web::delete().to(ladder_cancel))
        .route(
            "/api/tokens/{mint}/manage/volume",
            web::get().to(volume_list),
        )
        .route(
            "/api/tokens/{mint}/manage/volume",
            web::post().to(volume_start),
        )
        .route("/api/manage/volume/{id}/pause", web::post().to(volume_pause))
        .route(
            "/api/manage/volume/{id}/resume",
            web::post().to(volume_resume),
        )
        .route("/api/manage/volume/{id}", web::delete().to(volume_stop));
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok", "service": "live" }))
}

#[derive(Debug, Serialize)]
struct IngestStatusResponse {
    /// `false` when the box booted without Helius creds — no handle exists to toggle.
    configured: bool,
    live: bool,
}

async fn ingest_status(handle: web::Data<Option<Arc<IngestHandle>>>) -> HttpResponse {
    let live = handle.as_ref().as_ref().map(|h| h.is_live()).unwrap_or(false);
    HttpResponse::Ok().json(IngestStatusResponse {
        configured: handle.is_some(),
        live,
    })
}

#[derive(serde::Deserialize)]
struct SetIngestBody {
    live: bool,
}

async fn ingest_toggle(
    handle: web::Data<Option<Arc<IngestHandle>>>,
    sse: web::Data<crate::sse::SseHub>,
    body: web::Json<SetIngestBody>,
) -> Result<HttpResponse, actix_web::Error> {
    match handle.as_ref() {
        Some(h) => {
            h.set_live(body.live);
            // Push the new state so every dashboard's badge flips at once instead
            // of waiting on its 5s poll (the poll stays as a gap-heal fallback).
            sse.ingest_status(h.is_live());
            Ok(HttpResponse::Ok().json(IngestStatusResponse {
                configured: true,
                live: h.is_live(),
            }))
        }
        None => Ok(HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "ingest not configured — HELIUS_LASERSTREAM_URL / HELIUS_API_KEY not set"
        }))),
    }
}

/// One-shot composite for the launch console's initial load: templates + dev
/// wallets + metadata templates in a single round trip (fetched concurrently),
/// instead of the frontend firing three separate GETs on mount.
async fn bootstrap(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    let (templates, dev_wallets, metadata_templates) = tokio::try_join!(
        LaunchTemplateRepo::all(pool.get_ref()),
        ManagedWalletRepo::list_all(pool.get_ref(), Some(WalletRole::Dev.as_str())),
        MetadataTemplateRepo::all(pool.get_ref()),
    )
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "templates": templates,
        "dev_wallets": dev_wallets,
        "metadata_templates": metadata_templates,
    })))
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

/// Validate `params` against the one struct that actually consumes it at
/// launch-execute time (`launcher::PumpfunTemplateParams`) — reuses that shape
/// as the single source of truth instead of duplicating a JSON schema here, and
/// stops a malformed template from only failing much later, mid-launch.
fn validate_params(params: &Option<serde_json::Value>) -> Result<(), actix_web::Error> {
    let value = params.clone().unwrap_or_else(|| serde_json::json!({}));
    let parsed = serde_json::from_value::<PumpfunTemplateParams>(value)
        .map_err(|e| actix_web::error::ErrorBadRequest(format!("invalid template params: {e}")))?;
    // Fail-closed on any invalid template piece — a tokens-out bundler-leg variant
    // (can't encode a SOL amount) or a hand-picked ix layout that breaks the
    // landing-safety rails (the same checks the plan gate re-runs before send).
    parsed
        .validate()
        .map_err(actix_web::error::ErrorBadRequest)?;
    Ok(())
}

async fn launch_templates_create(
    pool: web::Data<PgPool>,
    body: web::Json<NewLaunchTemplate>,
) -> Result<HttpResponse, actix_web::Error> {
    let body = body.into_inner();
    validate_params(&body.params)?;
    let row = LaunchTemplateRepo::insert(pool.get_ref(), &body).await.map_err(e500)?;
    Ok(HttpResponse::Ok().json(row))
}

async fn launch_templates_update(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateLaunchTemplate>,
) -> Result<HttpResponse, actix_web::Error> {
    let body = body.into_inner();
    validate_params(&body.params)?;
    let row = LaunchTemplateRepo::update(pool.get_ref(), path.into_inner(), &body)
        .await
        .map_err(e500)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("template not found"))?;
    Ok(HttpResponse::Ok().json(row))
}

async fn launch_templates_delete(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if LaunchTemplateRepo::delete(pool.get_ref(), path.into_inner())
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("template not found"))
    }
}

#[derive(serde::Deserialize)]
struct WalletsQuery {
    role: Option<String>,
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

/// `POST /api/wallet_pool/refresh_balances` — one live `getMultipleAccounts` burst
/// over EVERY managed wallet (optionally scoped to `?role=`), writing each cached
/// balance so the pool page can show an exact, current total (incl. `used`/`retired`
/// wallets the steady poller leaves frozen). Operator-triggered; not on any hot
/// path — see `launcher::refresh_all_balances`.
async fn wallet_pool_refresh_balances(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    q: web::Query<WalletsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let rows = launcher::refresh_all_balances(pool.get_ref(), &settings.rpc_url, q.role.as_deref())
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
    settings: web::Data<Option<LauncherSettings>>,
    body: web::Json<GenerateWalletsBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let wallets = launcher::generate_wallets(
        pool.get_ref(),
        settings,
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
        let settings = settings.clone();
        tokio::spawn(async move {
            if let Err(e) = launcher::run_backup(&backup_pool, &settings, &backup_dir).await {
                tracing::warn!(%e, "wallet-pool backup failed after generation batch");
            }
        });
    }

    Ok(HttpResponse::Ok().json(wallets))
}

#[derive(serde::Deserialize)]
struct FundWalletsBody {
    /// Restrict the pass to one role (unset = both fundable roles: dev + bundler).
    #[serde(default)]
    role: Option<String>,
    /// Fund exactly this many (unset = top each role up to its warm target).
    #[serde(default)]
    count: Option<i64>,
}

/// On-demand treasury -> pool funding pass (docs/wallet-funding-plan.md P4).
/// Requires `FUND_ENABLED=true` (503 otherwise). Best-effort: returns the
/// per-wallet outcome for every wallet the pass touched.
async fn wallet_pool_fund(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    sse: web::Data<crate::sse::SseHub>,
    body: web::Json<FundWalletsBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    if settings.funding.is_none() {
        return Ok(HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "wallet funding disabled — set FUND_ENABLED=true to enable"
        })));
    }
    let body = body.into_inner();
    let role = match body.role.as_deref() {
        Some(r) => Some(
            WalletRole::from_str(r)
                .map_err(|e| actix_web::error::ErrorBadRequest(format!("invalid role: {e}")))?,
        ),
        None => None,
    };
    let report = fund_once(
        pool.get_ref(),
        settings,
        FundScope { role, count: body.count },
        FundMode::Manual,
        Some(sse.get_ref()),
    )
    .await
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(report))
}

#[derive(serde::Deserialize)]
struct FundForLaunchBody {
    /// The launch template whose dev-buy + per-leg amounts drive the funding.
    template_id: Uuid,
    /// The specific dev wallet this launch will use (funded to the launch gate).
    dev_wallet_id: Uuid,
    /// "Use N bundlers" override — unset falls back to the template's
    /// `bundle_leg_count`. Must match what the subsequent launch passes.
    #[serde(default)]
    bundler_count: Option<u32>,
}

/// Just-in-time, template-driven pre-launch funding (docs/jit-funding-plan.md):
/// tops the chosen dev wallet + `leg_count` bundler wallets up to the amounts the
/// selected template will actually spend, drawing from the treasury pool. Runs in
/// `FundMode::Background` (confirms each send) so the wallets are `funded` — and
/// so claimable by `execute_launch` — the moment this returns. Requires
/// `FUND_ENABLED=true` (503 otherwise).
async fn wallet_pool_fund_for_launch(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    sse: web::Data<crate::sse::SseHub>,
    body: web::Json<FundForLaunchBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    if settings.funding.is_none() {
        return Ok(HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "wallet funding disabled — set FUND_ENABLED=true to enable"
        })));
    }
    let body = body.into_inner();
    let report = fund_for_launch(
        pool.get_ref(),
        settings,
        body.template_id,
        body.bundler_count,
        body.dev_wallet_id,
        FundMode::Background,
        Some(sse.get_ref()),
    )
    .await
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(report))
}

#[derive(serde::Deserialize)]
struct TransferBody {
    from_id: Uuid,
    to_id: Uuid,
    /// Whole SOL to move. Ignored when `max` is set; required otherwise.
    #[serde(default)]
    amount_sol: Option<f64>,
    /// Sweep the source to ~0 instead of an exact amount.
    #[serde(default)]
    max: Option<bool>,
}

/// Operator wallet-to-wallet SOL move (docs/wallet-transfer-plan.md). The source
/// signs + pays the fee. Bearer-gated like every mutating route. 503 if the
/// launcher isn't configured. Returns the `TransferReport` (signature + lamports).
async fn wallet_pool_transfer(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    body: web::Json<TransferBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let body = body.into_inner();
    let amount = if body.max == Some(true) {
        TransferAmount::Max
    } else {
        let sol = body
            .amount_sol
            .ok_or_else(|| actix_web::error::ErrorBadRequest("amount_sol required unless max=true"))?;
        TransferAmount::Exact(sol)
    };
    let report = transfer_between_wallets(pool.get_ref(), settings, body.from_id, body.to_id, amount)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(report))
}

/// `POST /api/wallet_pool/sweep` — operator "Sweep & retire" pass: run the
/// `used`-wallet dust sweep (skipping open-position holders) AND reclaim already-
/// `retired` wallets (residual SOL + close empty token accounts). All lands in the
/// oldest treasury. Bearer-gated; 503 if the launcher isn't configured. Returns the
/// per-wallet `SweepReport`.
async fn wallet_pool_sweep(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let report = sweep_used_and_retired(pool.get_ref(), settings)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(report))
}

#[derive(serde::Deserialize)]
struct ConsolidateBody {
    /// The treasury-role wallet every other wallet's SOL + reclaimed rent drains into.
    dest_treasury_id: Uuid,
}

/// `POST /api/wallet_pool/consolidate` — operator "Consolidate → treasury" pass:
/// drain SOL + close empty token accounts on EVERY managed wallet (incl. other
/// treasuries) into `dest_treasury_id`. Skips mid-launch (`funding`/`reserved`)
/// wallets. Bearer-gated; 503 if the launcher isn't configured.
async fn wallet_pool_consolidate(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    body: web::Json<ConsolidateBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let report = consolidate_all(pool.get_ref(), settings, body.into_inner().dest_treasury_id)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(report))
}

/// Constant-time byte comparison — avoids leaking the secret length/prefix via
/// early-exit timing on a remote brute-force.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// `POST /api/wallet_pool/{id}/export` — DANGER: returns a wallet's raw base58
/// private key. Gated on `WALLET_EXPORT_SECRET` (via `X-Export-Secret`); 403 when
/// the secret is unset (endpoint hard-disabled) or the header doesn't match. The
/// response is `no-store` and the key is never logged. Serve over TLS only — the
/// body is spendable key material.
async fn wallet_pool_export(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;

    // Endpoint is disabled unless an export secret is configured.
    let Some(expected) = settings.export_secret.as_deref() else {
        return Ok(HttpResponse::Forbidden().json(serde_json::json!({
            "error": "wallet export disabled — set WALLET_EXPORT_SECRET to enable"
        })));
    };

    let presented = req
        .headers()
        .get("X-Export-Secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !ct_eq(presented.as_bytes(), expected.as_bytes()) {
        // Throttle brute-force: a fixed delay caps guess rate to a crawl even
        // over a fast remote link. Constant regardless of where the mismatch is.
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        let managed_wallet_id = path.into_inner();
        tracing::warn!(%managed_wallet_id, "wallet export rejected — bad or missing X-Export-Secret");
        return Ok(HttpResponse::Forbidden().json(serde_json::json!({
            "error": "invalid export secret"
        })));
    }

    let managed_wallet_id = path.into_inner();
    let exported = export_wallet_base58(pool.get_ref(), settings, managed_wallet_id)
        .await
        .map_err(e500)?;

    // Never log the key; never let a cache retain it.
    tracing::info!(%managed_wallet_id, address = %exported.address, "wallet private key exported");
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(serde_json::json!({
            "address": exported.address,
            "private_key_base58": exported.private_key_base58.as_str(),
        })))
}

async fn metadata_templates_list(pool: web::Data<PgPool>) -> Result<HttpResponse, actix_web::Error> {
    let rows = MetadataTemplateRepo::all(pool.get_ref()).await.map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(serde::Deserialize)]
struct CreateMetadataTemplateBody {
    template_name: String,
    name: String,
    symbol: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    twitter: Option<String>,
    #[serde(default)]
    telegram: Option<String>,
    #[serde(default)]
    website: Option<String>,
    /// Base64 image bytes (the frontend's file picker read as a data URL, with
    /// the `data:<mime>;base64,` prefix stripped).
    image_base64: String,
    image_filename: String,
    image_content_type: String,
}

/// Pin the image + build/pin the standard off-chain JSON to Pinata, then
/// persist as a reusable template — `launcher::create_metadata_template`.
async fn metadata_templates_create(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    body: web::Json<CreateMetadataTemplateBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let body = body.into_inner();
    let template = create_metadata_template(
        pool.get_ref(),
        settings,
        NewMetadataTemplateRequest {
            template_name: body.template_name,
            name: body.name,
            symbol: body.symbol,
            description: body.description,
            twitter: body.twitter,
            telegram: body.telegram,
            website: body.website,
            image_base64: body.image_base64,
            image_filename: body.image_filename,
            image_content_type: body.image_content_type,
        },
    )
    .await
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(template))
}

#[derive(serde::Deserialize)]
struct UpdateMetadataTemplateBody {
    template_name: String,
    name: String,
    symbol: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    twitter: Option<String>,
    #[serde(default)]
    telegram: Option<String>,
    #[serde(default)]
    website: Option<String>,
    /// Optional replacement image (base64, prefix stripped). Omit all three
    /// image fields to keep the existing pinned image and only re-pin the JSON.
    #[serde(default)]
    image_base64: Option<String>,
    #[serde(default)]
    image_filename: Option<String>,
    #[serde(default)]
    image_content_type: Option<String>,
}

/// Re-pin (image only if replaced) + full-replace an existing metadata
/// template — `launcher::update_metadata_template`.
async fn metadata_templates_update(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateMetadataTemplateBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let body = body.into_inner();
    let template = update_metadata_template(
        pool.get_ref(),
        settings,
        path.into_inner(),
        UpdateMetadataTemplateRequest {
            template_name: body.template_name,
            name: body.name,
            symbol: body.symbol,
            description: body.description,
            twitter: body.twitter,
            telegram: body.telegram,
            website: body.website,
            image_base64: body.image_base64,
            image_filename: body.image_filename,
            image_content_type: body.image_content_type,
        },
    )
    .await
    .map_err(e500)?
    .ok_or_else(|| actix_web::error::ErrorNotFound("metadata template not found"))?;
    Ok(HttpResponse::Ok().json(template))
}

/// Delete a metadata template. Launch templates referencing it keep working —
/// their `metadata_template_id` is unset by the FK `ON DELETE SET NULL`.
async fn metadata_templates_delete(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if MetadataTemplateRepo::delete(pool.get_ref(), path.into_inner())
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("metadata template not found"))
    }
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

    // The bundle (if any) and the priced trades + count are independent given the
    // launch row — fetch them concurrently, and get the count from the same query
    // as the page (one windowed read instead of a separate count scan).
    let bundle_fut = async {
        match launch.bundle_id {
            Some(bundle_id) => BundleRepo::get(pool.get_ref(), bundle_id).await,
            None => Ok(None),
        }
    };
    let trades_fut =
        TradeRepo::find_priced_page_with_count(pool.get_ref(), &launch.mint_address, 50);
    let (bundle, (trades, trade_count)) =
        tokio::try_join!(bundle_fut, trades_fut).map_err(e500)?;

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
    /// Per-launch metadata override — pick a different `metadata_templates` row;
    /// unset falls back to the template's own `metadata_template_id`.
    #[serde(default)]
    metadata_template_id: Option<Uuid>,
    /// "Use N bundlers" override — unset falls back to the template's
    /// `bundle_leg_count` default.
    #[serde(default)]
    bundler_count: Option<u32>,
}

async fn launch_execute(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    body: web::Json<LaunchExecuteBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let body = body.into_inner();
    let result = execute_launch(
        pool.get_ref(),
        settings,
        LaunchRequest {
            template_id: body.template_id,
            dev_wallet_id: body.dev_wallet_id,
            metadata_template_id: body.metadata_template_id,
            bundler_count: body.bundler_count,
        },
    )
    .await
    // Surface the pre-launch balance gate as an actionable 400 ("needs X, has Y")
    // instead of the opaque 500 every other error collapses to.
    .map_err(|e| match e.downcast::<InsufficientDevBalance>() {
        Ok(insufficient) => e400(insufficient),
        // Unlike the generic `e500` (used on public reads, which must not leak
        // internals), this endpoint is the bearer-gated, nginx-fronted OPERATOR API
        // — so a launch failure returns the full anyhow CONTEXT chain (`{:#}` joins
        // every `.context()` message: e.g. "sign create tx: … Transaction simulation
        // failed: … custom program error: 0x…"), making the failure self-diagnosing
        // instead of a blank "internal server error". `{:#}` is the human-readable
        // source chain only — NOT the `{:?}` debug form, so no file paths / backtrace
        // leak. Full detail is still logged server-side.
        Err(other) => {
            tracing::error!(error = ?other, "launch execute failed");
            actix_web::error::ErrorInternalServerError(format!("launch failed: {other:#}"))
        }
    })?;
    Ok(HttpResponse::Ok().json(result))
}

#[derive(serde::Deserialize)]
struct LaunchRequirementQuery {
    template_id: Uuid,
    /// Optional: include this dev wallet's last-observed balance so the caller can
    /// render the shortfall without a second round-trip.
    #[serde(default)]
    dev_wallet_id: Option<Uuid>,
    /// "Use N bundlers" override — unset falls back to the template default.
    #[serde(default)]
    bundler_count: Option<u32>,
}

#[derive(Serialize)]
struct LaunchRequirementResponse {
    /// The dev-wallet minimum the launch gate enforces (rent + fees + live tip
    /// ceiling + dev-buy). This is the SAME figure `execute_launch` checks.
    dev_required_lamports: u64,
    /// Per-bundler-leg funding target (buy + tip + headroom); 0 if no bundle.
    per_leg_lamports: u64,
    /// Number of bundler legs this launch would run (0 = no bundle).
    leg_count: u32,
    /// The selected dev wallet's last-observed balance (lamports), if a wallet was
    /// given and its balance has been polled. `null` otherwise.
    dev_balance_lamports: Option<i64>,
}

/// `GET /api/launches/requirement` — read-only pre-launch cost preview. Reuses the
/// exact SSOT (`dev_launch_required_lamports` / `FundPlan`) the gate and funder
/// use, so the number shown can't drift from the number enforced. Spends nothing,
/// so it is NOT gated on `FUND_ENABLED`.
async fn launch_requirement(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    query: web::Query<LaunchRequirementQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let q = query.into_inner();
    let template = LaunchTemplateRepo::get(pool.get_ref(), q.template_id)
        .await
        .map_err(e500)?
        .ok_or_else(|| actix_web::error::ErrorNotFound("launch template not found"))?;
    let variant = template.variant.clone();
    let params: PumpfunTemplateParams = serde_json::from_value(template.params).map_err(e400)?;
    let tip_ceiling = settings.launch_tip_ceiling_lamports();
    let dev_required_lamports = dev_launch_required_lamports(&variant, &params, tip_ceiling);
    let plan =
        FundPlan::from_params(&variant, &params, q.bundler_count, tip_ceiling).map_err(e400)?;
    let dev_balance_lamports = match q.dev_wallet_id {
        Some(id) => ManagedWalletRepo::get(pool.get_ref(), id)
            .await
            .map_err(e500)?
            .and_then(|w| w.balance_lamports),
        None => None,
    };
    Ok(HttpResponse::Ok().json(LaunchRequirementResponse {
        dev_required_lamports,
        per_leg_lamports: plan.per_leg_lamports,
        leg_count: plan.leg_count,
        dev_balance_lamports,
    }))
}

async fn bundle_execute(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let result = execute_bundle(pool.get_ref(), settings, *id)
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

#[derive(serde::Deserialize)]
struct LaunchesQuery {
    #[serde(default = "default_launches_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn default_launches_limit() -> i64 {
    100
}

/// Newest-first page of enriched launch rows for the "launched tokens" list +
/// the total count (for pagination). One round trip: the page and count run
/// concurrently. `limit` is clamped to a sane ceiling so a bad query param can't
/// ask for an unbounded scan.
async fn launches_list(
    pool: web::Data<PgPool>,
    q: web::Query<LaunchesQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let limit = q.limit.clamp(1, 500);
    let offset = q.offset.max(0);
    let (launches, total) = tokio::try_join!(
        LaunchRepo::list_page(pool.get_ref(), limit, offset),
        LaunchRepo::count(pool.get_ref()),
    )
    .map_err(e500)?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "launches": launches, "total": total })))
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

/// Per-wallet holdings for a launched token (post-launch management, Phase 1).
/// Balance, cost basis, and realized proceeds are all **feed-derived** — replayed
/// from the ingested `trades` with **zero RPC** — so this read never issues a
/// network call however often the page refetches. On-chain truth (an external
/// transfer the feed can't see, or a dropped feed leg) is corrected only on the
/// explicit "Refresh" action (`POST …/positions/refresh`, the sole RPC path).
async fn token_positions(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let mint = mint.into_inner();
    let rows = launcher::read_positions(pool.get_ref(), &mint)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

/// Refresh a mint's holdings against **chain** — the one and only RPC balance path
/// for holdings. The plain GET reads are feed-derived (zero RPC); this explicit
/// action runs the batched on-chain reconcile (`getMultipleAccounts` over derived
/// ATAs) so an external transfer or a missed feed leg is corrected, then returns
/// the refreshed rows. Requires the launcher (RPC) configured. A mutating route, so
/// it sits behind the bearer gate, but it places no trade.
async fn positions_refresh(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    mint: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    let mint = mint.into_inner();
    let rows = launcher::load_positions(pool.get_ref(), Some(settings), &mint)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

/// Dry-run a management action (post-launch management, Phase 2): freshen positions
/// from the **feed** (zero RPC — so tweaking the sell % and re-previewing never hits
/// the chain), then compute the per-wallet `ActionPlan` WITHOUT placing any trade.
/// The preview is an estimate; `manage/execute` re-reconciles against chain (fatal
/// on a sell) so the executed size is authoritative even if an external transfer
/// left the feed view slightly behind. For an exact preview, hit "Refresh" on the
/// holdings card first. Always allowed (reading + previewing are safe).
async fn manage_preview(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
    body: web::Json<ManageRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let mint = mint.into_inner();
    launcher::read_positions(pool.get_ref(), &mint)
        .await
        .map_err(e500)?;
    let plan = launcher::build_plan(pool.get_ref(), &mint, &body)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(plan))
}

/// Execute a management action (DANGER: places real sells). Requires the launcher
/// to be configured AND `MANAGE_ENABLED=true` (503 otherwise) — the kill switch,
/// mirroring `FUND_ENABLED`. Returns the finalized audit row (per-leg outcomes).
async fn manage_execute(
    pool: web::Data<PgPool>,
    settings: web::Data<Option<LauncherSettings>>,
    mint: web::Path<String>,
    body: web::Json<ManageRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let settings = launcher_settings(&settings)?;
    if settings.manage.is_none() {
        return Ok(HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "token management disabled — set MANAGE_ENABLED=true to enable"
        })));
    }
    let mint = mint.into_inner();
    let action = execute_action(pool.get_ref(), settings, &mint, &body)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(action))
}

#[derive(serde::Deserialize)]
struct ManageActionsQuery {
    #[serde(default = "default_manage_actions_limit")]
    limit: i64,
}
fn default_manage_actions_limit() -> i64 {
    50
}

/// A mint's management-action history (newest first).
async fn manage_actions(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
    q: web::Query<ManageActionsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = ManageActionRepo::by_mint(pool.get_ref(), &mint, q.limit.clamp(1, 200))
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(serde::Deserialize)]
struct ArmLadderBody {
    #[serde(default)]
    selection: WalletSelection,
    rungs: Vec<LadderRung>,
}

/// Arm a take-profit sell ladder (post-launch management, Phase 4). Arming is
/// always allowed (it places no trade); the background evaluator only FIRES a rung
/// when `MANAGE_ENABLED=true` — until then armed ladders simply wait. Rungs are
/// validated here, so a bad rung 400s at arm time.
async fn ladder_arm(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
    body: web::Json<ArmLadderBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let body = body.into_inner();
    let ladder = arm_ladder(pool.get_ref(), &mint, body.selection, body.rungs)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(ladder))
}

/// A mint's ladders (any status, newest first).
async fn ladders_list(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = SellLadderRepo::by_mint(pool.get_ref(), &mint)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

/// Cancel an armed ladder. 404 if it wasn't armed (already fired out or cancelled).
async fn ladder_cancel(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if SellLadderRepo::cancel(pool.get_ref(), id.into_inner())
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("no armed ladder with that id"))
    }
}

#[derive(serde::Deserialize)]
struct StartVolumeBody {
    #[serde(default)]
    selection: WalletSelection,
    config: VolumeConfig,
}

/// Start a volume-making bot (post-launch management, Phase 5). Starting is always
/// allowed (it places no trade); the background scheduler only trades a `running`
/// bot when `MANAGE_ENABLED=true` — until then it sits idle. The config is
/// validated here, so a bad config 400s at start time.
async fn volume_start(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
    body: web::Json<StartVolumeBody>,
) -> Result<HttpResponse, actix_web::Error> {
    let body = body.into_inner();
    let bot = start_volume_bot(pool.get_ref(), &mint, body.selection, body.config)
        .await
        .map_err(e400)?;
    Ok(HttpResponse::Ok().json(bot))
}

/// A mint's volume bots (any status, newest first).
async fn volume_list(
    pool: web::Data<PgPool>,
    mint: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let rows = VolumeBotRepo::by_mint(pool.get_ref(), &mint)
        .await
        .map_err(e500)?;
    Ok(HttpResponse::Ok().json(rows))
}

/// Pause a running bot. 404 if it wasn't running (already paused/stopped).
async fn volume_pause(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if VolumeBotRepo::pause(pool.get_ref(), id.into_inner())
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("no running bot with that id"))
    }
}

/// Resume a paused bot (due immediately). 404 if it wasn't paused.
async fn volume_resume(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if VolumeBotRepo::resume(pool.get_ref(), id.into_inner())
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("no paused bot with that id"))
    }
}

/// Stop a bot (terminal, operator). 404 if it was already stopped.
async fn volume_stop(
    pool: web::Data<PgPool>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, actix_web::Error> {
    if VolumeBotRepo::stop(pool.get_ref(), id.into_inner(), Some("stopped by operator"))
        .await
        .map_err(e500)?
    {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(actix_web::error::ErrorNotFound("no active bot with that id"))
    }
}
