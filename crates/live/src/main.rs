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

use actix_web::{web, App, HttpServer};
use tracing::{info, warn};

use platform_core::config::Settings;
use platform_core::storage::connect;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let settings = Settings::from_env()?;
    let pools = connect(&settings).await?;
    info!("live box: DB connected, migrations applied");

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
                r = ingest    => warn!(?r, "ingest task ended — shutting down"),
                r = http_task => warn!(?r, "HTTP server ended — shutting down"),
            }
        }
        None => {
            let _ = http_task.await;
        }
    }
    Ok(())
}
