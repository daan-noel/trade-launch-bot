mod api;
mod config;
pub use config::constants as constants;
mod ingest;
mod models;
mod analyzers;
mod state;
mod storage;
mod services;
mod strategies;
mod trader;

use anyhow::Context;
use solana_sdk::signature::Keypair;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use crate::trader::{PumpFunTrader, TraderConfig};

/// CORS allow-origin for the HTTP API. "*" allows any origin; tighten to your
/// frontend origin for production.
const CORS_ALLOWED_ORIGIN: &str = "*";

fn parse_wallet_keypair(base58_key: &str) -> anyhow::Result<Keypair> {
    let bytes = bs58::decode(base58_key)
        .into_vec()
        .context("Failed to decode WALLET_PRIVATE_KEY as base58")?;
    Keypair::from_bytes(&bytes)
        .context("Failed to construct Keypair from WALLET_PRIVATE_KEY bytes")
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Load .env before anything else
    dotenvy::dotenv().ok();

    // Tracing / logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=info,sqlx=error".into()),
        )
        .init();

    // Config
    let settings = config::Settings::from_env().context("Failed to load configuration")?;

    info!(
        host = %settings.host,
        port = settings.port,
        pump_program = constants::PUMP_FUN_PROGRAM_ID,
        "Configuration loaded"
    );

    let trader_config = Arc::new(TraderConfig {
        rpc_url: settings.helius_rpc_url.clone(),
        helius_sender_url: settings.helius_sender_url.clone(),
        keypair: parse_wallet_keypair(&settings.wallet_private_key)
            .context("Failed to parse trader wallet private key")?,
        nonce_accounts: settings.nonce_accounts.clone(),
    });

    let mut trader = PumpFunTrader::new(trader_config);
    trader
        .initialize()
        .await
        .context("Failed to initialize PumpFunTrader")?;
    let trader = Arc::new(trader);

    // Database — connect and run migrations
    let db = storage::postgres::connect(&settings).await?;

    // In-memory caches (shared between services and future API handlers)
    let token_cache = Arc::new(state::token_cache::TokenCache::new());

    storage::seed::seed_token_cache(&db, token_cache.clone()).await?;

    let (sse_tx, _) = tokio::sync::broadcast::channel::<models::ingest::SseEvent>(512);

    let (live_tx, live_rx) = tokio::sync::watch::channel(false);

    // Load the persisted settings document and hold it in a watch channel as the
    // in-memory source of truth, so a policy set in a previous run is in force
    // before the first event arrives.
    let app_settings = storage::repositories::settings_repo::SettingsRepo::new(db.clone())
        .get()
        .await
        .context("Failed to load app settings")?;
    info!(
        track_mayhem = app_settings.track_mayhem,
        track_post_migration = app_settings.track_post_migration,
        "Settings loaded"
    );
    let (settings_tx, _) = tokio::sync::watch::channel(app_settings);

    let (sol_price_tx, _sol_price_rx) = tokio::sync::watch::channel::<Option<f64>>(None);
    let sol_price = Arc::new(sol_price_tx);
    let tpsl_cache = Arc::new(strategies::tpsl_sniper_1::TpslRuntimeCache::new());
    tpsl_cache
        .load_from_db(&db)
        .await
        .context("Failed to load TPSL runtime cache")?;

    let (db_tx, db_rx, strategy_tx, strategy_rx) = ingest::IngestPipeline::channel_pair();

    let (raw_tx, raw_rx) = tokio::sync::mpsc::channel::<String>(1024);

    // Built before AppState so its pool→mint index and migration signal can be
    // shared with the HTTP handlers — a token sync registers a migrated token's
    // pool here so the live WS subscribes to it immediately.
    let pipeline = ingest::IngestPipeline::new(
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        token_cache.clone(),
        db_tx,
        strategy_tx,
        sse_tx.clone(),
        settings_tx.subscribe(),
        trader.clone(),
    );

    let app_state = Arc::new(state::AppState::new(
        db.clone(),
        settings.helius_rpc_url.clone(),
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        token_cache.clone(),
        sse_tx.clone(),
        live_tx.clone(),
        settings_tx.clone(),
        sol_price.clone(),
        trader.clone(),
        tpsl_cache.clone(),
        pipeline.pool_index(),
        pipeline.pools_changed(),
    ));

    // The WS task drives its PumpSwap pool subscriptions from the pipeline's
    // pool→mint index (seeded with migrated tokens) and re-subscribes, via the
    // `pools_changed` signal, as new tokens migrate.
    let ws_settings = Arc::new(settings.clone());
    let ws_task = tokio::spawn(ingest::helius_ws::run(
        ws_settings,
        raw_tx,
        live_rx,
        pipeline.pool_index(),
        pipeline.pools_changed(),
    ));

    // Revival sweep: re-subscribe pools of migrated tokens that become active
    // again (e.g. after a manual sync), so pruned-as-quiet pools aren't blind.
    tokio::spawn(ingest::run_pool_subscription_refresh(
        token_cache.clone(),
        pipeline.pool_index(),
        pipeline.pools_changed(),
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        settings_tx.subscribe(),
    ));

    let pipeline_task = tokio::spawn(pipeline.run(raw_rx));

    let db_writer = ingest::DbWriter::new(db.clone());
    let db_writer_task = tokio::spawn(db_writer.run(db_rx));

    let strategy_runner = strategies::StrategyRunner::new(
        db.clone(),
        trader.clone(),
        token_cache.clone(),
        tpsl_cache,
    );
    let strategy_task = tokio::spawn(strategy_runner.run(strategy_rx));

    // Initialize SOL price cache immediately, then start the poller.
    match services::sol_price::fetch_latest_sol_price().await {
        Ok(price) => {
            info!("Initial SOL/USD price: ${price:.2}");
            let _ = sol_price.send(Some(price));
        }
        Err(err) => {
            warn!("Initial SOL/USD price fetch failed: {err}");
        }
    }

    // Service: SOL price polling — updates the in-memory SOL/USD cache
    let price_task = tokio::spawn(services::sol_price::run_poller(sol_price.clone()));

    let server_task = if settings.http_enabled {
        let bind_addr = format!("{}:{}", settings.host, settings.port);
        info!(addr = %bind_addr, workers = settings.http_workers, "Starting HTTP server");
        let http_state = app_state.clone();
        let http_workers = settings.http_workers;
        let http_server = HttpServer::new(move || {
            let allowed_origin = CORS_ALLOWED_ORIGIN;

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
                .wrap(cors)
                .app_data(web::Data::new(http_state.clone()))
                .configure(api::configure)
        })
        .workers(http_workers)
        .bind(&bind_addr)
        .context("Failed to bind HTTP server")?;
        Some(tokio::spawn(http_server.run()))
    } else {
        info!("HTTP disabled (HTTP_ENABLED=false) — bot-only mode");
        None
    };

    info!("System running — waiting for events");

    tokio::select! {
        _ = ws_task       => error!("WS task exited unexpectedly"),
        _ = pipeline_task  => error!("Ingest pipeline exited unexpectedly"),
        _ = db_writer_task  => error!("DbWriter task exited unexpectedly"),
        _ = strategy_task => error!("Strategy runner exited unexpectedly"),
        _ = price_task    => error!("SOL price poller exited unexpectedly"),
        res = async {
            match server_task {
                Some(task) => task.await,
                None => std::future::pending().await,
            }
        } => {
            match res {
                Ok(Ok(())) => info!("HTTP server stopped"),
                Ok(Err(e)) => error!("HTTP server error: {e}"),
                Err(e) => error!("HTTP server task panicked: {e}"),
            }
        }
    }

    Ok(())
}
