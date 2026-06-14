use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::services::http;

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
    let data: HashMap<String, RawPriceEntry> =
        http::client().get(&url).send().await?.json().await?;

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
