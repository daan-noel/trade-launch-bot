mod api;
mod config;
pub use config::constants as constants;
mod ingest_laserstream;
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
use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::{from_fn, Next};
use actix_web::{web, App, HttpResponse, HttpServer};
use crate::trader::{PumpFunTrader, TraderConfig};

/// Optional bearer token gating mutating API requests, shared via `app_data`.
/// `None` disables the check entirely (the default, current behaviour).
#[derive(Clone)]
struct ApiAuth {
    token: Option<String>,
}

/// Middleware: require `Authorization: Bearer <token>` on mutating requests when
/// an `API_AUTH_TOKEN` is configured. Preflight (OPTIONS) and safe reads
/// (GET/HEAD) always pass, and when no token is configured every request passes
/// — so enabling the env var is the only thing that turns auth on.
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
        // Non-mutating, or no token configured → always allowed.
        (false, _) | (_, None) => true,
        (true, Some(expected)) => req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false),
    };

    if authorized {
        Ok(next.call(req).await?.map_into_left_body())
    } else {
        let resp = HttpResponse::Unauthorized()
            .json(serde_json::json!({ "error": "unauthorized" }));
        Ok(req.into_response(resp).map_into_right_body())
    }
}

fn parse_wallet_keypair(base58_key: &str) -> anyhow::Result<Keypair> {
    let bytes = bs58::decode(base58_key)
        .into_vec()
        .context("Failed to decode WALLET_PRIVATE_KEY as base58")?;
    Keypair::from_bytes(&bytes)
        .context("Failed to construct Keypair from WALLET_PRIVATE_KEY bytes")
}

/// One-shot, no-/low-SOL probes for the tx-latency changes. Runs after the
/// trader is initialized (so the tip cache and nonce slots are warm) and exits
/// before any DB/ingest/HTTP startup. Subcommands:
///   probe ladder [levels]                      — Jito tip escalation ladder
///                                                (read-only, zero SOL)
///   probe fanout [lamports] [--tip] [--confirm] — fan-out self-transfer to all
///                                                sender endpoints (base fee only)
///   probe simulate-sell <mint> <amount> [--cashback]
///                                              — simulate a real curve sell
///                                                (zero SOL)
async fn run_probe(trader: &PumpFunTrader, args: Vec<String>) -> anyhow::Result<()> {
    const LPS: f64 = pump_trader::constants::LAMPORTS_PER_SOL as f64;
    match args.first().map(String::as_str).unwrap_or("") {
        "ladder" => {
            let levels: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
            let ladder = trader.probe_tip_ladder(levels).await?;
            println!("Jito tip escalation ladder (live tip-floor):");
            for (lvl, lamports) in ladder {
                println!(
                    "  level {lvl}: {lamports:>9} lamports  ({:.6} SOL)",
                    lamports as f64 / LPS
                );
            }
        }
        "fanout" => {
            let lamports: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            let include_tip = args.iter().any(|a| a == "--tip");
            let do_confirm = args.iter().any(|a| a == "--confirm");
            println!(
                "Fan-out self-transfer: {lamports} lamports, tip={include_tip}, confirm={do_confirm}"
            );
            let report = trader
                .probe_fanout_self_transfer(lamports, include_tip, do_confirm)
                .await?;
            for r in &report.results {
                match &r.outcome {
                    Ok(sig) => println!("  ✅ {:>5}ms  {}  -> {sig}", r.elapsed_ms, r.url),
                    Err(e) => println!("  ❌ {:>5}ms  {}  -> {e}", r.elapsed_ms, r.url),
                }
            }
            match (report.confirm_ms, &report.confirmed) {
                (Some(ms), Some(Ok(()))) => println!("  confirmed in {ms}ms"),
                (Some(ms), Some(Err(e))) => println!("  confirm failed in {ms}ms: {e}"),
                _ => {}
            }
        }
        "holdings" => {
            let holdings = trader.get_all_token_accounts().await?;
            if holdings.is_empty() {
                println!("Wallet holds no token accounts.");
            } else {
                println!("Wallet holdings ({}):", holdings.len());
                for h in holdings {
                    println!(
                        "  {}  {} raw ({} UI)  acct={}",
                        h.mint, h.amount, h.ui_amount, h.token_account
                    );
                }
            }
        }
        "simulate-sell" => {
            let mint = args
                .get(1)
                .context("usage: probe simulate-sell <mint> [amount] [--cashback]")?;
            // Amount (raw base units) is optional: default to the wallet's full
            // on-chain balance of the mint so the sim mirrors a real full exit.
            let amount: u64 = match args.get(2).filter(|s| !s.starts_with("--")) {
                Some(s) => s.parse().context("amount must be a u64 (raw base units)")?,
                None => {
                    let bal = trader.get_token_balance(&trader.wallet_pubkey(), mint).await?;
                    println!(
                        "No amount given — using on-chain balance: {} raw ({} UI)",
                        bal.amount, bal.ui_amount
                    );
                    if bal.amount == 0 {
                        anyhow::bail!("wallet holds 0 of {mint} — nothing to simulate");
                    }
                    bal.amount
                }
            };
            let is_cashback = args.iter().any(|a| a == "--cashback");
            let report = trader
                .probe_simulate_curve_sell(mint, amount, is_cashback, 0)
                .await?;
            match &report.err {
                None => println!(
                    "✅ simulation passed — CU consumed: {:?}",
                    report.units_consumed
                ),
                Some(e) => println!(
                    "❌ simulation reverted: {e}\n   CU consumed: {:?}",
                    report.units_consumed
                ),
            }
            println!("--- logs ---");
            for line in &report.logs {
                println!("  {line}");
            }
        }
        "cashback-status" => {
            let pots = trader.cashback_status().await?;
            println!("Cashback status (read-only):");
            let mut total_wsol = 0u64;
            for p in &pots {
                let claimable = p.claimable();
                total_wsol += claimable;
                println!(
                    "  [{}] uva={} exists={}",
                    p.label, p.pda, p.exists
                );
                println!(
                    "      cashback: earned={} claimed={} -> claimable={} lamports ({:.6} SOL)",
                    p.cashback_earned,
                    p.total_cashback_claimed,
                    claimable,
                    claimable as f64 / LPS
                );
                let stable = p.stable_claimable();
                if p.stable_earned != 0 || stable != 0 {
                    println!(
                        "      stable:   earned={} claimed={} -> claimable={} (raw stable-mint units)",
                        p.stable_earned, p.stable_claimed, stable
                    );
                }
            }
            println!(
                "  TOTAL claimable WSOL cashback: {total_wsol} lamports ({:.6} SOL)",
                total_wsol as f64 / LPS
            );
        }
        "claim-cashback" => {
            let execute = args.iter().any(|a| a == "--execute");
            println!(
                "Cashback claim ({}):",
                if execute { "EXECUTE — sending" } else { "simulate-only" }
            );
            let outcomes = trader.claim_cashback(execute).await?;
            if outcomes.is_empty() {
                println!("  Nothing claimable in either pot — nothing to do.");
            }
            for o in &outcomes {
                if o.is_stable {
                    println!(
                        "  [{}] claimable={} raw stable-mint units (claimed as an SPL balance)",
                        o.label, o.claimable
                    );
                } else {
                    println!(
                        "  [{}] claimable={} lamports ({:.6} SOL)",
                        o.label,
                        o.claimable,
                        o.claimable as f64 / LPS
                    );
                }
                match (&o.err, &o.signature, o.simulated) {
                    (None, _, true) => println!(
                        "      ✅ simulation passed — CU consumed: {:?}",
                        o.units_consumed
                    ),
                    (Some(e), _, true) => println!("      ❌ simulation reverted: {e}"),
                    (None, Some(sig), false) => println!("      ✅ sent — sig: {sig}"),
                    (Some(e), _, false) => println!("      ❌ send failed: {e}"),
                    (None, None, false) => println!("      ⚠️  sent but no signature returned"),
                }
                if o.simulated && !o.logs.is_empty() {
                    println!("      --- logs ---");
                    for line in &o.logs {
                        println!("        {line}");
                    }
                }
            }
        }
        other => anyhow::bail!(
            "unknown probe '{other}'. Use: ladder | fanout | simulate-sell | holdings | \
             cashback-status | claim-cashback [--execute]"
        ),
    }
    Ok(())
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
        helius_sender_urls: settings.helius_sender_urls.clone(),
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

    // Probe mode: `cargo run -p backend -- probe <subcommand>` runs a one-shot,
    // no-/low-SOL validation of the latency changes against live infra, then
    // exits before touching the DB / ingest / HTTP server. See `run_probe`.
    if std::env::args().nth(1).as_deref() == Some("probe") {
        return run_probe(&trader, std::env::args().skip(2).collect()).await;
    }

    // Database — connect and run migrations
    let db = storage::postgres::connect(&settings).await?;

    // In-memory caches (shared between services and future API handlers)
    let token_cache = Arc::new(state::token_cache::TokenCache::new());

    storage::seed::seed_token_cache(&db, token_cache.clone()).await?;

    let (sse_tx, _) = tokio::sync::broadcast::channel::<models::ingest::SseEvent>(512);

    // Load the persisted settings document and hold it in a watch channel as the
    // in-memory source of truth, so a policy set in a previous run is in force
    // before the first event arrives.
    let app_settings = storage::repositories::settings_repo::SettingsRepo::new(db.clone())
        .load_all()
        .await
        .context("Failed to load app settings")?;
    info!(
        track_mayhem = app_settings.track_mayhem,
        track_post_migration = app_settings.track_post_migration,
        live = app_settings.live,
        "Settings loaded"
    );

    // Seed live mode from the persisted toggle so a restart resumes the
    // operator's last on/off choice instead of always booting paused.
    let (live_tx, live_rx) = tokio::sync::watch::channel(app_settings.live);

    let (settings_tx, _) = tokio::sync::watch::channel(app_settings);

    let (sol_price_tx, _sol_price_rx) = tokio::sync::watch::channel::<Option<f64>>(None);
    let sol_price = Arc::new(sol_price_tx);
    let tpsl1_cache = Arc::new(strategies::tpsl_sniper_1::Tpsl1RuntimeCache::new(sse_tx.clone()));
    tpsl1_cache
        .load_from_db(&db)
        .await
        .context("Failed to load TPSL1 runtime cache")?;
    let tpsl2_cache = Arc::new(strategies::tpsl_sniper_2::Tpsl2RuntimeCache::new(sse_tx.clone()));
    tpsl2_cache
        .load_from_db(&db)
        .await
        .context("Failed to load TPSL2 runtime cache")?;

    // Shared (wallet, mint) wakeup hub: the DbWriter signals it once a trade is
    // persisted; the live buy/sell confirm loops await it instead of polling the
    // DB on a fixed timer (they keep their timeout as a fallback).
    let trade_signals = Arc::new(state::trade_signals::TradeSignals::new());

    // ── Ingest transport: LaserStream (Yellowstone gRPC) ──
    // Feeds the shared token cache / strategy / SSE / DB. Yields the pool→mint
    // index + migration signal (for AppState), the strategy receiver, and the
    // long-lived task handles the supervising select watches.
    let (pool_index, pools_changed, strategy_rx, producer_task, pipeline_task, db_writer_task) = {
        info!("Ingest transport: LaserStream (gRPC)");
        let (db_tx, db_rx, strategy_tx, strategy_rx) =
            ingest_laserstream::pipeline::IngestPipeline::channel_pair();
        let (value_tx, value_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(1024);

        let pipeline = ingest_laserstream::pipeline::IngestPipeline::new(
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            token_cache.clone(),
            db_tx,
            strategy_tx,
            sse_tx.clone(),
            settings_tx.subscribe(),
            trader.clone(),
        );
        let pool_index = pipeline.pool_index();
        let pools_changed = pipeline.pools_changed();

        let producer_task = tokio::spawn(ingest_laserstream::client::run(
            settings.helius_laserstream_url.clone(),
            settings.helius_api_key.clone(),
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            value_tx,
            live_rx,
            pool_index.clone(),
            pools_changed.clone(),
            settings.reconnect_interval,
        ));

        tokio::spawn(ingest_laserstream::pipeline::run_pool_subscription_refresh(
            token_cache.clone(),
            pool_index.clone(),
            pools_changed.clone(),
            constants::PUMP_FUN_PROGRAM_ID.to_string(),
            settings_tx.subscribe(),
        ));

        // Weekly partition maintenance for raw_transactions (~2-month retention).
        tokio::spawn(ingest_laserstream::maintenance::run_partition_maintenance(
            db.clone(),
        ));

        let pipeline_task = tokio::spawn(pipeline.run(value_rx));
        let db_writer =
            ingest_laserstream::db_writer::DbWriter::new(db.clone(), trade_signals.clone());
        let db_writer_task = tokio::spawn(db_writer.run(db_rx));

        (
            pool_index,
            pools_changed,
            strategy_rx,
            producer_task,
            pipeline_task,
            db_writer_task,
        )
    };

    // Built after the transport branch so AppState shares the active pipeline's
    // pool→mint index and migration signal with the HTTP handlers (a token sync
    // registers a migrated token's pool so live ingest subscribes immediately).
    let app_state = Arc::new(state::AppState::new(
        db.clone(),
        settings.helius_rpc_url.clone(),
        settings.helius_laserstream_url.clone(),
        settings.helius_api_key.clone(),
        constants::PUMP_FUN_PROGRAM_ID.to_string(),
        token_cache.clone(),
        sse_tx.clone(),
        live_tx.clone(),
        settings_tx.clone(),
        sol_price.clone(),
        trader.clone(),
        tpsl1_cache.clone(),
        tpsl2_cache.clone(),
        pool_index,
        pools_changed,
        trade_signals.clone(),
    ));

    let strategy_runner = strategies::StrategyRunner::new(
        db.clone(),
        trader.clone(),
        token_cache.clone(),
        tpsl1_cache,
        tpsl2_cache,
        sse_tx.clone(),
        trade_signals.clone(),
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
        // Render each SSE event to wire bytes exactly once and fan the shared
        // frame out to all connections, instead of every subscriber re-rendering
        // (and re-reading the token cache) per event. Only needed when serving HTTP.
        tokio::spawn(api::handlers::system::run_sse_render_bridge(
            app_state.clone(),
        ));
        let http_state = app_state.clone();
        let http_workers = settings.http_workers;
        let cors_allowed_origin = settings.cors_allowed_origin.clone();
        let api_auth = ApiAuth {
            token: settings.api_auth_token.clone(),
        };
        if api_auth.token.is_some() {
            info!("API auth enabled: mutating requests require a bearer token");
        }
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
                .app_data(web::Data::new(http_state.clone()))
                .app_data(web::Data::new(api_auth.clone()))
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
        _ = producer_task => error!("Ingest producer task exited unexpectedly"),
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
