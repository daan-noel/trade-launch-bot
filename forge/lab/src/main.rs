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
mod lake;

use actix_web::{web, App, HttpServer};
use tracing::info;

use platform_core::config::Settings;
use platform_core::storage::connect;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lab=info,sqlx=error".into()),
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

    // Prefer HOST/PORT (what the container injects) so the docker bind is on
    // 0.0.0.0 and reachable via the published port; fall back to the
    // LAB_HOST/LAB_PORT local-dev convention (127.0.0.1:8240, the same number as
    // the deploy LAB_API_PORT).
    let host = std::env::var("HOST")
        .or_else(|_| std::env::var("LAB_HOST"))
        .unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .or_else(|_| std::env::var("LAB_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8240);
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
