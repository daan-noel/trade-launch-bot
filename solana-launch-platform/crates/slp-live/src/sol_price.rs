//! SOL/USD poller — updates `quote_assets.usd_rate` for the native (SOL) quote.
//! The quote's interned id + mint are resolved from the DB (`QuoteAssetRepo::native`),
//! never hardcoded. USDC stays seeded at 1.0; USD remains derived in views only (ADR D4).

use std::sync::Arc;
use std::time::Duration;

use platform_core::storage::repositories::QuoteAssetRepo;
use sqlx::PgPool;
use tracing::{info, warn};

const POLL_INTERVAL_SECS: u64 = 60;
const COINGECKO_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";

/// Fetch SOL/USD, preferring CoinGecko with a Jupiter fallback. `jup_mint` is the
/// native quote's mint (from `quote_assets`), used only for the Jupiter query.
pub async fn fetch_latest_sol_price(jup_mint: &str) -> anyhow::Result<f64> {
    match fetch_coingecko().await {
        Ok(price) => Ok(price),
        Err(cg_err) => {
            warn!("SOL price: CoinGecko failed ({cg_err}); trying Jupiter fallback");
            fetch_jupiter(jup_mint).await.map_err(|jup_err| {
                anyhow::anyhow!("both SOL price sources failed: coingecko={cg_err}; jupiter={jup_err}")
            })
        }
    }
}

async fn fetch_coingecko() -> anyhow::Result<f64> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let resp = client.get(COINGECKO_URL).send().await?.error_for_status()?;
    let v: serde_json::Value = resp.json().await?;
    v.get("solana")
        .and_then(|s| s.get("usd"))
        .and_then(|u| u.as_f64())
        .ok_or_else(|| anyhow::anyhow!("CoinGecko response missing solana.usd"))
}

async fn fetch_jupiter(mint: &str) -> anyhow::Result<f64> {
    let url = format!("https://api.jup.ag/price/v3?ids={mint}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let resp = client.get(&url).send().await?.error_for_status()?;
    let v: serde_json::Value = resp.json().await?;
    v.get(mint)
        .and_then(|e| e.get("usdPrice"))
        .and_then(|u| u.as_f64())
        .ok_or_else(|| anyhow::anyhow!("Jupiter response missing native-quote usdPrice"))
}

/// Background task: poll every 60s and write `quote_assets.usd_rate` for the
/// native quote. `quote_id` + `jup_mint` are resolved once by the caller from
/// `QuoteAssetRepo::native` — no hardcoded id/mint.
pub async fn run_poller(pool: Arc<PgPool>, quote_id: i16, jup_mint: String) {
    info!("SOL/USD poller: starting (every {POLL_INTERVAL_SECS}s)");

    loop {
        match fetch_latest_sol_price(&jup_mint).await {
            Ok(price) => {
                match QuoteAssetRepo::set_usd_rate(&pool, quote_id, price).await {
                    Ok(()) => info!("SOL/USD price: ${price:.2} (written to quote_assets)"),
                    Err(e) => warn!("SOL price poller: DB write failed: {e}"),
                }
            }
            Err(e) => warn!("SOL price poller: fetch failed: {e}"),
        }

        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}
