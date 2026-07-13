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
mod ingest;
mod sol_price;

use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpServer};
use std::sync::Arc;
use tracing::{info, warn};

// Same fail-closed bearer gate hunter's real-money bins use — forge-live moves
// treasury SOL (fund / launch / manage), so its mutating routes require it too.
use http_auth::{require_bearer_auth, ApiAuth};

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
    if args.first().map(String::as_str) == Some("bundle-simulate") {
        // Read-only pre-flight: rebuild a persisted launch bundle and run Jito
        // simulateBundle against it (no submit, no SOL). Diagnoses a bundle that
        // is accepted but never lands (a leg reverts). Needs the launcher env.
        launcher::run_bundle_simulate(&settings, &args[1..]).await?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("create-alt") {
        // Provision the persistent launch ALT (spends real SOL). Needs the
        // launcher env (keystore + KEK + RPC), so build LauncherSettings here.
        let launcher_settings = LauncherSettings::from_env()?;
        launcher::run_create_alt(&launcher_settings, &args[1..]).await?;
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

    // Notify over poll: the ingest DbWriter pings this after committing trades so
    // the bundle-confirm watcher re-checks its pending leg signatures immediately
    // instead of blind-polling. Created unconditionally — if ingest is disabled
    // (no Helius creds) it simply never fires and the watcher's slow fallback tick
    // carries it (same as the old poll, just slower).
    let trades_notify = std::sync::Arc::new(tokio::sync::Notify::new());

    // Feed-based bundle-landing confirmation — cheap, always on (confirming reads
    // only `bundles`/`trades` from the already-connected pool). The launcher
    // settings (when configured) additionally let it auto re-bid a `dropped`
    // bundle at a higher Jito tip before conceding; without them it stays
    // read-only (confirm/mark only).
    let bundle_confirm_task = launcher::spawn_bundle_confirm_watcher(
        pools.hot.clone(),
        launcher_settings.clone(),
        trades_notify.clone(),
    );

    // Fresh-wallet pool: balance poller (generated/funding -> funded) +
    // reservation TTL sweep (Phase 1) + dust sweep (used -> retired, Phase 4) +
    // treasury->pool funder (wallet-funding-plan; only when FUND_ENABLED) +
    // sell-ladder evaluator (token-management Phase 4; self-skips firing until
    // MANAGE_ENABLED).
    let (
        wallet_balance_task,
        wallet_sweep_task,
        wallet_dust_sweep_task,
        wallet_funding_task,
        ladder_task,
        volume_task,
    ) = match launcher_settings.as_ref() {
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
                Some(launcher::spawn_ladder_evaluator(pools.hot.clone(), s.clone())),
                Some(launcher::spawn_volume_scheduler(pools.hot.clone(), s.clone())),
            )
        }
        None => (None, None, None, None, None, None),
    };

    // Ingest (optional): spawn only when Helius creds are present. `ingest_handle`
    // is kept for the process lifetime and exposed over HTTP so an operator can
    // pause/resume the stream at runtime without a redeploy.
    let (ingest_task, ingest_handle) = match (
        std::env::var("HELIUS_LASERSTREAM_URL"),
        std::env::var("HELIUS_API_KEY"),
    ) {
        (Ok(endpoint), Ok(api_key)) if !endpoint.is_empty() && !api_key.is_empty() => {
            let (task, handle) =
                ingest::spawn_ingest(pools.hot.clone(), endpoint, api_key, trades_notify.clone())
                    .await?;
            (Some(task), Some(handle))
        }
        _ => {
            warn!("ingest disabled — HELIUS_LASERSTREAM_URL / HELIUS_API_KEY not set; serving HTTP only");
            (None, None)
        }
    };

    // Fail-closed bearer auth on mutating routes. Required at HTTP-startup (not
    // for the CLI subcommands above, which return before this) so the launch /
    // fund / manage endpoints are never reachable without the server-side token
    // the reverse proxy injects. Absent token ⇒ refuse to boot the server rather
    // than serve real-money routes wide open.
    let api_auth_token = std::env::var("API_AUTH_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("API_AUTH_TOKEN is required to serve HTTP (see forge/.env.example)")
        })?;
    let api_auth = ApiAuth { token: Some(api_auth_token) };
    info!("API auth enabled (fail-closed): mutating requests require a bearer token");

    // Thin HTTP surface over the api pool. Prefer HOST/PORT (what the container
    // injects — the compose LIVE_API_PORT, so nginx's envsubst upstream matches),
    // falling back to the LIVE_HOST/LIVE_PORT local-dev convention (127.0.0.1:8230,
    // the same number as the deploy LIVE_API_PORT).
    let host = std::env::var("HOST")
        .or_else(|_| std::env::var("LIVE_HOST"))
        .unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .or_else(|_| std::env::var("LIVE_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8230);
    let api_pool = pools.api.clone();
    let server = HttpServer::new(move || {
        App::new()
            .wrap(from_fn(require_bearer_auth))
            .app_data(web::Data::new(api_pool.clone()))
            .app_data(web::Data::new(ingest_handle.clone()))
            .app_data(web::Data::new(launcher_settings.clone()))
            .app_data(web::Data::new(api_auth.clone()))
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
    let ladder = async {
        match ladder_task {
            Some(h) => {
                let _ = h.await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let volume = async {
        match volume_task {
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
                _ = ladder              => warn!("ladder evaluator ended — shutting down"),
                _ = volume              => warn!("volume scheduler ended — shutting down"),
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
                _ = ladder              => warn!("ladder evaluator ended — shutting down"),
                _ = volume              => warn!("volume scheduler ended — shutting down"),
            }
        }
    }
    Ok(())
}
