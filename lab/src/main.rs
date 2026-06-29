//! `lab` — the analysis-box binary. Composition root for the local
//! stack: no trader, no ingest, no live strategy runner. It connects the Postgres
//! pools (the corpus), serves the core read routes + the local routes (rule
//! authoring, backtests, grouped sweeps, swing detection), and runs the SOL-price
//! poller + token-list refresh. It boots with **no** trading keys and **no**
//! HELIUS gRPC — see `Settings::from_env_local`.

use lab::{api, state, storage, sweep};

use trading_core::config::{self, constants};
use trading_core::{models, services};

use anyhow::Context;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::{from_fn, Next};
use actix_web::{web, App, HttpResponse, HttpServer};

/// Optional bearer token gating mutating API requests, shared via `app_data`.
#[derive(Clone)]
struct ApiAuth {
    token: Option<String>,
}

/// Middleware: require `Authorization: Bearer <token>` on mutating requests.
/// Preflight (OPTIONS) and safe reads (GET/HEAD) always pass; the check is
/// fail-closed (a mutating request with no token configured is rejected).
/// `API_AUTH_TOKEN` is required by `Settings::from_env_local`, so the `None` arm
/// is only reachable if the server is started without it.
async fn require_bearer_auth(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, actix_web::Error> {
    use actix_web::http::{header::AUTHORIZATION, Method};

    let mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );
    let configured = req
        .app_data::<web::Data<ApiAuth>>()
        .and_then(|a| a.token.clone());

    let authorized = match (mutating, configured) {
        (false, _) => true,
        (true, Some(expected)) => req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false),
        (true, None) => false,
    };

    if authorized {
        Ok(next.call(req).await?.map_into_left_body())
    } else {
        let resp = HttpResponse::Unauthorized()
            .json(serde_json::json!({ "error": "unauthorized" }));
        Ok(req.into_response(resp).map_into_right_body())
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lab=info,sqlx=error".into()),
        )
        .init();

    // `lab lake-export` — batch job (data-pipeline hop 2): export newly-sealed days
    // from local PG into the Parquet lake, then exit. Runs before any server wiring
    // (mirrors `live -- probe`). All other args fall through to the HTTP server.
    if std::env::args().nth(1).as_deref() == Some("lake-export") {
        // `lab lake-export --include-today` also exports today's still-open UTC day
        // (force-overwriting its non-immutable file). Since the lake is now the sole
        // sweep corpus source, this is the only way to sweep *current-day* data — the
        // default sealed-only export never includes the open day. Off by default so a
        // plain `lake-export` keeps the lake to immutable, settled days.
        let include_today = std::env::args().any(|a| a == "--include-today");
        return run_lake_export(include_today).await;
    }

    let settings = config::Settings::from_env_local().context("Failed to load configuration")?;
    info!(host = %settings.host, port = settings.port, "Local (analysis) configuration loaded");

    // Database — the three workload-isolated pools + migrations. `api_db` backs the
    // fast handlers, `batch_db` the heavy sweep/backtest jobs; `db` (hot) backs the
    // boot reconcile + settings load + token-list refresh (no ingest/strategy here).
    let storage::postgres::DbPools {
        hot: db,
        api: api_db,
        batch: batch_db,
    } = storage::postgres::connect(&settings).await?;

    // Lab-only schema: grouped-sweep result tables (and future analysis-only
    // tables) live in the lab-owned migration set, never on EC2/live. Must run
    // before the grouped-sweep reconcile below, which reads those tables.
    storage::lab_migrations::run(&db)
        .await
        .context("lab migrations failed")?;

    // Crash recovery: a killed process can leave a grouped sweep stuck at
    // `status = 'running'`. None can be live at boot (single-flight gate), so any
    // `running` run is an orphan — mark it `cancelled`. Best-effort.
    for strategy_id in sweep::registry::strategy_ids() {
        if let Some(tables) = sweep::registry::tables_for(strategy_id) {
            match storage::repositories::grouped_sweep_repo::GroupedSweepRepo::new(db.clone(), tables)
                .reconcile_orphaned_runs()
                .await
            {
                Ok(0) => {}
                Ok(n) => warn!("grouped sweep: marked {n} orphaned '{strategy_id}' run(s) cancelled"),
                Err(e) => error!("grouped sweep: orphaned-run reconcile for '{strategy_id}' failed: {e}"),
            }
        }
    }

    let (sse_tx, _) = tokio::sync::broadcast::channel::<models::ingest::SseEvent>(512);

    // Load persisted settings into a watch channel (the in-memory source of truth).
    let settings_repo = storage::repositories::settings_repo::SettingsRepo::new(db.clone());
    let app_settings = settings_repo
        .load_all()
        .await
        .context("Failed to load app settings")?;
    let (settings_tx, _) = tokio::sync::watch::channel(app_settings);

    let (sol_price_tx, _sol_price_rx) = tokio::sync::watch::channel::<Option<f64>>(None);
    let sol_price = Arc::new(sol_price_tx);

    // In-memory token cache (empty: no live ingest feeds it locally). The token
    // list is served from the DB base, kept fresh by the refresh task below.
    let token_cache = Arc::new(state::token_cache::TokenCache::new());

    let core_state = Arc::new(state::core_state::CoreState::new(
        api_db,
        batch_db,
        settings.helius_rpc_url.clone(),
        settings.helius_laserstream_url.clone(),
        settings.helius_api_key.clone(),
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        token_cache.clone(),
        sse_tx.clone(),
        settings_tx.clone(),
        sol_price.clone(),
    ));
    let local_state = Arc::new(state::local_state::LocalState::new(core_state.clone()));

    // Keep the token-list DB base fresh so `GET /api/tokens` reflects the whole
    // seeded universe (tokens + persisted stats). Fire-and-forget.
    tokio::spawn(state::token_list_cache::run_token_list_db_refresh(
        core_state.token_repo(),
        core_state.token_list.clone(),
    ));

    // SOL/USD price: prime the cache, then poll.
    match services::sol_price::fetch_latest_sol_price().await {
        Ok(price) => {
            info!("Initial SOL/USD price: ${price:.2}");
            let _ = sol_price.send(Some(price));
        }
        Err(err) => warn!("Initial SOL/USD price fetch failed: {err}"),
    }
    let price_task = tokio::spawn(services::sol_price::run_poller(sol_price.clone()));

    let server_task = if settings.http_enabled {
        let bind_addr = format!("{}:{}", settings.host, settings.port);
        info!(addr = %bind_addr, workers = settings.http_workers, "Starting HTTP server");
        // Render each SSE event to wire bytes once and fan the shared frame out to
        // all connections instead of re-rendering per subscriber.
        tokio::spawn(
            trading_core::api::handlers::system::run_sse_render_bridge(core_state.clone()),
        );
        let http_core = core_state.clone();
        let http_local = local_state.clone();
        let http_workers = settings.http_workers;
        let cors_allowed_origin = settings.cors_allowed_origin.clone();
        let api_auth = ApiAuth {
            token: settings.api_auth_token.clone(),
        };
        info!("API auth enabled (fail-closed): mutating requests require a bearer token");
        let http_server = HttpServer::new(move || {
            let allowed_origin = cors_allowed_origin.as_str();
            let cors = if allowed_origin == "*" {
                Cors::default()
                    .allow_any_origin()
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                    .allowed_header(actix_web::http::header::CONTENT_TYPE)
                    .allowed_header(actix_web::http::header::ACCEPT)
                    .max_age(3600)
            } else {
                Cors::default()
                    .allowed_origin(allowed_origin)
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                    .allowed_header(actix_web::http::header::CONTENT_TYPE)
                    .allowed_header(actix_web::http::header::ACCEPT)
                    .max_age(3600)
            };

            App::new()
                .wrap(from_fn(require_bearer_auth))
                .wrap(cors)
                .app_data(web::Data::new(http_core.clone()))
                .app_data(web::Data::new(http_local.clone()))
                .app_data(web::Data::new(api_auth.clone()))
                .configure(trading_core::api::configure_core_routes)
                .configure(api::configure_local_routes)
        })
        .workers(http_workers)
        .bind(&bind_addr)
        .context("Failed to bind HTTP server")?;
        Some(tokio::spawn(http_server.run()))
    } else {
        info!("HTTP disabled (HTTP_ENABLED=false)");
        None
    };

    info!("Local backend running");

    // Both long-lived tasks should run for the process lifetime, so either
    // resolving is a fault — surface it as a non-zero exit for the supervisor.
    let outcome: anyhow::Result<()> = tokio::select! {
        res = price_task => Err(task_fault("SOL price poller", res)),
        res = async {
            match server_task {
                Some(task) => task.await,
                None => std::future::pending().await,
            }
        } => match res {
            Ok(Ok(())) => {
                info!("HTTP server stopped");
                Ok(())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("HTTP server error: {e}")),
            Err(e) => Err(anyhow::anyhow!("HTTP server task panicked: {e}")),
        },
    };

    if let Err(ref e) = outcome {
        error!("Fatal: {e} — exiting non-zero so the supervisor restarts the process");
    }
    outcome
}

/// `lab lake-export`: connect the batch pool, export sealed days into the lake, exit.
/// Reads `DATABASE_URL` (via `Settings`) for local PG and `SWEEP_LAKE_DIR` for the
/// lake root. A batch job — no HTTP, no pollers.
async fn run_lake_export(include_today: bool) -> anyhow::Result<()> {
    let settings = config::Settings::from_env_local().context("Failed to load configuration")?;
    let storage::postgres::DbPools { batch: batch_db, .. } =
        storage::postgres::connect(&settings).await?;

    let root = lab::lake::lake_root();
    info!(lake = %root.display(), include_today, "lake-export: starting");
    let summary = lab::lake::export::export_lake(&batch_db, &root, include_today).await?;
    info!(
        days_written = summary.days_written.len(),
        days_skipped = summary.days_skipped,
        tokens = summary.tokens_written,
        "lake-export: done"
    );
    println!(
        "lake-export complete: {} day(s) written, {} skipped, {} token rows -> {}",
        summary.days_written.len(),
        summary.days_skipped,
        summary.tokens_written,
        root.display()
    );
    Ok(())
}

/// Classify a long-lived task's `JoinHandle` result as a fatal error.
fn task_fault<T>(name: &str, res: Result<T, tokio::task::JoinError>) -> anyhow::Error {
    match res {
        Ok(_) => anyhow::anyhow!("{name} task exited unexpectedly"),
        Err(e) if e.is_panic() => anyhow::anyhow!("{name} task panicked: {e}"),
        Err(e) => anyhow::anyhow!("{name} task aborted: {e}"),
    }
}
