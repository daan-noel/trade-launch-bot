//! `lab` — the analysis-box binary. Composition root for the local
//! stack: no trader, no ingest, no live strategy runner. It connects the Postgres
//! pools (the corpus), serves the core read routes + the local routes (rule
//! authoring, backtests, grouped sweeps), and runs the SOL-price
//! poller + token-list refresh. It boots with **no** trading keys and **no**
//! HELIUS gRPC: it loads the shared `Settings::from_env` + `FeeTuning::from_env`
//! (same tip/CU knobs as live, for CostModel) and simply leaves the optional
//! Helius endpoints empty and never loads `live::config::TradingSecrets`.

use lab::{api, state, storage, sweep};

use trading_core::config::{self, constants};
use trading_core::{models, services};

use anyhow::Context;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpServer};

// Bearer-auth gate (`ApiAuth` + `require_bearer_auth`) is the shared, fail-closed
// `http_auth` middleware — one SSOT copy across hunter live/lab + forge live.
use http_auth::{require_bearer_auth, ApiAuth};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Anchor relative `.env` paths (`EVENT_LOG_DIR`) to the `.env`'s directory so
    // the replay inspector reads the exact dir the live recorder writes, whatever
    // CWD either bin started in — see `config::env_paths`.
    config::install_dotenv_anchor(dotenvy::dotenv().ok());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                // The bin crate is `hunter_lab` (bin target `hunter-lab`), where all
                // of main.rs's startup logs live; `lab` is the lib crate. Enable both
                // (plus the shared `trading_core` lib) or the process runs silently and
                // looks idle.
                .unwrap_or_else(|_| {
                    "hunter_lab=info,lab=info,trading_core=info,sqlx=error".into()
                }),
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

    // `lab reroll-run <uuid>...` — recompute those runs' `strategy_run_metrics`
    // from their current positions, then exit. Metrics are normally written once,
    // when a run is finalized; this is the manual lever for the case where a
    // finalized run's membership changed afterwards (a position reattributed
    // between runs, a straggler settled while the process was down). It goes
    // through `StrategyRepo::roll_up_run` — i.e. the same `exact_run_metrics`
    // kernel a live finalize uses — so a hand-repaired run stays comparable with
    // every other run and with a backtest. Never recompute these in SQL.
    if std::env::args().nth(1).as_deref() == Some("reroll-run") {
        let ids: Vec<String> = std::env::args().skip(2).filter(|a| !a.starts_with("--")).collect();
        return run_reroll(&ids).await;
    }

    let settings = config::Settings::from_env().context("Failed to load configuration")?;
    // Same tip/CU knobs live applies to the trader — CostModel's fixed per-leg
    // cost reads them via FeeTuning::current().
    let fee_tuning = config::FeeTuning::from_env()
        .context("Failed to load fee/tip tuning for CostModel")?;
    fee_tuning.clone().install();
    // HTTP bind — lab reads its own LAB_HOST/LAB_PORT (docker's HOST/PORT wins),
    // defaulting to :8140 (the deploy LAB_API_PORT) so it never collides with the
    // live bin's :8130 and local + docker use the same port.
    let http_host = config::resolve_host("LAB_HOST");
    let http_port = config::resolve_port("LAB_PORT", 8140)?;
    info!(
        host = %http_host,
        port = http_port,
        jito_min_tip_sol = fee_tuning.jito_min_tip_sol,
        cu_price_micro_lamports = fee_tuning.cu_price_micro_lamports,
        "Local (analysis) configuration loaded"
    );

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
    // token universe held in RAM. Lab (workstation, big RAM) loads the FULL set —
    // uncapped-but-bounded by `LAB_TOKEN_LIST_LIMIT` over a wide window — so its
    // filter/sort/page runs in memory at analysis speed. Fire-and-forget.
    tokio::spawn(state::token_list_cache::run_token_list_db_refresh(
        core_state.token_repo(),
        core_state.token_list.clone(),
        trading_core::config::constants::LAB_TOKEN_LIST_LIMIT,
        trading_core::config::constants::LAB_TOKEN_LIST_WINDOW_DAYS,
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
        let bind_addr = format!("{http_host}:{http_port}");
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
                .app_data(trading_core::api::json_error_config())
                .app_data(trading_core::api::query_error_config())
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
    let settings = config::Settings::from_env().context("Failed to load configuration")?;
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

/// `lab reroll-run <uuid>...`: re-roll each run's metrics row from its current
/// positions and exit. Batch job — no HTTP, no pollers. Idempotent (the upsert
/// only advances a row with an older `rolled_up_at`), so re-running is harmless.
async fn run_reroll(ids: &[String]) -> anyhow::Result<()> {
    if ids.is_empty() {
        anyhow::bail!("usage: hunter-lab reroll-run <run-uuid> [<run-uuid>...]");
    }
    let settings = config::Settings::from_env().context("Failed to load configuration")?;
    // The cost model's fixed per-leg cost is env-derived; a rollup prices PnL
    // through it, so install the same tuning a live finalize would have used.
    config::FeeTuning::from_env()
        .context("Failed to load fee/tip tuning for CostModel")?
        .install();
    let storage::postgres::DbPools { batch: batch_db, .. } =
        storage::postgres::connect(&settings).await?;
    let repo = storage::repositories::strategy_repo::StrategyRepo::new(batch_db);

    let force = std::env::args().any(|a| a == "--force");
    for raw in ids {
        let id: uuid::Uuid = raw.parse().with_context(|| format!("not a uuid: {raw}"))?;
        // Metrics are finalize-time by contract: the run navigator reads "a
        // `strategy_run_metrics` row exists" as "this run is over" (`has_metrics`).
        // Writing one for a live run would show a half-finished activation as a
        // settled result — and it would be wrong again on the next fill anyway.
        match repo.find_run(id).await? {
            None => anyhow::bail!("no such run: {id}"),
            Some(run) if run.status == "Running" && !force => {
                println!("{id}: skipped — run is still Running (metrics are written at finalize; pass --force to override)");
                continue;
            }
            Some(_) => {}
        }
        let r = repo
            .roll_up_run(id)
            .await
            .with_context(|| format!("reroll {id}"))?;
        println!(
            "{id}: fired={} closed={} open={} win_rate={:.3} pnl_sol={:.4} unsettled={}",
            r.metrics.n_fired,
            r.metrics.n_closed,
            r.metrics.n_open,
            r.metrics.win_rate,
            r.metrics.total_pnl_sol,
            r.unsettled,
        );
    }
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
