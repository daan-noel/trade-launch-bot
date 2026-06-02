mod analyzers;
mod api;
mod config;
pub use config::constants as constants;
mod ingest;
mod models;
mod services;
mod state;
mod storage;
mod strategies;
mod trader;
mod utils;

use anyhow::Context;
use solana_sdk::signature::Keypair;
use std::sync::Arc;
use tracing::{error, info, warn};

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use crate::trader::{PumpFunTrader, TraderConfig};

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
                .unwrap_or_else(|_| "backend=info,sqlx=warn".into()),
        )
        .init();

    // Config
    let settings = config::Settings::from_env().context("Failed to load configuration")?;

    info!(
        host = %settings.host,
        port = settings.port,
        pump_program = %settings.pump_program_id,
        "Configuration loaded"
    );

    let trader_config = Arc::new(TraderConfig {
        rpc_url: settings.helius_rpc_url.clone(),
        helius_sender_url: settings.helius_sender_url.clone(),
        keypair: parse_wallet_keypair(&settings.wallet_private_key)
            .context("Failed to parse trader wallet private key")?,
        nonce_accounts: settings.nonce_accounts.clone(),
        priority_fee_lamports: settings.compute_unit_price,
        buy_seed_pool_size: settings.buy_seed_pool_size,
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
    let creator_cache = Arc::new(state::creator_cache::CreatorCache::new());

    // Seed caches from DB so historical data is visible immediately
    storage::seed::seed_token_cache(&db, token_cache.clone()).await?;
    storage::seed::seed_creator_cache(&db, creator_cache.clone()).await?;

    // Internal event bus: one sender, many receivers (services, analyzers, API)
    let (event_tx, _) = tokio::sync::broadcast::channel::<models::events::InternalEvent>(2048);

    // AppState — shared with API handlers via web::Data
    let (live_tx, live_rx) = tokio::sync::watch::channel(false);
    let (sol_price_tx, _sol_price_rx) = tokio::sync::watch::channel::<Option<f64>>(None);
    let sol_price = Arc::new(sol_price_tx);
    let app_state = Arc::new(state::AppState::new(
        db.clone(),
        token_cache.clone(),
        creator_cache.clone(),
        event_tx.clone(),
        live_tx.clone(),
        sol_price.clone(),
        trader.clone(),
    ));

    // Raw WS message channel: helius_ws → event_handler
    let (raw_tx, raw_rx) = tokio::sync::mpsc::channel::<String>(1024);

    // Ingest: Helius WebSocket feed
    let ws_settings = Arc::new(settings.clone());
    let ws_task = tokio::spawn(ingest::helius_ws::run(ws_settings, raw_tx, live_rx));

    // Ingest: decode + persist + broadcast
    let event_handler = ingest::EventHandler::new(
        settings.pump_program_id.clone(),
        event_tx.clone(),
        db.clone(),
    );
    let handler_task = tokio::spawn(event_handler.run(raw_rx));

    // Service: token lifecycle — subscribes to the event bus
    let token_service =
        services::TokenService::new(db.clone(), token_cache.clone(), creator_cache.clone());
    let service_task = tokio::spawn(token_service.run(event_tx.subscribe()));

    // Service: strategy handler — subscribes to the event bus
    let trading_service = services::TradingService::new(trader.clone());
    let strategy_service = services::StrategyService::new(db.clone(), trading_service.clone());
    let strategy_task = tokio::spawn(strategy_service.run(event_tx.subscribe()));
    let _trading_service_task = tokio::spawn(async move {
        // TradingService currently acts as a bridge only; keep it alive for
        // potential future event-driven trading tasks.
        tokio::task::yield_now().await;
    });

    // Initialize SOL price cache immediately, then start the poller.
    match services::price_service::fetch_latest_sol_price().await {
        Ok(price) => {
            info!("Initial SOL/USD price: ${price:.2}");
            let _ = sol_price.send(Some(price));
        }
        Err(err) => {
            warn!("Initial SOL/USD price fetch failed: {err}");
        }
    }

    // Service: SOL price polling — updates the in-memory SOL/USD cache
    let price_task = tokio::spawn(services::price_service::run(sol_price.clone()));

    // // Service: analyzers — volume + creator scoring
    // let analyzer_service = analyzers::AnalyzerService::new(
    //     db,
    //     token_cache,
    //     creator_cache,
    // );
    // let analyzer_task = tokio::spawn(analyzer_service.run(event_tx.subscribe()));

    // HTTP server
    let bind_addr = format!("{}:{}", settings.host, settings.port);
    info!(addr = %bind_addr, "Starting HTTP server");
    let http_state = app_state.clone();
    let http_server = HttpServer::new(move || {
        // Read allowed origin from env at worker-spawn time so it can be set
        // per-environment without recompilation.  Defaults to "*" (dev).
        let allowed_origin =
            std::env::var("CORS_ALLOWED_ORIGIN").unwrap_or_else(|_| "*".to_string());

        let cors = if allowed_origin == "*" {
            Cors::default()
                .allow_any_origin()
                .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allowed_header(actix_web::http::header::CONTENT_TYPE)
                .allowed_header(actix_web::http::header::ACCEPT)
                .max_age(3600)
        } else {
            Cors::default()
                .allowed_origin(&allowed_origin)
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
    .workers(4)
    .bind(&bind_addr)
    .context("Failed to bind HTTP server")?;
    let server_task = tokio::spawn(http_server.run());

    info!("System running — waiting for events");

    // Run until a critical task exits
    tokio::select! {
        _ = ws_task       => error!("WS task exited unexpectedly"),
        _ = handler_task  => error!("Event handler task exited unexpectedly"),
        _ = service_task  => error!("Token service task exited unexpectedly"),
        _ = strategy_task => error!("Strategy service task exited unexpectedly"),
        _ = price_task    => error!("Price service task exited unexpectedly"),
        // _ = analyzer_task => error!("Analyzer service task exited unexpectedly"),
        res = server_task => {
            match res {
                Ok(Ok(())) => info!("HTTP server stopped"),
                Ok(Err(e)) => error!("HTTP server error: {e}"),
                Err(e) => error!("HTTP server task panicked: {e}"),
            }
        }
    }

    Ok(())
}
