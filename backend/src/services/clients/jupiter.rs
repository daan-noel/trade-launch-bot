use std::collections::HashMap;
use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::warn;

use crate::config::constants::WSOL_MINT;
use crate::services::http;

/// Bounded retry on transient failures (network error, 5xx, 429): a single
/// transient blip shouldn't drop a price refresh on the floor. Mirrors the
/// CoinGecko client's backoff; 4xx (other than 429) is not retried.
const MAX_ATTEMPTS: usize = 3;
const INITIAL_BACKOFF: Duration = Duration::from_millis(300);
const MAX_BACKOFF: Duration = Duration::from_secs(4);

/// Raw per-mint entry from Jupiter price v3. Map keyed by mint address.
#[derive(Debug, Default, Deserialize)]
struct RawPriceEntry {
    #[serde(rename = "usdPrice")]
    usd_price: Option<f64>,
    liquidity: Option<f64>,
    #[serde(rename = "priceChange24h")]
    price_change_24h: Option<f64>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct JupiterPriceEntry {
    pub price_usd: Option<f64>,
    pub liquidity: Option<f64>,
    pub price_change_24h: Option<f64>,
    pub token_created_at: Option<String>,
}

pub async fn fetch_prices(mints: &[String]) -> anyhow::Result<HashMap<String, JupiterPriceEntry>> {
    if mints.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = mints.join(",");
    let url = format!("https://api.jup.ag/price/v3?ids={ids}");

    let mut backoff = INITIAL_BACKOFF;
    let mut last_err: Option<anyhow::Error> = None;
    let mut fetched: Option<HashMap<String, RawPriceEntry>> = None;
    for attempt in 0..MAX_ATTEMPTS {
        match http::client().get(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                // 429 / 5xx are transient: back off and retry.
                if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    warn!("Jupiter {status} (attempt {attempt}); backing off {backoff:?}");
                    last_err = Some(anyhow::anyhow!("Jupiter transient status {status}"));
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
                // `error_for_status` so a non-2xx (which can return an HTML body)
                // fails with the status, not a confusing JSON-parse error.
                fetched = Some(resp.error_for_status()?.json().await?);
                break;
            }
            Err(e) => {
                // Network/timeout: back off and retry.
                warn!("Jupiter request failed (attempt {attempt}): {e}");
                last_err = Some(anyhow::anyhow!("Jupiter request: {e}"));
                sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
    let data = match fetched {
        Some(d) => d,
        None => return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Jupiter: exhausted retries"))),
    };

    let result = data
        .into_iter()
        .map(|(mint, entry)| {
            (
                mint,
                JupiterPriceEntry {
                    price_usd: entry.usd_price,
                    liquidity: entry.liquidity,
                    price_change_24h: entry.price_change_24h,
                    token_created_at: entry.created_at,
                },
            )
        })
        .collect();
    Ok(result)
}

/// Read SOL/USD from Jupiter (via the WSOL mint). Secondary source behind
/// CoinGecko for the price poller.
pub async fn fetch_sol_usd() -> anyhow::Result<f64> {
    let prices = fetch_prices(&[WSOL_MINT.to_string()]).await?;
    prices
        .get(WSOL_MINT)
        .and_then(|e| e.price_usd)
        .ok_or_else(|| anyhow::anyhow!("Jupiter returned no USD price for WSOL"))
}
