//! ANALYSIS composition root — workstation only (never ships to EC2).
//!
//! Runs with NO signing keys and NO gRPC. Two modes:
//!   * `lab -- lake-export [--include-today]` — seal PG days → Parquet (stub; the
//!     lake pipeline fills later). Reads the synced LOCAL PG mirror.
//!   * default — a thin analysis HTTP surface over the local mirror.
//!
//! Links the LAB half of the dep partition (`lake`) and deliberately NOT
//! ingest-host / launcher / pump-trader / ingest-laserstream.

mod http;

use actix_web::{web, App, HttpServer};
use tracing::info;

use platform_core::config::Settings;
use platform_core::storage::connect;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let settings = Settings::from_env()?;

    // Subcommand: `lab -- lake-export [--include-today]`.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("lake-export") {
        let include_today = args.iter().any(|a| a == "--include-today");
        let n = lake::run_export(&settings.database_url, include_today).await?;
        info!(exported = n, include_today, "lake-export finished");
        return Ok(());
    }

    // Default: HTTP analysis surface over the local mirror.
    let pools = connect(&settings).await?;
    info!("lab box: DB connected, migrations applied (local mirror)");

    let host = std::env::var("LAB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("LAB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8092);
    let api_pool = pools.api.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(api_pool.clone()))
            .configure(http::configure)
    })
    .bind((host.as_str(), port))?
    .run();
    info!(%host, port, "lab HTTP listening");
    server.await?;
    Ok(())
}
