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

use launcher::LauncherSettings;
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
    if args.first().map(String::as_str) == Some("wallet-verify") {
        launcher::run_wallet_verify(&args[1..])?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("wallet-export") {
        launcher::run_wallet_export(&settings, &args[1..]).await?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("launch-probe") {
        launcher::run_launch_probe(&settings, &args[1..]).await?;
        return Ok(());
    }

    let pools = connect(&settings).await?;
    info!("live box: DB connected, migrations applied");

    // SOL/USD poller — keeps quote_assets.usd_rate fresh for trades_priced views.
    // Resolve the native quote's interned id + mint from the DB (never hardcoded).
    let price_pool = Arc::new(pools.hot.clone());
    let native_quote =
        platform_core::storage::repositories::QuoteAssetRepo::native(&pools.hot).await?;
    match sol_price::fetch_latest_sol_price(&native_quote.mint).await {
        Ok(price) => {
            if let Err(e) =
                platform_core::storage::repositories::QuoteAssetRepo::set_usd_rate(
                    &pools.hot,
                    native_quote.id,
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
    let price_task = tokio::spawn(sol_price::run_poller(
        price_pool,
        native_quote.id,
        native_quote.mint,
    ));

    // Feed-based bundle-landing confirmation — cheap, always on (no RPC/keys
    // needed; it only reads `bundles`/`trades` from the already-connected pool).
    let bundle_confirm_task = launcher::spawn_bundle_confirm_watcher(pools.hot.clone());

    // Launcher settings — built ONCE at boot (single source of truth) and shared
    // by BOTH the wallet-pool background tasks below AND the HTTP handlers (via
    // `app_data`), instead of each HTTP request re-parsing ~15 env vars.
    let launcher_settings: Option<LauncherSettings> = match LauncherSettings::from_env() {
        Ok(s) => Some(s),
        Err(e) => {
            warn!(
                "launcher not configured — launch/fund/generate/metadata endpoints \
                 and wallet-pool background tasks disabled: {e}"
            );
            None
        }
    };

    // Fresh-wallet pool: balance poller (generated/funding -> funded) +
    // reservation TTL sweep (Phase 1) + dust sweep (used -> retired, Phase 4) +
    // treasury->pool funder (wallet-funding-plan; only when FUND_ENABLED).
    let (wallet_balance_task, wallet_sweep_task, wallet_dust_sweep_task, wallet_funding_task) =
        match launcher_settings.as_ref() {
            Some(s) => {
                let funding_task = if s.funding.is_some() {
                    Some(launcher::spawn_wallet_funding(pools.hot.clone(), s.clone()))
                } else {
                    warn!("wallet funding disabled — set FUND_ENABLED=true to enable");
                    None
                };
                (
                    Some(launcher::spawn_balance_poller(
                        pools.hot.clone(),
                        s.rpc_url.clone(),
                    )),
                    Some(launcher::spawn_reservation_sweep(pools.hot.clone())),
                    Some(launcher::spawn_dust_sweep(pools.hot.clone(), s.clone())),
                    funding_task,
                )
            }
            None => (None, None, None, None),
        };

    // Ingest (optional): spawn only when Helius creds are present. `ingest_handle`
    // is kept for the process lifetime and exposed over HTTP so an operator can
    // pause/resume the stream at runtime without a redeploy.
    let (ingest_task, ingest_handle) = match (
        std::env::var("HELIUS_LASERSTREAM_URL"),
        std::env::var("HELIUS_API_KEY"),
    ) {
        (Ok(endpoint), Ok(api_key)) if !endpoint.is_empty() && !api_key.is_empty() => {
            let (task, handle) = ingest_host::spawn_ingest(pools.hot.clone(), endpoint, api_key).await?;
            (Some(task), Some(handle))
        }
        _ => {
            warn!("ingest disabled — HELIUS_LASERSTREAM_URL / HELIUS_API_KEY not set; serving HTTP only");
            (None, None)
        }
    };

    // Thin HTTP surface over the api pool.
    let host = std::env::var("LIVE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("LIVE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8091);
    let api_pool = pools.api.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(api_pool.clone()))
            .app_data(web::Data::new(ingest_handle.clone()))
            .app_data(web::Data::new(launcher_settings.clone()))
            .configure(http::configure)
    })
    .bind((host.as_str(), port))?
    .run();
    info!(%host, port, "live HTTP listening");
    let http_task = tokio::spawn(server);

    // Optional tasks become always-pollable futures (pending forever when absent)
    // so they can join the single `tokio::select!` below without duplicating it
    // per combination of enabled tasks.
    let wallet_balance = async {
        match wallet_balance_task {
            Some(h) => {
                let _ = h.await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let wallet_sweep = async {
        match wallet_sweep_task {
            Some(h) => {
                let _ = h.await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let wallet_dust_sweep = async {
        match wallet_dust_sweep_task {
            Some(h) => {
                let _ = h.await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let wallet_funding = async {
        match wallet_funding_task {
            Some(h) => {
                let _ = h.await;
            }
            None => std::future::pending::<()>().await,
        }
    };

    match ingest_task {
        Some(ingest) => {
            tokio::select! {
                r = ingest              => warn!(?r, "ingest task ended — shutting down"),
                r = http_task           => warn!(?r, "HTTP server ended — shutting down"),
                r = bundle_confirm_task => warn!(?r, "bundle confirm watcher ended — shutting down"),
                r = price_task          => warn!(?r, "SOL price poller ended — shutting down"),
                _ = wallet_balance      => warn!("wallet balance poller ended — shutting down"),
                _ = wallet_sweep        => warn!("wallet reservation sweep ended — shutting down"),
                _ = wallet_dust_sweep   => warn!("wallet dust sweep ended — shutting down"),
                _ = wallet_funding      => warn!("wallet funding task ended — shutting down"),
            }
        }
        None => {
            tokio::select! {
                r = http_task           => warn!(?r, "HTTP server ended — shutting down"),
                r = bundle_confirm_task => warn!(?r, "bundle confirm watcher ended — shutting down"),
                r = price_task          => warn!(?r, "SOL price poller ended — shutting down"),
                _ = wallet_balance      => warn!("wallet balance poller ended — shutting down"),
                _ = wallet_sweep        => warn!("wallet reservation sweep ended — shutting down"),
                _ = wallet_dust_sweep   => warn!("wallet dust sweep ended — shutting down"),
                _ = wallet_funding      => warn!("wallet funding task ended — shutting down"),
            }
        }
    }
    Ok(())
}
