use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::services::http;

/// Wrapped-SOL mint — used to read a SOL/USD price from Jupiter as a fallback
/// when the primary (CoinGecko) source is down or rate-limited.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

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
    // `error_for_status` first so a 4xx/5xx (which can return an HTML body) fails
    // with the status instead of a confusing JSON-parse error.
    let data: HashMap<String, RawPriceEntry> = http::client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

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
