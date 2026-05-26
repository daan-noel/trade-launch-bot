mod analyzers;
mod api;
mod config;
mod ingest;
mod models;
mod services;
mod state;
mod storage;
mod strategies;

use anyhow::Context;
use std::sync::Arc;
use tracing::{error, info};

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env before anything else
    dotenvy::dotenv().ok();

    // Tracing / logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,info".into()),
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
    let app_state = Arc::new(state::AppState::new(
        db.clone(),
        token_cache.clone(),
        creator_cache.clone(),
        event_tx.clone(),
        live_tx.clone(),
    ));

    // Raw WS message channel: helius_ws → event_handler
    let (raw_tx, raw_rx) = tokio::sync::mpsc::channel::<String>(1024);

    // Ingest: Helius WebSocket feed
    let ws_settings = Arc::new(settings.clone());
    let ws_task = tokio::spawn(ingest::helius_ws::run(
        ws_settings,
        raw_tx,
        live_rx,
    ));

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
    let strategy_service = services::StrategyService::new(db.clone());
    let strategy_task = tokio::spawn(strategy_service.run(event_tx.subscribe()));

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
