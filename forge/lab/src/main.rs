//! ANALYSIS composition root — workstation only (never ships to EC2).
//!
//! Runs with NO signing keys and NO gRPC. Serves a thin analysis HTTP surface over
//! the synced LOCAL PG mirror. (The `lake-export` subcommand was removed — it was a
//! do-nothing stub; the Parquet export/reader pipeline will re-add a real one. The
//! column-SSOT seam it will build on survives in `lake::schema`.)
//!
//! Links the LAB half of the dep partition (`lake`) and deliberately NOT
//! ingest-host / launcher / pump-trader / ingest-laserstream.

mod http;
mod lake;

use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpServer};
use http_auth::{require_bearer_auth, ApiAuth};
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

    // No subcommands today (the `lake-export` stub was removed). Reject any arg
    // explicitly instead of silently falling through to the HTTP server.
    if let Some(arg) = std::env::args().nth(1) {
        anyhow::bail!("unknown argument {arg:?} — forge-lab takes no subcommands");
    }

    // HTTP analysis surface over the local mirror.
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
    // Same shared fail-closed bearer gate as forge-live + hunter live/lab, so the
    // four bins can't drift on the auth decision. lab's surface is read-only today,
    // and the gate lets safe reads (GET/HEAD) through — so this is defense in depth
    // that fail-closes any future mutating lab route. The token is OPTIONAL here
    // (unlike forge-live, which refuses to boot without it): lab moves no SOL and
    // runs on a workstation, and reads pass regardless; primary protection is the
    // loopback publish + nginx in deploy.
    let api_auth = ApiAuth {
        token: std::env::var("API_AUTH_TOKEN").ok().filter(|t| !t.is_empty()),
    };
    let api_pool = pools.api.clone();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(from_fn(require_bearer_auth))
            .app_data(web::Data::new(api_pool.clone()))
            .app_data(web::Data::new(api_auth.clone()))
            .configure(http::configure)
    })
    .bind((host.as_str(), port))?
    .run();
    info!(%host, port, "lab HTTP listening");
    server.await?;
    Ok(())
}
