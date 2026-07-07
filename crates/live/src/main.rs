//! LIVE composition root — ships to EC2 (2vCPU / 4GB RAM).
//!
//! Owns the live workload: ingest (`ingest-host` over the borrowed
//! `ingest-laserstream` transport) + a thin HTTP surface, each a long-lived task
//! under `tokio::select!`. Launcher + trading land in later phases. Links the LIVE
//! half of the dep partition and deliberately NOT `lake`/DuckDB.
//!
//! Ingest requires Helius creds (`HELIUS_LASERSTREAM_URL` + `HELIUS_API_KEY`); when
//! absent, the box still boots and serves HTTP (ingest disabled) so the data layer
//! is inspectable without a live feed.

mod http;
mod sol_price;

use actix_web::{web, App, HttpServer};
use std::sync::Arc;
use tracing::{info, warn};

use platform_core::config::Settings;
use platform_core::storage::connect;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "live=info,sqlx=error".into()),
        )
        .init();

    let settings = Settings::from_env()?;

    // Subcommand: `live -- wallet-encrypt <keypair.json> <key_ref>` (no HTTP/ingest).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("wallet-encrypt") {
        launcher::run_wallet_encrypt(&args[1..])?;
        return Ok(());
    }

    let pools = connect(&settings).await?;
    info!("live box: DB connected, migrations applied");

    // SOL/USD poller — keeps quote_assets.usd_rate fresh for trades_priced views.
    let price_pool = Arc::new(pools.hot.clone());
    match sol_price::fetch_latest_sol_price().await {
        Ok(price) => {
            if let Err(e) =
                platform_core::storage::repositories::QuoteAssetRepo::set_usd_rate(
                    &pools.hot,
                    1,
                    price,
                )
                .await
            {
                warn!("initial SOL/USD DB write failed: {e}");
            } else {
                info!("SOL/USD seeded: ${price:.2}");
            }
        }
        Err(e) => warn!("initial SOL/USD fetch failed (poller will retry): {e}"),
    }
    let price_task = tokio::spawn(sol_price::run_poller(price_pool));

    // Feed-based bundle-landing confirmation — cheap, always on (no RPC/keys
    // needed; it only reads `bundles`/`trades` from the already-connected pool).
    let bundle_confirm_task = launcher::spawn_bundle_confirm_watcher(pools.hot.clone());

    // Ingest (optional): spawn only when Helius creds are present.
    let ingest_task = match (
        std::env::var("HELIUS_LASERSTREAM_URL"),
        std::env::var("HELIUS_API_KEY"),
    ) {
        (Ok(endpoint), Ok(api_key)) if !endpoint.is_empty() && !api_key.is_empty() => {
            Some(ingest_host::spawn_ingest(pools.hot.clone(), endpoint, api_key).await?)
        }
        _ => {
            warn!("ingest disabled — HELIUS_LASERSTREAM_URL / HELIUS_API_KEY not set; serving HTTP only");
            None
        }
    };

    // Thin HTTP surface over the api pool.
    let host = std::env::var("LIVE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("LIVE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8091);
    let api_pool = pools.api.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(api_pool.clone()))
            .configure(http::configure)
    })
    .bind((host.as_str(), port))?
    .run();
    info!(%host, port, "live HTTP listening");
    let http_task = tokio::spawn(server);

    match ingest_task {
        Some(ingest) => {
            tokio::select! {
                r = ingest              => warn!(?r, "ingest task ended — shutting down"),
                r = http_task           => warn!(?r, "HTTP server ended — shutting down"),
                r = bundle_confirm_task => warn!(?r, "bundle confirm watcher ended — shutting down"),
                r = price_task          => warn!(?r, "SOL price poller ended — shutting down"),
            }
        }
        None => {
            tokio::select! {
                r = http_task           => warn!(?r, "HTTP server ended — shutting down"),
                r = bundle_confirm_task => warn!(?r, "bundle confirm watcher ended — shutting down"),
                r = price_task          => warn!(?r, "SOL price poller ended — shutting down"),
            }
        }
    }
    Ok(())
}
